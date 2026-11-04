//! Cookie jar (FR-COOKIE): keeps `Set-Cookie` values per RFC 6265 (domain, path, expiry,
//! Secure) and sends matching cookies back, like a browser. Wraps `cookie_store` (the same
//! engine reqwest's own jar uses) and adds listing, deleting and saving to disk.
use std::io::{BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use cookie_store::{CookieExpiration, CookieStore, RawCookie};
use reqwest::header::HeaderValue;
use reqwest::Url;

/// One stored cookie, for display.
#[derive(Clone, Debug, PartialEq)]
pub struct CookieInfo {
    pub domain: String,
    pub path: String,
    pub name: String,
    pub value: String,
    /// Unix seconds; `None` = session cookie.
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
}

pub struct CookieJar {
    store: Mutex<CookieStore>,
    changed: AtomicBool,
}

impl std::fmt::Debug for CookieJar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CookieJar({} cookies)", self.list().len())
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}

impl CookieJar {
    pub fn new() -> CookieJar {
        CookieJar {
            store: Mutex::new(CookieStore::new()),
            changed: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> MutexGuard<'_, CookieStore> {
        // a poisoned lock only means another thread panicked mid-update; the store is still usable
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Load a jar saved with [`CookieJar::save`]; a missing file is an empty jar.
    /// Expired cookies are dropped on load.
    pub fn load(path: &Path) -> Result<CookieJar, String> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CookieJar::new()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let store = cookie_store::serde::json::load(BufReader::new(file))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(CookieJar {
            store: Mutex::new(store),
            changed: AtomicBool::new(false),
        })
    }

    /// Save all live cookies, session cookies included (so a login survives a restart, as in
    /// other API tools). Atomic: temp file + rename.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut buf = Vec::new();
        cookie_store::serde::json::save_incl_expired_and_nonpersistent(&self.lock(), &mut buf)
            .map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&buf)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| format!("{}: {e}", path.display()))
    }

    /// True once after cookies changed (set by a response, or edited); for "save when dirty".
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }

    /// Live cookies, sorted by domain, path, name.
    pub fn list(&self) -> Vec<CookieInfo> {
        let store = self.lock();
        let mut out: Vec<CookieInfo> = store
            .iter_unexpired()
            .map(|c| CookieInfo {
                domain: c
                    .domain
                    .as_cow()
                    .map(|d| d.into_owned())
                    .unwrap_or_default(),
                path: c.path.to_string(),
                name: c.name().to_string(),
                value: c.value().to_string(),
                expires: match &c.expires {
                    CookieExpiration::AtUtc(t) => Some(t.unix_timestamp()),
                    CookieExpiration::SessionEnd => None,
                },
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
            })
            .collect();
        out.sort_by(|a, b| {
            (a.domain.as_str(), a.path.as_str(), a.name.as_str()).cmp(&(
                b.domain.as_str(),
                b.path.as_str(),
                b.name.as_str(),
            ))
        });
        out
    }

    pub fn remove(&self, domain: &str, path: &str, name: &str) -> bool {
        let removed = self.lock().remove(domain, path, name).is_some();
        if removed {
            self.changed.store(true, Ordering::Relaxed);
        }
        removed
    }

    /// Remove every cookie of one domain; returns how many.
    pub fn clear_domain(&self, domain: &str) -> usize {
        let victims: Vec<CookieInfo> = self
            .list()
            .into_iter()
            .filter(|c| c.domain == domain)
            .collect();
        for c in &victims {
            self.remove(&c.domain, &c.path, &c.name);
        }
        victims.len()
    }

    pub fn clear(&self) {
        self.lock().clear();
        self.changed.store(true, Ordering::Relaxed);
    }

    /// Store one `Set-Cookie` value as if it came from `url` (also used by tests).
    pub fn set_cookie(&self, set_cookie: &str, url: &str) {
        if let Ok(url) = Url::parse(url) {
            if let Ok(c) = RawCookie::parse(set_cookie.to_owned()) {
                self.lock().store_response_cookies(std::iter::once(c), &url);
                self.changed.store(true, Ordering::Relaxed);
            }
        }
    }

    /// The `Cookie` header value that would be sent to `url`.
    pub fn header_for(&self, url: &str) -> Option<String> {
        let url = Url::parse(url).ok()?;
        let s = self
            .lock()
            .get_request_values(&url)
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        (!s.is_empty()).then_some(s)
    }
}

impl reqwest::cookie::CookieStore for CookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let cookies: Vec<RawCookie<'static>> = cookie_headers
            .filter_map(|v| std::str::from_utf8(v.as_bytes()).ok())
            .filter_map(|s| RawCookie::parse(s.to_owned()).ok())
            .collect();
        if !cookies.is_empty() {
            self.lock().store_response_cookies(cookies.into_iter(), url);
            self.changed.store(true, Ordering::Relaxed);
        }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let s = self
            .lock()
            .get_request_values(url)
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if s.is_empty() {
            None
        } else {
            HeaderValue::from_str(&s).ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_path_and_secure_matching() {
        let jar = CookieJar::new();
        jar.set_cookie("sid=1; Path=/", "https://api.test/login");
        jar.set_cookie("admin=2; Path=/admin", "https://api.test/admin/x");
        jar.set_cookie("tls=3; Secure", "https://api.test/");
        jar.set_cookie("wide=4; Domain=api.test", "https://www.api.test/");
        let sorted = |h: Option<String>| {
            let mut v: Vec<String> = h
                .unwrap_or_default()
                .split("; ")
                .map(String::from)
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            sorted(jar.header_for("https://api.test/users")),
            vec!["sid=1", "tls=3", "wide=4"]
        );
        let admin = jar.header_for("https://api.test/admin/panel").unwrap();
        assert!(
            admin.contains("admin=2") && admin.contains("sid=1"),
            "{admin}"
        );
        let plain = jar.header_for("http://api.test/").unwrap();
        assert!(
            !plain.contains("tls=3"),
            "Secure cookies stay on https: {plain}"
        );
        assert_eq!(jar.header_for("https://other.test/"), None);
        assert!(jar
            .header_for("https://sub.api.test/")
            .unwrap()
            .contains("wide=4"));
    }

    #[test]
    fn expiry_overwrite_and_delete() {
        let jar = CookieJar::new();
        jar.set_cookie("a=1", "https://x.test/");
        jar.set_cookie("a=2", "https://x.test/");
        assert_eq!(
            jar.header_for("https://x.test/").as_deref(),
            Some("a=2"),
            "same name overwrites"
        );
        jar.set_cookie("a=gone; Max-Age=0", "https://x.test/");
        assert_eq!(jar.header_for("https://x.test/"), None, "Max-Age=0 deletes");
        jar.set_cookie("b=1; Max-Age=3600", "https://x.test/");
        let list = jar.list();
        assert_eq!(list.len(), 1);
        assert!(list[0].expires.is_some(), "persistent cookie has an expiry");
        jar.set_cookie("s=1; HttpOnly", "https://x.test/");
        let s = jar.list().into_iter().find(|c| c.name == "s").unwrap();
        assert!(s.expires.is_none() && s.http_only);
        assert!(jar.remove("x.test", "/", "b"));
        assert!(!jar.remove("x.test", "/", "b"));
        assert_eq!(jar.clear_domain("x.test"), 1);
        assert!(jar.list().is_empty());
    }

    #[test]
    fn save_load_and_change_tracking() {
        let dir = std::env::temp_dir().join(format!("at-cookies-{}", std::process::id()));
        let path = dir.join("cookies.json");
        let _ = std::fs::remove_file(&path);
        assert!(
            CookieJar::load(&path).unwrap().list().is_empty(),
            "missing file = empty"
        );

        let jar = CookieJar::new();
        assert!(!jar.take_changed());
        jar.set_cookie("p=1; Max-Age=3600", "https://s.test/");
        jar.set_cookie("session=2", "https://s.test/");
        assert!(jar.take_changed());
        assert!(!jar.take_changed(), "reported once");
        jar.save(&path).unwrap();
        let back = CookieJar::load(&path).unwrap();
        let names: Vec<_> = back.list().into_iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            vec!["p", "session"],
            "session cookies survive a restart too"
        );

        std::fs::write(&path, "not json").unwrap();
        assert!(CookieJar::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

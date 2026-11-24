//! App-wide TLS options (G-17): extra root certificates, client certificates per host, and a
//! minimum TLS version.
//!
//! The backend is rustls (`reqwest`'s `rustls-tls`), so a client certificate must be PEM:
//! one file holding the certificate chain and an unencrypted private key. PKCS#12
//! (`.p12` / `.pfx`) needs reqwest's `native-tls` backend, which this build does not use;
//! such a file gets an error that says how to convert it.
//!
//! [`EngineConfig::default`](crate::EngineConfig) starts from a process-wide default set with
//! [`set_default_tls`], so callers that build their config with `..Default::default()` (the
//! request pipeline in `at-runner`) pick up the app's Settings without having to pass them.
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

use crate::HttpError;

/// Lowest TLS version a connection may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsVersion {
    Tls12,
    Tls13,
}

impl TlsVersion {
    /// `"1.2"` / `"1.3"` (as stored in settings); anything else is `None`.
    pub fn parse(s: &str) -> Option<TlsVersion> {
        match s.trim() {
            "1.2" => Some(TlsVersion::Tls12),
            "1.3" => Some(TlsVersion::Tls13),
            _ => None,
        }
    }

    fn to_reqwest(self) -> reqwest::tls::Version {
        match self {
            TlsVersion::Tls12 => reqwest::tls::Version::TLS_1_2,
            TlsVersion::Tls13 => reqwest::tls::Version::TLS_1_3,
        }
    }
}

/// A client certificate presented to one host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientCert {
    /// Host name it is for, matched exactly (case-insensitive, no port, no wildcards).
    pub host: String,
    /// PEM file with the certificate (chain) and its private key.
    pub path: PathBuf,
}

/// TLS options that apply to every request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TlsConfig {
    /// PEM bundle of extra trusted roots, added to the built-in ones.
    pub ca_bundle: Option<PathBuf>,
    pub client_certs: Vec<ClientCert>,
    pub min_tls: Option<TlsVersion>,
}

impl TlsConfig {
    /// The client certificate for `host`, if one is configured.
    pub fn cert_for(&self, host: &str) -> Option<&ClientCert> {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        self.client_certs
            .iter()
            .find(|c| !c.host.trim().is_empty() && c.host.trim().eq_ignore_ascii_case(host))
    }
}

static DEFAULT_TLS: RwLock<Option<TlsConfig>> = RwLock::new(None);

/// Set the TLS options `EngineConfig::default()` starts from (the app calls this at startup
/// and whenever Settings are saved).
pub fn set_default_tls(tls: TlsConfig) {
    *DEFAULT_TLS.write().unwrap_or_else(|p| p.into_inner()) = Some(tls);
}

/// The TLS options set with [`set_default_tls`]; none until then.
pub fn default_tls() -> TlsConfig {
    DEFAULT_TLS
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default()
}

/// A file as part of a cache key: its path and when it last changed, so editing the file on
/// disk builds a fresh client instead of reusing one made from the old contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileKey {
    path: PathBuf,
    modified: Option<SystemTime>,
}

impl FileKey {
    pub(crate) fn of(path: &Path) -> FileKey {
        FileKey {
            path: path.to_path_buf(),
            modified: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
        }
    }
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>, HttpError> {
    std::fs::read(path)
        .map_err(|e| HttpError::Tls(format!("cannot read {what} {}: {e}", path.display())))
}

/// Add the CA bundle, the client identity and the minimum version to a client builder.
pub(crate) fn apply(
    mut cb: reqwest::ClientBuilder,
    ca_bundle: Option<&Path>,
    identity: Option<&Path>,
    min_tls: Option<TlsVersion>,
) -> Result<reqwest::ClientBuilder, HttpError> {
    if let Some(path) = ca_bundle {
        let pem = read(path, "CA bundle")?;
        let certs = reqwest::Certificate::from_pem_bundle(&pem).map_err(|e| {
            HttpError::Tls(format!(
                "CA bundle {} is not valid PEM: {e}",
                path.display()
            ))
        })?;
        if certs.is_empty() {
            return Err(HttpError::Tls(format!(
                "CA bundle {} holds no PEM certificates",
                path.display()
            )));
        }
        for c in certs {
            cb = cb.add_root_certificate(c);
        }
    }
    if let Some(path) = identity {
        cb = cb.identity(load_identity(path)?);
    }
    if let Some(v) = min_tls {
        cb = cb.min_tls_version(v.to_reqwest());
    }
    Ok(cb)
}

fn load_identity(path: &Path) -> Result<reqwest::Identity, HttpError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "p12" || ext == "pfx" {
        return Err(HttpError::Tls(format!(
            "client certificate {}: PKCS#12 (.p12/.pfx) isn't supported, only PEM. Convert it with \
             `openssl pkcs12 -in {} -out cert.pem -nodes`",
            path.display(),
            path.display()
        )));
    }
    let pem = read(path, "client certificate")?;
    if String::from_utf8_lossy(&pem).contains("ENCRYPTED PRIVATE KEY") {
        return Err(HttpError::Tls(format!(
            "client certificate {}: the private key is encrypted; only unencrypted PEM keys are supported",
            path.display()
        )));
    }
    reqwest::Identity::from_pem(&pem).map_err(|e| {
        let mut text = e.to_string();
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            text.push_str(": ");
            text.push_str(&s.to_string());
            src = s.source();
        }
        HttpError::Tls(format!(
            "client certificate {} must be one PEM file with the certificate and its private key ({text})",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse() {
        assert_eq!(TlsVersion::parse("1.2"), Some(TlsVersion::Tls12));
        assert_eq!(TlsVersion::parse(" 1.3 "), Some(TlsVersion::Tls13));
        assert_eq!(TlsVersion::parse(""), None);
        assert_eq!(TlsVersion::parse("1.1"), None);
    }

    #[test]
    fn client_certs_match_the_exact_host() {
        let tls = TlsConfig {
            client_certs: vec![
                ClientCert {
                    host: "api.example.com".into(),
                    path: "a.pem".into(),
                },
                ClientCert {
                    host: String::new(),
                    path: "blank.pem".into(),
                },
                ClientCert {
                    host: "::1".into(),
                    path: "v6.pem".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            tls.cert_for("API.example.com").unwrap().path,
            PathBuf::from("a.pem")
        );
        assert!(tls.cert_for("example.com").is_none());
        assert!(tls.cert_for("x.api.example.com").is_none());
        assert!(tls.cert_for("").is_none(), "a blank host matches nothing");
        assert_eq!(tls.cert_for("[::1]").unwrap().path, PathBuf::from("v6.pem"));
    }
}

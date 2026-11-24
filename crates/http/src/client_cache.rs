//! A small bounded cache of `reqwest::Client`s (G-10).
//!
//! Building a client per request throws away the connection pool and redoes the TLS setup on
//! every Send. A client is reused for every request whose *client-level* settings match; the
//! timeout is not one of them, it is applied per request with `RequestBuilder::timeout`.
use std::sync::{Arc, Mutex};

use at_core::RequestSettings;

use crate::tls::{self, FileKey, TlsVersion};
use crate::{CookieJar, EngineConfig, HttpError};

/// How many distinct clients are kept. Least recently used ones are dropped first.
pub const DEFAULT_CAPACITY: usize = 16;

/// Everything that requires a different `reqwest::Client`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientKey {
    verify_tls: bool,
    follow_redirects: bool,
    /// `EngineConfig.proxy`, empty = no explicit proxy.
    proxy: String,
    /// Pointer identity of `EngineConfig.cookies`. The cached client holds a clone of that `Arc`,
    /// so the jar cannot be freed (and its address reused) while the entry is in the cache.
    jar: Option<usize>,
    /// `EngineConfig.tls.ca_bundle`, with its modification time.
    ca_bundle: Option<FileKey>,
    /// The client certificate configured for the request's host, with its modification time.
    identity: Option<FileKey>,
    min_tls: Option<TlsVersion>,
}

impl ClientKey {
    pub(crate) fn new(settings: &RequestSettings, cfg: &EngineConfig, host: &str) -> Self {
        Self {
            verify_tls: settings.verify_tls,
            follow_redirects: settings.follow_redirects,
            proxy: cfg.proxy.clone(),
            jar: cfg.cookies.as_ref().map(|j| Arc::as_ptr(j) as usize),
            ca_bundle: cfg.tls.ca_bundle.as_deref().map(FileKey::of),
            identity: cfg.tls.cert_for(host).map(|c| FileKey::of(&c.path)),
            min_tls: cfg.tls.min_tls,
        }
    }
}

/// Bounded LRU of clients. Cheap to share: wrap it in an `Arc` and put it in
/// [`EngineConfig::client_cache`], or leave that `None` to use the process-wide cache.
#[derive(Debug)]
pub struct ClientCache {
    capacity: usize,
    /// Most recently used last. Linear scan: the capacity is tiny.
    entries: Mutex<Vec<(ClientKey, reqwest::Client)>>,
}

impl Default for ClientCache {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl ClientCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// A cache holding at most `capacity` clients (at least 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Number of clients currently cached.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(ClientKey, reqwest::Client)>> {
        // A panic while holding the lock cannot leave the Vec inconsistent; keep going.
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The cached client for these settings and request host (which picks the client
    /// certificate), building (and caching) one on a miss.
    pub(crate) fn get(
        &self,
        settings: &RequestSettings,
        cfg: &EngineConfig,
        host: &str,
    ) -> Result<reqwest::Client, HttpError> {
        let key = ClientKey::new(settings, cfg, host);
        let mut entries = self.lock();
        if let Some(i) = entries.iter().position(|(k, _)| *k == key) {
            let hit = entries.remove(i);
            let client = hit.1.clone();
            entries.push(hit);
            return Ok(client);
        }
        // Building is synchronous and fast relative to a request; holding the lock avoids two
        // concurrent misses building duplicate clients for the same key.
        let client = build_client(
            settings,
            &cfg.proxy,
            cfg.cookies.as_ref(),
            cfg.tls.ca_bundle.as_deref(),
            cfg.tls.cert_for(host).map(|c| c.path.as_path()),
            cfg.tls.min_tls,
        )?;
        if entries.len() >= self.capacity {
            entries.remove(0);
        }
        entries.push((key, client.clone()));
        Ok(client)
    }
}

fn build_client(
    settings: &RequestSettings,
    proxy: &str,
    cookies: Option<&Arc<CookieJar>>,
    ca_bundle: Option<&std::path::Path>,
    identity: Option<&std::path::Path>,
    min_tls: Option<TlsVersion>,
) -> Result<reqwest::Client, HttpError> {
    let mut cb = reqwest::Client::builder()
        .danger_accept_invalid_certs(!settings.verify_tls)
        .redirect(if settings.follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
    if let Some(jar) = cookies {
        // a Cookie header set on the request itself is left alone; the jar adds nothing then
        cb = cb.cookie_provider(jar.clone());
    }
    // Same meaning as before caching: empty adds no proxy (reqwest's defaults stay in effect).
    if !proxy.is_empty() {
        cb = cb.proxy(
            reqwest::Proxy::all(proxy).map_err(|e| HttpError::InvalidRequest(e.to_string()))?,
        );
    }
    let custom_tls = ca_bundle.is_some() || identity.is_some() || min_tls.is_some();
    cb = tls::apply(cb, ca_bundle, identity, min_tls)?;
    cb.build().map_err(|e| {
        if custom_tls {
            HttpError::Tls(format!(
                "cannot set up TLS with the configured certificates: {e}"
            ))
        } else {
            HttpError::Other(e.to_string())
        }
    })
}

/// The process-wide cache used when `EngineConfig::client_cache` is `None`.
pub(crate) fn shared() -> &'static ClientCache {
    static SHARED: std::sync::OnceLock<ClientCache> = std::sync::OnceLock::new();
    SHARED.get_or_init(ClientCache::default)
}

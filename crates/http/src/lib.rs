//! HTTP engine: native client (no CORS, no proxy route), cancellation, timeouts,
//! structured errors, streaming with disk spill for large bodies (FR-ENG-*).
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use at_core::{Built, BuiltBody, BuiltPart, RequestSettings};
use bytes::Bytes;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("DNS lookup failed: {0}")]
    Dns(String),
    #[error("connection refused: {0}")]
    ConnectionRefused(String),
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("request timed out")]
    Timeout,
    #[error("request cancelled")]
    Cancelled,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub enum ResponseBody {
    Memory(Bytes),
    /// Bodies above the spill threshold live on disk, never fully in RAM.
    File {
        path: PathBuf,
        len: u64,
    },
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: ResponseBody,
    /// Size in bytes of the (decompressed) body.
    pub size: u64,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Bodies larger than this are written to `spill_dir` (proposed 10 MB).
    pub spill_threshold: u64,
    pub spill_dir: PathBuf,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            spill_threshold: 10 * 1024 * 1024,
            spill_dir: std::env::temp_dir().join("api-tester-spill"),
        }
    }
}

static SPILL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Send a built request. Dropping/cancelling aborts the in-flight request immediately.
pub async fn send(
    built: &Built,
    settings: &RequestSettings,
    cfg: &EngineConfig,
    cancel: &CancellationToken,
) -> Result<HttpResponse, HttpError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(HttpError::Cancelled),
        r = run(built, settings, cfg) => r,
    }
}

async fn run(
    built: &Built,
    settings: &RequestSettings,
    cfg: &EngineConfig,
) -> Result<HttpResponse, HttpError> {
    let start = Instant::now();

    // NOTE: client is built per request for now (per-request TLS/redirect/timeout
    // settings). Phase 2 will cache clients keyed by settings to reuse connections.
    let mut cb = reqwest::Client::builder()
        .danger_accept_invalid_certs(!settings.verify_tls)
        .redirect(if settings.follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
    if settings.timeout_ms > 0 {
        cb = cb.timeout(Duration::from_millis(settings.timeout_ms));
    }
    let client = cb.build().map_err(|e| HttpError::Other(e.to_string()))?;

    let method = reqwest::Method::from_bytes(built.method.as_str().as_bytes())
        .map_err(|e| HttpError::InvalidRequest(e.to_string()))?;
    let mut rb = client.request(method, &built.url);
    for (k, v) in &built.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    match &built.body {
        BuiltBody::None => {}
        BuiltBody::Bytes(data) => rb = rb.body(data.clone()),
        BuiltBody::Multipart(parts) => {
            let mut form = reqwest::multipart::Form::new();
            for p in parts {
                match p {
                    BuiltPart::Text { name, value } => {
                        form = form.text(name.clone(), value.clone())
                    }
                    BuiltPart::File { name, path } => {
                        let data = tokio::fs::read(path).await?;
                        let fname = std::path::Path::new(path)
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "file".into());
                        form = form.part(
                            name.clone(),
                            reqwest::multipart::Part::bytes(data).file_name(fname),
                        );
                    }
                }
            }
            rb = rb.multipart(form);
        }
    }

    let mut resp = rb.send().await.map_err(classify)?;
    let status = resp.status();
    let version = format!("{:?}", resp.version());
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();

    let mut mem: Vec<u8> = Vec::new();
    let mut file: Option<(PathBuf, tokio::fs::File)> = None;
    let mut size: u64 = 0;
    while let Some(chunk) = resp.chunk().await.map_err(classify)? {
        size += chunk.len() as u64;
        if let Some((_, f)) = file.as_mut() {
            f.write_all(&chunk).await?;
        } else {
            mem.extend_from_slice(&chunk);
            if size > cfg.spill_threshold {
                tokio::fs::create_dir_all(&cfg.spill_dir).await?;
                let path = cfg.spill_dir.join(format!(
                    "resp-{}-{}.bin",
                    std::process::id(),
                    SPILL_COUNTER.fetch_add(1, Ordering::Relaxed)
                ));
                let mut f = tokio::fs::File::create(&path).await?;
                f.write_all(&mem).await?;
                mem = Vec::new();
                file = Some((path, f));
            }
        }
    }

    let body = match file {
        Some((path, mut f)) => {
            f.flush().await?;
            ResponseBody::File { path, len: size }
        }
        None => ResponseBody::Memory(Bytes::from(mem)),
    };

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        version,
        headers,
        body,
        size,
        elapsed: start.elapsed(),
    })
}

fn classify(e: reqwest::Error) -> HttpError {
    if e.is_timeout() {
        return HttpError::Timeout;
    }
    if e.is_builder() {
        return HttpError::InvalidRequest(e.to_string());
    }
    // Walk the error chain; reqwest hides the useful text in `source()`.
    let mut text = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        text.push_str(": ");
        text.push_str(&s.to_string());
        src = s.source();
    }
    let l = text.to_lowercase();
    if l.contains("dns error")
        || l.contains("failed to lookup address")
        || l.contains("name or service not known")
        || l.contains("nodename nor servname")
        || l.contains("no such host")
    {
        HttpError::Dns(text)
    } else if l.contains("connection refused") || l.contains("actively refused") {
        HttpError::ConnectionRefused(text)
    } else if l.contains("certificate") || l.contains("tls") || l.contains("ssl") {
        HttpError::Tls(text)
    } else if e.is_connect() {
        HttpError::Connect(text)
    } else {
        HttpError::Other(text)
    }
}

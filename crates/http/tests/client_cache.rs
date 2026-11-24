//! G-10: clients are cached per client-level settings, so repeat sends reuse connections.
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use at_core::{build, Request, RequestSettings, Variables};
use at_http::{send, ClientCache, CookieJar, EngineConfig, HttpError, ResponseBody};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Keep-alive HTTP/1.1 server that counts accepted TCP connections.
/// `GET /slow/<ms>` waits that long before answering; everything else answers at once.
async fn keep_alive_server() -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    // one request head per loop; these tests send no request bodies
                    let end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                        match sock.read(&mut tmp).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    buf.drain(..end);
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    if let Some(ms) = path.strip_prefix("/slow/") {
                        let ms: u64 = ms.parse().unwrap_or(0);
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                    }
                    let body = format!("ok {path}");
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{body}",
                        body.len()
                    );
                    if sock.write_all(resp.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (addr, accepted)
}

fn get(addr: SocketAddr, path: &str) -> at_core::Built {
    let r = Request {
        url: format!("http://{addr}{path}"),
        ..Default::default()
    };
    build(&r, &Variables::default()).unwrap()
}

fn isolated(cache: &Arc<ClientCache>) -> EngineConfig {
    EngineConfig {
        client_cache: Some(cache.clone()),
        ..Default::default()
    }
}

fn text(resp: at_http::HttpResponse) -> String {
    match resp.body {
        ResponseBody::Memory(b) => String::from_utf8_lossy(&b).to_string(),
        _ => panic!("expected memory body"),
    }
}

#[tokio::test]
async fn same_settings_reuse_one_connection() {
    let (addr, accepted) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let cfg = isolated(&cache);
    let s = RequestSettings::default();
    let t = CancellationToken::new();

    let a = send(&get(addr, "/a"), &s, &cfg, &t).await.unwrap();
    assert_eq!(text(a), "ok /a");
    let b = send(&get(addr, "/b"), &s, &cfg, &t).await.unwrap();
    assert_eq!(text(b), "ok /b");

    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "second send reused the pooled connection"
    );
    assert_eq!(cache.len(), 1);
}

#[tokio::test]
async fn shared_cache_is_used_by_default_configs() {
    // two separately built default configs (as the app does per Send) still share a client
    let (addr, accepted) = keep_alive_server().await;
    let s = RequestSettings::default();
    let t = CancellationToken::new();
    send(&get(addr, "/1"), &s, &EngineConfig::default(), &t)
        .await
        .unwrap();
    send(&get(addr, "/2"), &s, &EngineConfig::default(), &t)
        .await
        .unwrap();
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn timeout_does_not_split_the_cache() {
    let (addr, accepted) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let cfg = isolated(&cache);
    let t = CancellationToken::new();
    for ms in [1_000, 5_000, 0] {
        let s = RequestSettings {
            timeout_ms: ms,
            ..Default::default()
        };
        send(&get(addr, "/x"), &s, &cfg, &t).await.unwrap();
    }
    assert_eq!(cache.len(), 1);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tls_and_redirect_settings_get_different_clients() {
    let (addr, accepted) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let cfg = isolated(&cache);
    let t = CancellationToken::new();
    let variants = [(true, true), (false, true), (true, false), (false, false)];
    for (verify_tls, follow_redirects) in variants {
        let s = RequestSettings {
            verify_tls,
            follow_redirects,
            ..Default::default()
        };
        send(&get(addr, "/v"), &s, &cfg, &t).await.unwrap();
    }
    assert_eq!(
        cache.len(),
        4,
        "one client per (verify_tls, follow_redirects)"
    );
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        4,
        "different clients do not share a pool"
    );
    // repeating a combination is a hit
    let s = RequestSettings {
        verify_tls: false,
        follow_redirects: true,
        ..Default::default()
    };
    send(&get(addr, "/v"), &s, &cfg, &t).await.unwrap();
    assert_eq!(cache.len(), 4);
    assert_eq!(accepted.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn proxy_gets_its_own_client_and_is_used() {
    // The keep-alive server doubles as the "proxy": a proxied plain-http request arrives as
    // `GET http://target/path`, so the echoed path proves the request went through it.
    let (proxy_addr, proxy_accepted) = keep_alive_server().await;
    let (target, target_accepted) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let s = RequestSettings::default();
    let t = CancellationToken::new();

    let direct = send(&get(target, "/p"), &s, &isolated(&cache), &t)
        .await
        .unwrap();
    assert_eq!(text(direct), "ok /p");

    let proxied_cfg = EngineConfig {
        proxy: format!("http://{proxy_addr}"),
        client_cache: Some(cache.clone()),
        ..Default::default()
    };
    let proxied = send(&get(target, "/p"), &s, &proxied_cfg, &t)
        .await
        .unwrap();
    assert_eq!(text(proxied), format!("ok http://{target}/p"));

    assert_eq!(cache.len(), 2, "proxy is part of the key");
    assert_eq!(target_accepted.load(Ordering::SeqCst), 1);
    assert_eq!(proxy_accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn invalid_proxy_is_an_invalid_request() {
    let cache = Arc::new(ClientCache::new());
    let cfg = EngineConfig {
        proxy: "not a proxy url".into(),
        client_cache: Some(cache.clone()),
        ..Default::default()
    };
    let r = send(
        &get("127.0.0.1:9".parse().unwrap(), "/"),
        &RequestSettings::default(),
        &cfg,
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(r, Err(HttpError::InvalidRequest(_))), "{r:?}");
    assert!(cache.is_empty(), "a failed build is not cached");
}

#[tokio::test]
async fn each_cookie_jar_gets_its_own_client() {
    let (addr, _) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let s = RequestSettings::default();
    let t = CancellationToken::new();
    let jar_a = Arc::new(CookieJar::new());
    let jar_b = Arc::new(CookieJar::new());
    for jar in [&jar_a, &jar_b, &jar_a] {
        let cfg = EngineConfig {
            cookies: Some(jar.clone()),
            client_cache: Some(cache.clone()),
            ..Default::default()
        };
        send(&get(addr, "/c"), &s, &cfg, &t).await.unwrap();
    }
    assert_eq!(cache.len(), 2, "keyed on jar identity");
}

#[tokio::test]
async fn cache_is_bounded() {
    let (addr, _) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::with_capacity(4));
    let s = RequestSettings::default();
    let t = CancellationToken::new();
    for _ in 0..20 {
        let cfg = EngineConfig {
            cookies: Some(Arc::new(CookieJar::new())),
            client_cache: Some(cache.clone()),
            ..Default::default()
        };
        send(&get(addr, "/b"), &s, &cfg, &t).await.unwrap();
    }
    assert_eq!(cache.len(), 4);
}

#[tokio::test]
async fn per_request_timeout_applies_to_a_cached_client() {
    let (addr, accepted) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let cfg = isolated(&cache);
    let t = CancellationToken::new();

    // warm the cache with a short-timeout request
    let short = RequestSettings {
        timeout_ms: 300,
        ..Default::default()
    };
    send(&get(addr, "/warm"), &short, &cfg, &t).await.unwrap();
    assert_eq!(cache.len(), 1);

    // the same cached client honours a longer timeout: 600 ms of server delay is fine
    let long = RequestSettings {
        timeout_ms: 5_000,
        ..Default::default()
    };
    let ok = send(&get(addr, "/slow/600"), &long, &cfg, &t).await;
    assert!(ok.is_ok(), "{ok:?}");

    // ...and a short one still times out
    let started = std::time::Instant::now();
    let r = send(&get(addr, "/slow/3000"), &short, &cfg, &t).await;
    assert!(matches!(r, Err(HttpError::Timeout)), "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(2));

    assert_eq!(cache.len(), 1, "all three used the one cached client");
    assert!(accepted.load(Ordering::SeqCst) <= 2);
}

#[tokio::test]
async fn cancel_still_works_with_a_cached_client() {
    let (addr, _) = keep_alive_server().await;
    let cache = Arc::new(ClientCache::new());
    let cfg = isolated(&cache);
    let s = RequestSettings::default();
    send(&get(addr, "/warm"), &s, &cfg, &CancellationToken::new())
        .await
        .unwrap();

    let t = CancellationToken::new();
    let t2 = t.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        t2.cancel();
    });
    let r = send(&get(addr, "/slow/5000"), &s, &cfg, &t).await;
    assert!(matches!(r, Err(HttpError::Cancelled)), "{r:?}");

    // the cache stays usable after an aborted request
    let again = send(&get(addr, "/after"), &s, &cfg, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(text(again), "ok /after");
}

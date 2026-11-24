//! G-10: the second request to the same host, with and without the client cache.
//!
//! `cached` is what every Send after the first does now: the engine finds its `reqwest::Client`
//! in the cache and reuses the pooled keep-alive connection. `fresh` builds a new cache per
//! request, which is what the engine did before G-10 (new client, new connection, every time).
//! Both talk to a keep-alive HTTP/1.1 server on 127.0.0.1, so the gap is client setup plus the
//! TCP handshake; against a real HTTPS host it also includes DNS and the TLS handshake.

use std::net::SocketAddr;
use std::sync::Arc;

use at_core::{build, Built, Request, RequestSettings, Variables};
use at_http::{send, ClientCache, EngineConfig};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

/// Keep-alive server answering every request head with a tiny fixed body.
async fn keep_alive_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                        match sock.read(&mut tmp).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    };
                    buf.drain(..end);
                    let resp: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}";
                    if sock.write_all(resp).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

fn get(addr: SocketAddr) -> Built {
    let r = Request {
        url: format!("http://{addr}/v1/ping"),
        ..Default::default()
    };
    build(&r, &Variables::default()).unwrap()
}

fn send_same_host(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let addr = rt.block_on(keep_alive_server());
    let built = get(addr);
    let settings = RequestSettings::default();
    let token = CancellationToken::new();

    let warm = EngineConfig {
        client_cache: Some(Arc::new(ClientCache::new())),
        ..Default::default()
    };
    // the first request builds the client and opens the connection
    rt.block_on(send(&built, &settings, &warm, &token)).unwrap();

    let mut g = c.benchmark_group("send_same_host");
    g.bench_function("second_request_cached_client", |b| {
        b.iter(|| {
            rt.block_on(send(black_box(&built), &settings, &warm, &token))
                .unwrap()
        })
    });
    g.bench_function("second_request_fresh_client", |b| {
        b.iter(|| {
            let fresh = EngineConfig {
                client_cache: Some(Arc::new(ClientCache::new())),
                ..Default::default()
            };
            rt.block_on(send(black_box(&built), &settings, &fresh, &token))
                .unwrap()
        })
    });
    g.finish();
}

criterion_group!(benches, send_same_host);
criterion_main!(benches);

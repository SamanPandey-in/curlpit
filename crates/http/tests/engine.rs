use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use at_core::{build, Auth, KeyValue, Request, RequestSettings, Variables};
use at_http::{send, EngineConfig, HttpError, ResponseBody};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Minimal HTTP/1.1 server. `f` gets the raw request head; `None` = never answer.
async fn serve<F>(f: F) -> SocketAddr
where
    F: Fn(String) -> Option<Vec<u8>> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let f = Arc::new(f);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => break,
            };
            let f = f.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = match sock.read(&mut tmp).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf).to_string();
                match f(req) {
                    Some(body) => {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = sock.write_all(head.as_bytes()).await;
                        let _ = sock.write_all(&body).await;
                        let _ = sock.shutdown().await;
                    }
                    None => tokio::time::sleep(Duration::from_secs(30)).await,
                }
            });
        }
    });
    addr
}

fn req(url: String) -> Request {
    Request {
        url,
        ..Default::default()
    }
}

#[tokio::test]
async fn get_with_params_and_basic_auth() {
    let addr = serve(|raw| Some(raw.into_bytes())).await;
    let mut r = req(format!("http://{addr}/hello"));
    r.params.push(KeyValue::new("q", "a b"));
    r.auth = Auth::Basic {
        username: "user".into(),
        password: "pass".into(),
    };
    let built = build(&r, &Variables::default()).unwrap();
    let resp = send(
        &built,
        &RequestSettings::default(),
        &EngineConfig::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.status_text, "OK");
    let ResponseBody::Memory(b) = resp.body else {
        panic!("expected in-memory body")
    };
    let text = String::from_utf8_lossy(&b).to_lowercase();
    assert!(text.starts_with("get /hello?q=a%20b "), "got: {text}");
    assert!(
        text.contains("authorization: basic dxnlcjpwyxnz"),
        "got: {text}"
    );
}

#[tokio::test]
async fn cancel_in_flight() {
    let addr = serve(|_| None).await;
    let built = build(&req(format!("http://{addr}/")), &Variables::default()).unwrap();
    let token = CancellationToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        t2.cancel();
    });
    let started = std::time::Instant::now();
    let err = send(
        &built,
        &RequestSettings::default(),
        &EngineConfig::default(),
        &token,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, HttpError::Cancelled), "got {err:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn timeout_is_reported() {
    let addr = serve(|_| None).await;
    let built = build(&req(format!("http://{addr}/")), &Variables::default()).unwrap();
    let settings = RequestSettings {
        timeout_ms: 200,
        ..Default::default()
    };
    let err = send(
        &built,
        &settings,
        &EngineConfig::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, HttpError::Timeout), "got {err:?}");
}

#[tokio::test]
async fn connection_refused_is_structured() {
    // Bind then drop to get a port nothing listens on.
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    let built = build(&req(format!("http://{addr}/")), &Variables::default()).unwrap();
    let err = send(
        &built,
        &RequestSettings::default(),
        &EngineConfig::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, HttpError::ConnectionRefused(_) | HttpError::Connect(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn large_body_spills_to_disk() {
    let addr = serve(|_| Some(vec![b'x'; 1000])).await;
    let built = build(&req(format!("http://{addr}/")), &Variables::default()).unwrap();
    let dir = std::env::temp_dir().join(format!("at-spill-test-{}", std::process::id()));
    let cfg = EngineConfig {
        spill_threshold: 100,
        spill_dir: dir.clone(),
    };
    let resp = send(
        &built,
        &RequestSettings::default(),
        &cfg,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(resp.size, 1000);
    match resp.body {
        ResponseBody::File { path, len } => {
            assert_eq!(len, 1000);
            assert_eq!(std::fs::metadata(&path).unwrap().len(), 1000);
        }
        other => panic!("expected spill, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

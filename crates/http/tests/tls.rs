//! G-17: CA bundle, client certificates and minimum TLS version, against a local rustls
//! server whose certificates are generated here. Certificate verification stays on.
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use at_core::{build, Request, RequestSettings, Variables};
use at_http::{
    send, ClientCache, ClientCert, EngineConfig, HttpError, ResponseBody, TlsConfig, TlsVersion,
};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

/// A private CA, a server certificate for 127.0.0.1 and a client certificate, all signed by it.
struct Pki {
    ca_pem: String,
    server_der: CertificateDer<'static>,
    server_key: Vec<u8>,
    /// Client certificate followed by its private key, as one PEM file.
    client_pem: String,
    ca_der: CertificateDer<'static>,
}

fn pki() -> Pki {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.distinguished_name
        .push(DnType::CommonName, "API Tester test CA");
    let ca_cert = ca.self_signed(&ca_key).unwrap();

    let server_key = KeyPair::generate().unwrap();
    let server = CertificateParams::new(vec!["127.0.0.1".to_string()])
        .unwrap()
        .signed_by(&server_key, &ca_cert, &ca_key)
        .unwrap();

    let client_key = KeyPair::generate().unwrap();
    let mut client_params = CertificateParams::new(vec!["client.test".to_string()]).unwrap();
    client_params
        .distinguished_name
        .push(DnType::CommonName, "test client");
    let client = client_params
        .signed_by(&client_key, &ca_cert, &ca_key)
        .unwrap();

    Pki {
        ca_pem: ca_cert.pem(),
        ca_der: ca_cert.der().clone(),
        server_der: server.der().clone(),
        server_key: server_key.serialize_der(),
        client_pem: format!("{}{}", client.pem(), client_key.serialize_pem()),
    }
}

struct ServerOpts {
    require_client_cert: bool,
    tls12_only: bool,
}

/// HTTPS server answering every request with `200 hello`. Failed handshakes are dropped.
async fn serve(p: &Pki, opts: ServerOpts) -> SocketAddr {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let versions: &[&rustls::SupportedProtocolVersion] = if opts.tls12_only {
        &[&rustls::version::TLS12]
    } else {
        rustls::DEFAULT_VERSIONS
    };
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(versions)
        .unwrap();
    let builder = if opts.require_client_cert {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(p.ca_der.clone()).unwrap();
        let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
            .build()
            .unwrap();
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(p.server_key.clone()));
    let config = builder
        .with_single_cert(vec![p.server_der.clone()], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(sock).await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut tmp).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let _ = tls
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .await;
                let _ = tls.shutdown().await;
            });
        }
    });
    addr
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

/// Its own client cache, so nothing is shared with other tests.
fn cfg(tls: TlsConfig) -> EngineConfig {
    EngineConfig {
        client_cache: Some(Arc::new(ClientCache::new())),
        tls,
        ..Default::default()
    }
}

async fn get(addr: SocketAddr, cfg: &EngineConfig) -> Result<String, HttpError> {
    let r = Request {
        url: format!("https://{addr}/"),
        ..Default::default()
    };
    let built = build(&r, &Variables::default()).unwrap();
    let settings = RequestSettings {
        verify_tls: true,
        ..Default::default()
    };
    let resp = send(&built, &settings, cfg, &CancellationToken::new()).await?;
    assert_eq!(resp.status, 200);
    match resp.body {
        ResponseBody::Memory(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        _ => panic!("expected an in-memory body"),
    }
}

fn tls_error(r: Result<String, HttpError>) -> String {
    match r {
        Err(HttpError::Tls(m)) => m,
        other => panic!("expected a TLS error, got {other:?}"),
    }
}

#[tokio::test]
async fn ca_bundle_makes_a_private_ca_trusted_with_verification_on() {
    let p = pki();
    let dir = tempfile::tempdir().unwrap();
    let ca = write(dir.path(), "ca.pem", &p.ca_pem);
    let addr = serve(
        &p,
        ServerOpts {
            require_client_cert: false,
            tls12_only: false,
        },
    )
    .await;

    // without the bundle the private CA is unknown
    tls_error(get(addr, &cfg(TlsConfig::default())).await);

    let with_ca = cfg(TlsConfig {
        ca_bundle: Some(ca),
        ..Default::default()
    });
    assert_eq!(get(addr, &with_ca).await.unwrap(), "hello");
}

#[tokio::test]
async fn client_certificate_is_presented_to_its_host_only() {
    let p = pki();
    let dir = tempfile::tempdir().unwrap();
    let ca = write(dir.path(), "ca.pem", &p.ca_pem);
    let cert = write(dir.path(), "client.pem", &p.client_pem);
    let addr = serve(
        &p,
        ServerOpts {
            require_client_cert: true,
            tls12_only: false,
        },
    )
    .await;

    let with = |host: &str| {
        cfg(TlsConfig {
            ca_bundle: Some(ca.clone()),
            client_certs: vec![ClientCert {
                host: host.into(),
                path: cert.clone(),
            }],
            ..Default::default()
        })
    };
    assert_eq!(get(addr, &with("127.0.0.1")).await.unwrap(), "hello");
    // configured for another host: nothing is presented and the server refuses
    assert!(get(addr, &with("api.example.com")).await.is_err());
    // and none at all
    assert!(get(
        addr,
        &cfg(TlsConfig {
            ca_bundle: Some(ca.clone()),
            ..Default::default()
        })
    )
    .await
    .is_err());
}

#[tokio::test]
async fn bad_paths_and_files_name_the_file() {
    let p = pki();
    let dir = tempfile::tempdir().unwrap();
    let addr = serve(
        &p,
        ServerOpts {
            require_client_cert: false,
            tls12_only: false,
        },
    )
    .await;

    let missing = dir.path().join("nope.pem");
    let m = tls_error(
        get(
            addr,
            &cfg(TlsConfig {
                ca_bundle: Some(missing.clone()),
                ..Default::default()
            }),
        )
        .await,
    );
    assert!(m.contains("CA bundle") && m.contains("nope.pem"), "{m}");

    let junk = write(dir.path(), "junk.pem", "not a certificate");
    let m = tls_error(
        get(
            addr,
            &cfg(TlsConfig {
                ca_bundle: Some(junk),
                ..Default::default()
            }),
        )
        .await,
    );
    assert!(m.contains("junk.pem"), "{m}");

    let cert_cfg = |path: PathBuf| {
        cfg(TlsConfig {
            client_certs: vec![ClientCert {
                host: "127.0.0.1".into(),
                path,
            }],
            ..Default::default()
        })
    };
    let m = tls_error(get(addr, &cert_cfg(dir.path().join("gone.pem"))).await);
    assert!(
        m.contains("client certificate") && m.contains("gone.pem"),
        "{m}"
    );

    // a certificate without its key
    let only_cert = write(dir.path(), "only-cert.pem", &p.ca_pem);
    let m = tls_error(get(addr, &cert_cfg(only_cert)).await);
    assert!(m.contains("only-cert.pem"), "{m}");

    let p12 = write(dir.path(), "client.p12", "binary");
    let m = tls_error(get(addr, &cert_cfg(p12)).await);
    assert!(m.contains("PKCS#12") && m.contains("client.p12"), "{m}");

    let enc = write(
        dir.path(),
        "enc.pem",
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n",
    );
    let m = tls_error(get(addr, &cert_cfg(enc)).await);
    assert!(m.contains("encrypted") && m.contains("enc.pem"), "{m}");
}

#[tokio::test]
async fn minimum_tls_version_is_enforced() {
    let p = pki();
    let dir = tempfile::tempdir().unwrap();
    let ca = write(dir.path(), "ca.pem", &p.ca_pem);
    let addr = serve(
        &p,
        ServerOpts {
            require_client_cert: false,
            tls12_only: true,
        },
    )
    .await;
    let with = |min: Option<TlsVersion>| {
        cfg(TlsConfig {
            ca_bundle: Some(ca.clone()),
            min_tls: min,
            ..Default::default()
        })
    };
    assert_eq!(get(addr, &with(None)).await.unwrap(), "hello");
    assert_eq!(
        get(addr, &with(Some(TlsVersion::Tls12))).await.unwrap(),
        "hello"
    );
    tls_error(get(addr, &with(Some(TlsVersion::Tls13))).await);
}

#[tokio::test]
async fn tls_options_split_the_client_cache() {
    let p = pki();
    let dir = tempfile::tempdir().unwrap();
    let ca = write(dir.path(), "ca.pem", &p.ca_pem);
    let addr = serve(
        &p,
        ServerOpts {
            require_client_cert: false,
            tls12_only: false,
        },
    )
    .await;
    let cache = Arc::new(ClientCache::new());
    let mk = |min: Option<TlsVersion>| EngineConfig {
        client_cache: Some(cache.clone()),
        tls: TlsConfig {
            ca_bundle: Some(ca.clone()),
            min_tls: min,
            ..Default::default()
        },
        ..Default::default()
    };
    get(addr, &mk(None)).await.unwrap();
    get(addr, &mk(None)).await.unwrap();
    assert_eq!(cache.len(), 1);
    get(addr, &mk(Some(TlsVersion::Tls13))).await.unwrap();
    assert_eq!(cache.len(), 2);
}

#[test]
fn engine_default_starts_from_the_process_tls_default() {
    // Only this test touches the process default; every other test sets `tls` itself.
    let tls = TlsConfig {
        min_tls: Some(TlsVersion::Tls13),
        ..Default::default()
    };
    at_http::set_default_tls(tls.clone());
    assert_eq!(EngineConfig::default().tls, tls);
    at_http::set_default_tls(TlsConfig::default());
    assert_eq!(EngineConfig::default().tls, TlsConfig::default());
}

//! End to end: the real binary against a local HTTP server, over a temp collection of two
//! requests and a two-row CSV data file.

use at_core::{EnvVar, Environment, Request};
use at_store::Store;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BIN: &str = env!("CARGO_BIN_EXE_api-tester-cli");

/// `/status/N` answers with status N; anything else answers 200 with its own path as body.
async fn server() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let path = head.split(' ').nth(1).unwrap_or("/").to_string();
                let code: u16 = path
                    .strip_prefix("/status/")
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(200);
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{path}",
                    path.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    collection: PathBuf,
    data_dir: PathBuf,
}

fn fixture(base: &str) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let data_dir = root.join("data");
    let store = Store::open_at(&data_dir).unwrap();
    let c = store.create_collection("E2E API").unwrap();
    for req in [
        Request {
            name: "A user".into(),
            url: "{{base}}/users/{{user}}".into(),
            test_script: r#"
                pm.test("echoes the data row", () =>
                    pm.expect(pm.response.text()).to.equal("/users/" + pm.iterationData.get("user")));
            "#
            .into(),
            ..Default::default()
        },
        Request {
            name: "B status".into(),
            url: "{{base}}/status/{{code}}".into(),
            test_script: r#"
                pm.test("status <expected>", () =>
                    pm.response.to.have.status(Number(pm.iterationData.get("expect"))));
            "#
            .into(),
            ..Default::default()
        },
    ] {
        Store::save_request(&c.dir, &req, None).unwrap();
    }
    store
        .save_environment(
            &Environment {
                name: "Local".into(),
                variables: vec![EnvVar::new("base", base), EnvVar::new("user", "env-user")],
            },
            None,
        )
        .unwrap();
    std::fs::write(
        root.join("env.toml"),
        format!("name = \"File env\"\n[[variables]]\nkey = \"base\"\nvalue = \"{base}\"\n"),
    )
    .unwrap();
    Fixture {
        _tmp: tmp,
        root,
        collection: c.dir,
        data_dir,
    }
}

async fn cli(args: Vec<String>, cwd: &Path) -> Output {
    let cwd = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || {
        Command::new(BIN)
            .args(args)
            .current_dir(cwd)
            .env_remove("API_TESTER_DATA_DIR")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runs_a_collection_with_a_data_file_and_reports() {
    let base = server().await;
    let f = fixture(&base);
    let data = f.root.join("rows.csv");
    std::fs::write(
        &data,
        "user,code,expect\r\nada,200,200\r\n\"b,ob\",404,404\r\n",
    )
    .unwrap();
    let junit = f.root.join("out").join("junit.xml");

    // passing run: --env from the data dir, CSV rows override the environment's `user`
    let out = cli(
        vec![
            "run".into(),
            s(&f.collection),
            "--data-dir".into(),
            s(&f.data_dir),
            "--env".into(),
            "Local".into(),
            "--data".into(),
            s(&data),
            "-r".into(),
            "cli".into(),
            "-r".into(),
            "junit".into(),
            "-r".into(),
            "json".into(),
            "--out-dir".into(),
            s(&f.root.join("out")),
        ],
        &f.root,
    )
    .await;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("ok   #2 GET B status -> 404"), "{stdout}");
    assert!(stdout.contains("Run \"E2E API\": 4 request(s), 0 failed; tests 4 passed, 0 failed"));
    assert!(!stdout.contains('\x1b'), "NO_COLOR is honoured");

    let xml = std::fs::read_to_string(&junit).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let suite = doc.root_element().first_element_child().unwrap();
    assert_eq!(suite.attribute("name"), Some("E2E API"));
    assert_eq!(suite.attribute("tests"), Some("4"));
    assert_eq!(suite.attribute("failures"), Some("0"));
    let names: Vec<_> = suite
        .children()
        .filter(|n| n.has_tag_name("testcase"))
        .map(|n| n.attribute("name").unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "GET A user (iteration 1)",
            "GET B status (iteration 1)",
            "GET A user (iteration 2)",
            "GET B status (iteration 2)"
        ]
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(f.root.join("out/report.json")).unwrap())
            .unwrap();
    assert_eq!(json["summary"]["passed"], true);
    assert_eq!(json["rows"][2]["status"], 200);

    // failing run: --env-file, the second row expects the wrong status; JUnit written to --out
    let bad = f.root.join("bad.csv");
    std::fs::write(&bad, "user,code,expect\nada,200,200\nbob,500,200\n").unwrap();
    let junit = f.root.join("fail.xml");
    let out = cli(
        vec![
            "run".into(),
            s(&f.collection),
            "--data-dir".into(),
            s(&f.data_dir),
            "--env-file".into(),
            s(&f.root.join("env.toml")),
            "--data".into(),
            s(&bad),
            "-r".into(),
            "junit".into(),
            "--out".into(),
            s(&junit),
        ],
        &f.root,
    )
    .await;
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "no cli reporter, no stdout");
    let xml = std::fs::read_to_string(&junit).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(doc.root_element().attribute("failures"), Some("1"));
    let failure = doc
        .descendants()
        .find(|n| n.has_tag_name("failure"))
        .unwrap();
    assert!(failure
        .text()
        .unwrap()
        .contains("status <expected>: expected response to have status 200 but got 500"));

    // --bail stops at the first failure; a single file reporter with no --out goes to stdout
    let out = cli(
        vec![
            "run".into(),
            s(&f.collection),
            "--data-dir".into(),
            s(&f.data_dir),
            "--env-file".into(),
            s(&f.root.join("env.toml")),
            "--data".into(),
            s(&bad),
            "--iterations".into(),
            "5".into(),
            "--bail".into(),
            "-r".into(),
            "json".into(),
        ],
        &f.root,
    )
    .await;
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["summary"]["requests"], 4, "stopped at row 4 of 10");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_and_config_errors_exit_2() {
    let f = fixture("http://127.0.0.1:9");
    let cases: Vec<Vec<String>> = vec![
        vec![
            "run".into(),
            s(&f.collection),
            "--iterations".into(),
            "0".into(),
        ],
        vec!["run".into(), s(&f.root.join("nope"))],
        vec![
            "run".into(),
            s(&f.collection),
            "--data-dir".into(),
            s(&f.data_dir),
            "--env".into(),
            "Missing".into(),
        ],
        vec![
            "run".into(),
            s(&f.collection),
            "--data".into(),
            s(&f.root.join("missing.csv")),
        ],
        vec![
            "run".into(),
            s(&f.collection),
            "--out".into(),
            "x.json".into(),
        ],
        vec!["bogus".into()],
    ];
    for args in cases {
        let out = cli(args.clone(), &f.root).await;
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let bad_json = f.root.join("d.json");
    std::fs::write(&bad_json, "{\"user\": 1}").unwrap();
    let out = cli(
        vec![
            "run".into(),
            s(&f.collection),
            "--data".into(),
            s(&bad_json),
        ],
        &f.root,
    )
    .await;
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("array of objects"));

    // a collection of the data dir can be named instead of given as a path
    let out = cli(
        vec![
            "run".into(),
            "e2e api".into(),
            "--data-dir".into(),
            s(&f.data_dir),
            "--env".into(),
            "local".into(),
            "--timeout-ms".into(),
            "2000".into(),
        ],
        &f.root,
    )
    .await;
    // nothing listens on port 9: requests fail, which is exit 1, not a config error
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

//! Tiny test harness: `cargo run -p core-demo -- request.json [--var k=v]... [--env k=v]...`
use at_core::{build, Request, Variables};
use at_http::{send, EngineConfig, ResponseBody};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: core-demo <request.json> [--var k=v]... [--env k=v]...");
        std::process::exit(2);
    };
    let mut vars = Variables::default();
    while let Some(flag) = args.next() {
        let Some(kv) = args.next() else { break };
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        match flag.as_str() {
            "--var" => vars.global.insert(k.into(), v.into()),
            "--env" => vars.environment.insert(k.into(), v.into()),
            _ => None,
        };
    }

    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {path}: {e}");
        std::process::exit(2);
    });
    let req: Request = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("invalid request JSON: {e}");
        std::process::exit(2);
    });
    let built = build(&req, &vars).unwrap_or_else(|e| {
        eprintln!("build error: {e}");
        std::process::exit(1);
    });
    println!("> {} {}", built.method.as_str(), built.url);
    for (k, v) in &built.headers {
        println!("> {k}: {v}");
    }
    if !built.unresolved.is_empty() {
        println!("! unresolved variables: {:?}", built.unresolved);
    }

    let token = CancellationToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        t2.cancel();
    });

    match send(&built, &req.settings, &EngineConfig::default(), &token).await {
        Ok(resp) => {
            println!(
                "\n< {} {} ({}) in {:?}, {} bytes",
                resp.status, resp.status_text, resp.version, resp.elapsed, resp.size
            );
            for (k, v) in &resp.headers {
                println!("< {k}: {v}");
            }
            match resp.body {
                ResponseBody::Memory(b) => {
                    let s = String::from_utf8_lossy(&b);
                    let shown: String = s.chars().take(2000).collect();
                    println!("\n{shown}");
                    if s.chars().count() > 2000 {
                        println!("... (truncated)");
                    }
                }
                ResponseBody::File { path, len } => {
                    println!("\n[body spilled to {} ({len} bytes)]", path.display())
                }
            }
        }
        Err(e) => {
            eprintln!("\nERROR: {e}");
            std::process::exit(1);
        }
    }
}

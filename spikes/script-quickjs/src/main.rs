//! Spike B: QuickJS as the script engine. Checks startup cost, speed, and sandbox containment
//! (infinite loop, memory bomb, no require/process/fetch/fs).
use rquickjs::{Context, Runtime};
use std::time::{Duration, Instant};

fn rss_mb() -> f64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .map(|kb| kb / 1024.0)
        .unwrap_or(-1.0)
}

/// Fresh runtime with a time limit (5 s in the real product; shorter here) and a memory limit.
fn run_limited(label: &str, script: &str, time_limit: Duration, mem_limit: usize) {
    let t = Instant::now();
    let rt = Runtime::new().unwrap();
    rt.set_memory_limit(mem_limit);
    rt.set_max_stack_size(512 * 1024);
    let deadline = Instant::now() + time_limit;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let ctx = Context::full(&rt).unwrap();
    let outcome = ctx.with(|ctx| match ctx.eval::<String, _>(script) {
        Ok(v) => format!("ok -> {v}"),
        Err(e) => format!("stopped/error: {e}"),
    });
    println!("[{label}] {:>8.1?}  {outcome}", t.elapsed());
}

fn main() {
    println!("RSS at start: {:.1} MB", rss_mb());

    let t = Instant::now();
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    println!("runtime + context init: {:?}", t.elapsed());

    ctx.with(|ctx| {
        let t = Instant::now();
        let v: i32 = ctx
            .eval("let s = 0; for (let i = 0; i < 5000000; i++) { s += i % 7; } s")
            .unwrap();
        println!("5M-iteration loop: {:?} (result {v})", t.elapsed());

        let t = Instant::now();
        let v: String = ctx
            .eval(r#"JSON.stringify(JSON.parse('{"a":[1,2,3],"b":{"c":"d"}}').b)"#)
            .unwrap();
        println!("JSON parse/stringify: {:?} (result {v})", t.elapsed());
    });
    println!("RSS after warm run: {:.1} MB\n", rss_mb());

    println!("--- sandbox containment ---");
    run_limited("infinite loop", "while (true) {}; 'unreachable'", Duration::from_secs(1), 32 << 20);
    run_limited(
        "memory bomb",
        "let a = []; while (true) { a.push('x'.repeat(1000000)); } 'unreachable'",
        Duration::from_secs(5),
        32 << 20,
    );
    run_limited(
        "forbidden globals",
        "[typeof require, typeof process, typeof fetch, typeof XMLHttpRequest, typeof std, typeof os, typeof Deno].join(',')",
        Duration::from_secs(1),
        32 << 20,
    );
    run_limited("deep recursion", "function f(){ return f()+1 } f()", Duration::from_secs(1), 32 << 20);
    println!("\nfinal RSS: {:.1} MB", rss_mb());
}

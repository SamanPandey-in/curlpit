//! Pre-request and test scripts (FR-SCR) in a sandboxed QuickJS engine (D10).
//!
//! Every run gets a fresh runtime with time, memory and stack limits, and no file, network or
//! process access (QuickJS has none unless the `std`/`os` modules are added, and they aren't).
//! The Postman-style `pm` API is plain JavaScript (`prelude.js`); data crosses the boundary as
//! JSON, so the Rust side stays tiny.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, CaughtError, Context, Runtime};
use serde::{Deserialize, Serialize};

const PRELUDE: &str = include_str!("prelude.js");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    PreRequest,
    Test,
}

impl Phase {
    fn event_name(self) -> &'static str {
        match self {
            Phase::PreRequest => "prerequest",
            Phase::Test => "test",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub time: Duration,
    pub memory: usize,
    pub stack: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(5),
            memory: 64 * 1024 * 1024,
            stack: 512 * 1024,
        }
    }
}

/// The request as the script sees it: before variable substitution, enabled headers only.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Raw body text (empty for form bodies).
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptResponse {
    pub code: u16,
    pub status: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub time_ms: u64,
    pub size: u64,
}

pub type VarMap = BTreeMap<String, String>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptInput {
    pub request_name: String,
    pub request: ScriptRequest,
    pub response: Option<ScriptResponse>,
    pub environment_name: Option<String>,
    pub environment: VarMap,
    pub globals: VarMap,
    /// Runtime ("local") variables: highest precedence, live for this send only.
    pub variables: VarMap,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptOutput {
    pub environment: VarMap,
    pub globals: VarMap,
    pub variables: VarMap,
    /// The request after a pre-request script changed it (unchanged for test scripts).
    pub request: ScriptRequest,
    pub tests: Vec<TestResult>,
    pub logs: Vec<LogLine>,
    /// Syntax error, uncaught exception, time-out or out-of-memory.
    pub error: Option<String>,
    pub elapsed: Duration,
}

impl ScriptOutput {
    pub fn passed(&self) -> usize {
        self.tests.iter().filter(|t| t.passed && !t.skipped).count()
    }
    pub fn failed(&self) -> usize {
        self.tests.iter().filter(|t| !t.passed).count()
    }
}

#[derive(Serialize)]
struct JsInput<'a> {
    phase: &'static str,
    request_name: &'a str,
    request: &'a ScriptRequest,
    response: Option<&'a ScriptResponse>,
    environment_name: Option<&'a str>,
    environment: &'a VarMap,
    globals: &'a VarMap,
    variables: &'a VarMap,
}

#[derive(Deserialize)]
struct JsOutput {
    environment: VarMap,
    globals: VarMap,
    variables: VarMap,
    request: ScriptRequest,
    tests: Vec<TestResult>,
    logs: Vec<(String, String)>,
}

/// Run one script. Never panics: anything that goes wrong is in `ScriptOutput::error`, and
/// tests/variable changes made before the failure are still returned.
pub fn run(script: &str, phase: Phase, input: &ScriptInput, limits: Limits) -> ScriptOutput {
    let started = Instant::now();
    let mut out = ScriptOutput {
        environment: input.environment.clone(),
        globals: input.globals.clone(),
        variables: input.variables.clone(),
        request: input.request.clone(),
        ..Default::default()
    };
    if script.trim().is_empty() {
        return out;
    }
    let result = run_inner(script, phase, input, limits, &mut out);
    if let Err(e) = result {
        out.error.get_or_insert(e);
    }
    out.elapsed = started.elapsed();
    out
}

fn run_inner(
    script: &str,
    phase: Phase,
    input: &ScriptInput,
    limits: Limits,
    out: &mut ScriptOutput,
) -> Result<(), String> {
    let rt = Runtime::new().map_err(|e| format!("script engine failed to start: {e}"))?;
    rt.set_memory_limit(limits.memory);
    rt.set_max_stack_size(limits.stack);
    let armed = Arc::new(AtomicBool::new(true));
    let deadline = Instant::now() + limits.time;
    {
        let armed = armed.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            armed.load(Ordering::Relaxed) && Instant::now() > deadline
        })));
    }
    let ctx = Context::full(&rt).map_err(|e| format!("script engine failed to start: {e}"))?;
    let js_input = serde_json::to_string(&JsInput {
        phase: phase.event_name(),
        request_name: &input.request_name,
        request: &input.request,
        response: input.response.as_ref(),
        environment_name: input.environment_name.as_deref(),
        environment: &input.environment,
        globals: &input.globals,
        variables: &input.variables,
    })
    .map_err(|e| e.to_string())?;

    ctx.with(|ctx| -> Result<(), String> {
        ctx.globals()
            .set("__input", js_input)
            .map_err(|e| e.to_string())?;
        ctx.eval::<(), _>(PRELUDE)
            .catch(&ctx)
            .map_err(|e| format!("internal script error: {}", describe(e)))?;

        // The user's script. Its failure is reported, but what it did before still counts.
        if let Err(e) = ctx.eval::<(), _>(script).catch(&ctx) {
            out.error = Some(if Instant::now() > deadline {
                format!(
                    "Script stopped: it ran longer than {} s",
                    limits.time.as_secs_f32()
                )
            } else {
                describe(e)
            });
        }

        // Collect results even after a time-out (the interrupt is disarmed for this part).
        armed.store(false, Ordering::Relaxed);
        let json: String = ctx
            .eval("__finish()")
            .catch(&ctx)
            .map_err(|e| format!("could not read script results: {}", describe(e)))?;
        let o: JsOutput = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        out.environment = o.environment;
        out.globals = o.globals;
        out.variables = o.variables;
        out.request = o.request;
        out.tests = o.tests;
        out.logs = o
            .logs
            .into_iter()
            .map(|(level, text)| LogLine {
                level: match level.as_str() {
                    "info" => LogLevel::Info,
                    "warn" => LogLevel::Warn,
                    "error" => LogLevel::Error,
                    _ => LogLevel::Log,
                },
                text,
            })
            .collect();
        Ok(())
    })
}

fn describe(e: CaughtError<'_>) -> String {
    match e {
        CaughtError::Exception(ex) => {
            let msg = ex.message().unwrap_or_default();
            let name = ex
                .get::<_, Option<String>>("name")
                .ok()
                .flatten()
                .unwrap_or_else(|| "Error".into());
            // first stack frame tells the user the line
            let line = ex.stack().and_then(|s| {
                s.lines()
                    .find(|l| l.contains("<eval>"))
                    .map(str::trim)
                    .map(String::from)
            });
            match line {
                Some(l) => format!("{name}: {msg} ({l})"),
                None => format!("{name}: {msg}"),
            }
        }
        CaughtError::Value(v) => format!(
            "Uncaught: {}",
            v.as_string()
                .and_then(|s| s.to_string().ok())
                .unwrap_or_else(|| format!("{v:?}"))
        ),
        CaughtError::Error(e) => {
            let s = e.to_string();
            if s.to_lowercase().contains("memory") {
                "Script stopped: it used too much memory".into()
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ScriptInput {
        ScriptInput {
            request_name: "Get user".into(),
            request: ScriptRequest {
                method: "GET".into(),
                url: "https://{{host}}/users/1".into(),
                headers: vec![("Accept".into(), "application/json".into())],
                body: String::new(),
            },
            environment_name: Some("Dev".into()),
            environment: [("host".to_string(), "api.test".to_string())].into(),
            globals: [("g".to_string(), "1".to_string())].into(),
            ..Default::default()
        }
    }

    fn with_response(code: u16, body: &str) -> ScriptInput {
        let mut i = input();
        i.response = Some(ScriptResponse {
            code,
            status: "OK".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into(),
            time_ms: 42,
            size: body.len() as u64,
        });
        i
    }

    fn quick() -> Limits {
        Limits {
            time: Duration::from_millis(300),
            ..Default::default()
        }
    }

    #[test]
    fn empty_script_is_a_no_op() {
        let i = input();
        let o = run("  \n", Phase::PreRequest, &i, Limits::default());
        assert_eq!(o.environment, i.environment);
        assert_eq!(o.request, i.request);
        assert!(o.error.is_none() && o.tests.is_empty());
    }

    #[test]
    fn tests_pass_and_fail_with_messages() {
        let i = with_response(
            200,
            r#"{"id": 1, "name": "Ada", "tags": ["a","b"], "n": null}"#,
        );
        let script = r#"
            pm.test("status", () => pm.response.to.have.status(200));
            pm.test("ok", () => pm.response.to.be.ok);
            const j = pm.response.json();
            pm.test("name", () => pm.expect(j.name).to.equal("Ada"));
            pm.test("wrong", () => pm.expect(j.id).to.equal(2));
            pm.test("deep", () => pm.expect(j.tags).to.eql(["a","b"]));
            pm.test("prop", () => pm.expect(j).to.have.property("name", "Ada"));
            pm.test("include", () => pm.expect(j.tags).to.include("b"));
            pm.test("type", () => pm.expect(j.tags).to.be.an("array"));
            pm.test("null", () => pm.expect(j.n).to.be.null);
            pm.test("not", () => pm.expect(j.id).to.not.equal(5));
            pm.test("above", () => pm.expect(pm.response.responseTime).to.be.below(1000));
            pm.test("header", () => pm.response.to.have.header("content-type"));
            pm.test("length", () => pm.expect(j.tags).to.have.lengthOf(3));
            pm.test.skip("later");
        "#;
        let o = run(script, Phase::Test, &i, Limits::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        let failed: Vec<_> = o
            .tests
            .iter()
            .filter(|t| !t.passed)
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(failed, vec!["wrong", "length"]);
        let wrong = o.tests.iter().find(|t| t.name == "wrong").unwrap();
        assert_eq!(wrong.error.as_deref(), Some("expected 1 to equal 2"));
        let length = o.tests.iter().find(|t| t.name == "length").unwrap();
        assert!(length.error.as_deref().unwrap().contains("but got 2"));
        assert_eq!(o.passed(), 11);
        assert_eq!(o.failed(), 2);
        assert!(o.tests.iter().any(|t| t.skipped));
    }

    #[test]
    fn status_failure_message() {
        let o = run(
            r#"pm.test("s", () => pm.response.to.have.status(201)); pm.test("ok", () => pm.response.to.be.ok);"#,
            Phase::Test,
            &with_response(404, "{}"),
            Limits::default(),
        );
        assert_eq!(
            o.tests[0].error.as_deref(),
            Some("expected response to have status 201 but got 404")
        );
        assert!(o.tests[1].error.as_deref().unwrap().contains("ok (2xx)"));
    }

    #[test]
    fn variables_set_unset_and_precedence() {
        let script = r#"
            pm.environment.set("token", "abc");
            pm.environment.set("n", 5);
            pm.environment.set("obj", {a: 1});
            pm.environment.unset("host");
            pm.globals.set("g", "2");
            pm.variables.set("local", "x");
            console.log("g is", pm.globals.get("g"), pm.variables.get("g"));
            pm.test("lookup order", () => pm.expect(pm.variables.get("local")).to.equal("x"));
            pm.test("replaceIn", () => pm.expect(pm.variables.replaceIn("{{local}}-{{g}}-{{nope}}")).to.equal("x-2-{{nope}}"));
            pm.test("env name", () => pm.expect(pm.environment.name).to.equal("Dev"));
        "#;
        let o = run(script, Phase::PreRequest, &input(), Limits::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        assert!(o.tests.iter().all(|t| t.passed), "{:?}", o.tests);
        assert_eq!(o.environment.get("token").map(String::as_str), Some("abc"));
        assert_eq!(o.environment.get("n").map(String::as_str), Some("5"));
        assert_eq!(
            o.environment.get("obj").map(String::as_str),
            Some("{\"a\":1}")
        );
        assert!(!o.environment.contains_key("host"));
        assert_eq!(o.globals.get("g").map(String::as_str), Some("2"));
        assert_eq!(o.variables.get("local").map(String::as_str), Some("x"));
        assert_eq!(o.logs[0].text, "g is 2 2");
        assert_eq!(o.logs[0].level, LogLevel::Log);
    }

    #[test]
    fn pre_request_can_change_the_request() {
        let script = r#"
            pm.request.headers.add({key: "X-Trace", value: "1"});
            pm.request.headers.upsert({key: "accept", value: "text/plain"});
            pm.request.headers.remove("nope");
            pm.request.url = pm.request.url + "?v=2";
            pm.request.body.raw = JSON.stringify({ts: 1});
        "#;
        let o = run(script, Phase::PreRequest, &input(), Limits::default());
        assert!(o.error.is_none(), "{:?}", o.error);
        assert_eq!(
            o.request.headers,
            vec![
                ("Accept".to_string(), "text/plain".to_string()),
                ("X-Trace".to_string(), "1".to_string())
            ]
        );
        assert_eq!(o.request.url, "https://{{host}}/users/1?v=2");
        assert_eq!(o.request.body, "{\"ts\":1}");
    }

    #[test]
    fn errors_are_reported_and_earlier_work_is_kept() {
        let o = run(
            "pm.test('first', () => {}); pm.environment.set('a', '1'); undefinedFunction();",
            Phase::Test,
            &with_response(200, "{}"),
            Limits::default(),
        );
        let err = o.error.unwrap();
        assert!(err.contains("ReferenceError"), "{err}");
        assert_eq!(o.tests.len(), 1);
        assert_eq!(o.environment.get("a").map(String::as_str), Some("1"));

        let syntax = run(
            "pm.test('x', () => {",
            Phase::Test,
            &input(),
            Limits::default(),
        );
        assert!(syntax.error.unwrap().contains("SyntaxError"));

        let thrown = run("throw 'boom'", Phase::Test, &input(), Limits::default());
        assert!(thrown.error.unwrap().contains("boom"));

        // a broken response JSON fails only that test
        let bad = run(
            "pm.test('json', () => pm.response.json());",
            Phase::Test,
            &with_response(200, "not json"),
            Limits::default(),
        );
        assert!(bad.error.is_none());
        assert!(!bad.tests[0].passed);
    }

    #[test]
    fn sandbox_limits_hold() {
        let started = Instant::now();
        let o = run(
            "pm.test('before', () => {}); while (true) {}",
            Phase::Test,
            &input(),
            quick(),
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(o.error.unwrap().contains("ran longer than"), "time limit");
        assert_eq!(o.tests.len(), 1, "results before the time-out are kept");

        let o = run(
            "let a = []; while (true) { a.push('x'.repeat(1000000)); }",
            Phase::Test,
            &input(),
            Limits {
                memory: 16 << 20,
                ..Default::default()
            },
        );
        assert!(o.error.is_some(), "memory limit");

        let o = run(
            "function f(){ return f() + 1 } f()",
            Phase::Test,
            &input(),
            quick(),
        );
        assert!(o.error.is_some(), "stack limit");

        let o = run(
            r#"pm.test("no host access", () => {
                 pm.expect([typeof require, typeof process, typeof fetch, typeof XMLHttpRequest, typeof std, typeof os].join(","))
                   .to.equal("undefined,undefined,undefined,undefined,undefined,undefined");
               });"#,
            Phase::Test,
            &input(),
            Limits::default(),
        );
        assert!(o.tests[0].passed, "{:?}", o.tests[0].error);
    }

    #[test]
    fn legacy_postman_api() {
        let o = run(
            r#"tests["is 200"] = responseCode.code === 200;
               tests["bad"] = false;
               postman.setEnvironmentVariable("x", JSON.parse(responseBody).id);"#,
            Phase::Test,
            &with_response(200, r#"{"id": 7}"#),
            Limits::default(),
        );
        assert!(o.error.is_none(), "{:?}", o.error);
        assert_eq!(o.tests.len(), 2);
        assert!(o.tests[0].passed && !o.tests[1].passed);
        assert_eq!(o.environment.get("x").map(String::as_str), Some("7"));
    }

    #[test]
    fn unsupported_calls_say_so() {
        let o = run(
            "pm.sendRequest('https://x', () => {})",
            Phase::PreRequest,
            &input(),
            Limits::default(),
        );
        assert!(o.error.unwrap().contains("isn't supported yet"));
    }

    #[test]
    fn console_output_is_bounded() {
        let o = run(
            "for (let i = 0; i < 2000; i++) console.log(i)",
            Phase::Test,
            &input(),
            Limits::default(),
        );
        assert_eq!(o.logs.len(), 500);
    }
}

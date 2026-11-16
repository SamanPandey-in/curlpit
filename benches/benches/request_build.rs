//! Request building (FR-ENG-02), the app-added overhead NFR-PERF-03 budgets at under 5 ms.
//!
//! `at_core::build` resolves `{{variables}}` everywhere, percent-encodes and appends the enabled
//! params, merges headers, applies auth and serializes the body. It runs once per Send on the UI
//! thread, so it has to stay far below the 5 ms target.

mod support;

use at_core::{build, Auth, Body, KeyValue, Method, Request, RequestSettings, Variables};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn vars() -> Variables {
    let mut v = Variables::default();
    v.global
        .insert("base_url".into(), "https://api.example.test".into());
    v.collection.insert("api_version".into(), "v1".into());
    v.environment.insert("order_id".into(), "ord_8821".into());
    v.environment.insert("locale".into(), "en-GB".into());
    v.environment
        .insert("trace_id".into(), "4b1f9e2a7c3d4e5f".into());
    v.runtime.insert("api_user".into(), "svc-checkout".into());
    v.runtime
        .insert("api_secret".into(), "s3cr3t-token-value".into());
    v
}

/// A representative request: params, headers, auth and a JSON body, all with variables in them.
fn representative() -> Request {
    Request {
        id: "req-create-order".into(),
        name: "Create order".into(),
        method: Method::Post,
        url: "{{base_url}}/{{api_version}}/orders/{{order_id}}/confirm".into(),
        pre_request_script: String::new(),
        test_script: String::new(),
        params: vec![
            KeyValue::new("expand", "items,customer"),
            KeyValue::new("locale", "{{locale}}"),
            KeyValue::new("dry_run", "false"),
            KeyValue {
                key: "legacy".into(),
                value: "ignored".into(),
                description: "disabled rows must be skipped".into(),
                enabled: false,
            },
        ],
        headers: vec![
            KeyValue::new("Accept", "application/json"),
            KeyValue::new("X-Request-Id", "{{trace_id}}"),
            KeyValue::new("X-Client", "api-tester/0.0.1"),
            KeyValue::new("Accept-Language", "{{locale}}"),
        ],
        body: Body::Json {
            raw: concat!(
                "{\n",
                "  \"order_id\": \"{{order_id}}\",\n",
                "  \"idempotency_key\": \"{{trace_id}}\",\n",
                "  \"items\": [\n",
                "    { \"sku\": \"SKU-1\", \"qty\": 2, \"price\": 19.99 },\n",
                "    { \"sku\": \"SKU-2\", \"qty\": 1, \"price\": 249.00 }\n",
                "  ],\n",
                "  \"notes\": \"ship to {{locale}} warehouse\"\n",
                "}"
            )
            .into(),
        },
        auth: Auth::Basic {
            username: "{{api_user}}".into(),
            password: "{{api_secret}}".into(),
        },
        settings: RequestSettings::default(),
    }
}

/// The same request with 25 headers, to show how build scales with a fat header table.
fn many_headers() -> Request {
    let mut req = representative();
    req.headers = (0..25)
        .map(|i| KeyValue::new(&format!("X-Custom-{i}"), "{{trace_id}}"))
        .collect();
    req
}

fn request_build(c: &mut Criterion) {
    let vars = vars();
    let rep = representative();
    let fat = many_headers();

    let mut g = c.benchmark_group("request_build");
    g.bench_function("post_json_params_headers_basic_auth", |b| {
        b.iter(|| build(black_box(&rep), black_box(&vars)))
    });
    g.bench_function("post_json_25_headers", |b| {
        b.iter(|| build(black_box(&fat), black_box(&vars)))
    });
    g.finish();
}

criterion_group!(benches, request_build);
criterion_main!(benches);

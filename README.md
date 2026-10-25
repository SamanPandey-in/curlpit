# API Tester (GPUI) — implementation workspace

Status: **Phase 0 scaffold + Phase 1 core engine written, not yet compiled** (no Rust toolchain was available where it was authored). Start with `docs/testing/00-setup.md`.

- `crates/core` — models, variables, request build pipeline
- `crates/http` — HTTP engine (cancel, timeout, errors, disk spill)
- `crates/demo` — `core-demo` CLI for manual checks
- `docs/DECISIONS.md` — Rust decisions taken on your behalf
- `docs/testing/` — how to test at each checkpoint

Next: Phase 0 spikes (GPUI window, 10 MB viewer, script engine, editor), then Phase 2 app shell.

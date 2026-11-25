# Contributing to Curlpit

Thanks for helping. This page covers how to build, test and lint, how commits and decisions are recorded, and how features are tested by hand.

## Setup

- Rust **stable**. The repo's `rust-toolchain.toml` pins the channel and pulls in `rustfmt` and `clippy`. The MSRV is 1.80 (decision D1).
- A C toolchain, which the bundled SQLite and QuickJS need: the Xcode Command Line Tools on macOS (not the full Xcode, see D17), the MSVC Build Tools on Windows, or `build-essential` and `pkg-config` on Linux.
- On Linux, the GUI crate also needs the libraries CI installs:
  ```sh
  sudo apt-get install -y libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libvulkan-dev libx11-xcb-dev libxcb1-dev libfontconfig1-dev libfreetype-dev
  ```

## Build, test, lint

The workspace has two halves. The library crates are in `default-members`, so plain `cargo` commands cover them quickly. The GUI crate `at-app` (and `benches`) are outside `default-members` because GPUI is slow to build (D21, D38), so build them with `-p`.

```sh
# Library crates: what plain `cargo test` covers
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

# The GUI crate, separately
cargo clippy -p at-app --all-targets -- -D warnings
cargo test -p at-app
cargo run -p at-app                       # debug build of the app
cargo build -p at-app --release           # target/release/api-tester

# Benchmarks and the wall-clock budget gate (see docs/testing/21-phase4c-benchmarks.md)
cargo bench -p at-benches
cargo test -p at-benches --release -- --ignored --nocapture
```

CI (`.github/workflows/ci.yml`) runs the format check, clippy and tests on the default members, and a second job lints, tests and release-builds `at-app`, all on Linux, macOS and Windows. Run the same commands before you push. On a machine short of memory, add `-j 2` to cargo to limit parallel jobs.

To run the app without touching your real data, use a throw-away data directory:

```sh
API_TESTER_DATA_DIR=/tmp/curlpit-dev cargo run -p at-app
```

An empty directory is seeded with the "Getting started" sample on first launch, which makes a quick smoke test: send each request.

## Code conventions

- `rustfmt` defaults (D16). CI fails on unformatted code.
- Clippy must pass with `-D warnings`.
- No `unwrap` in non-test code paths. Libraries return `thiserror` errors, and the app reports them in the status line rather than panicking (D12).
- Keep logic out of the GUI crate where you can. Pure, UI-free code belongs in a library crate (`at-core`, `at-store`, `at-runner` and so on), where `cargo test` and CI cover it. `at-app` should hold UI state and wiring (D39, D46).
- `at-core` depends only on `serde` and `thiserror` (D3).
- On-disk formats are additive. A new field needs `#[serde(default)]` and an old-file test that proves older files still load (see the `old_*` tests in `crates/store`).

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/), as the history does:

```
feat(store): seed a getting-started collection on first run
fix(app): keep the find highlight on the right row after a new response
perf(http): cache reqwest clients per engine settings
docs: rewrite README and add user guides
ci: build, lint and test the GUI crate on all three OSes
```

The scope is the crate or area (`core`, `http`, `store`, `runner`, `app`, `interop`, `codegen`, `script`, `ui`). Keep each commit to one logical change, and keep pure moves separate from behaviour changes.

## Decisions: `docs/DECISIONS.md`

Any choice someone might later ask "why?" about gets an entry in [`docs/DECISIONS.md`](docs/DECISIONS.md): a library, a file format, a behaviour trade-off, a deliberate limitation. The convention:

- Entries are numbered in order (`D1`, `D2`, … the latest is near the bottom). Take the next free number, and never renumber or delete an entry. If a decision changes, add a new entry that says what it supersedes.
- Use a heading of the form `## D<n>: <short title> (<YYYY-MM-DD>)`, then a few bullets: **what** was decided, **why**, what it **costs**, and what was **deliberately not done**.
- State limits plainly: "stored, not applied yet" is more useful than silence.
- Refer to entries by number in code comments, commit messages and docs ("see D44").

## Testing docs: `docs/testing/`

Each feature slice ships with a numbered by-hand test document in [`docs/testing/`](docs/testing/), such as `16-phase3j-scripts.md`. It is the record of how the feature is meant to behave, and the checklist a person runs in the real GUI, since most UI behaviour can't be unit-tested. Each one has:

- **New:** what the slice adds, in user terms;
- **Known limits** / **Not yet:** what it deliberately doesn't do;
- **Setup:** the commands to build it and the expected test counts, usually with a fresh `API_TESTER_DATA_DIR`;
- **Checklist:** a table of numbered rows (Do → Expect), plus regression rows that point back at earlier docs.

When you add or change user-visible behaviour, add a new numbered doc or update the relevant one, and update the user guides in [`docs/user/`](docs/user/) if what users see changes. Performance numbers go in [`docs/BASELINE.md`](docs/BASELINE.md), with the machine and the command used to reproduce them.

## Where things are

The README has the [crate map](README.md#project-layout). Requirements are in [`docs/SRS.md`](docs/SRS.md) (requirement IDs like `FR-SCR-01` and `NFR-PERF-05` are cited in code and docs), the phased plan is in [`docs/PLAN.md`](docs/PLAN.md), and the current gap list and roadmap are in [`GAPS-AND-ROADMAP.md`](GAPS-AND-ROADMAP.md) and [`docs/IMPLEMENTATION-PLAN.md`](docs/IMPLEMENTATION-PLAN.md).

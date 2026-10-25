# 00 — Setup (do this once)

You need: Rust (stable), a C linker (comes with Xcode CLT / build-essential / MSVC Build Tools), and internet for the first build.

1. Install Rust: https://rustup.rs  (macOS/Linux: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`; Windows: run `rustup-init.exe`).
2. Open a new terminal: `rustc --version` should print 1.80 or newer.
3. In the project folder: `cargo build --workspace`

**If the build fails, that is a valuable result, not your mistake.** The code in this drop was written without a compiler available to me, so a typo or an API mismatch is possible. Copy the *first* error block and send it to me; I'll fix it.

Common first-build issues:
- `zstd` feature not found for reqwest → delete `"zstd"` from the reqwest features in the root `Cargo.toml`.
- Linker not found on Linux → `sudo apt install build-essential pkg-config`.

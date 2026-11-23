//! Build script for the app binary.

fn main() {
    // Windows gives the main thread a 1 MB stack, and gpui lays out and paints the whole
    // element tree recursively on it. Unoptimised builds use far larger stack frames, and
    // a page with several text inputs (the Variables page) overflowed it and crashed.
    // Zed raises its main-thread stack the same way; 8 MB leaves ample headroom.
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}

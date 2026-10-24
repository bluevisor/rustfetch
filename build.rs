// Haiku reads its GPU and screen through a small C++ shim, BScreen has no C API
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("haiku") {
        return;
    }

    println!("cargo:rerun-if-changed=src/platform/haiku_graphics.cpp");
    // cc's rustc-link-lib lines would only reach the lib target, main.rs compiles the modules
    // again, so haiku.rs names the libraries with #[link] and only the search path is set here
    cc::Build::new()
        .cpp(true)
        .cargo_metadata(false)
        .file("src/platform/haiku_graphics.cpp")
        .compile("rustfetch_haiku_graphics");
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        println!("cargo:rustc-link-search=native={}", out_dir);
    }
}

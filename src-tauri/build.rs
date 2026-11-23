fn main() {
    tauri_build::build();
    // Tauri embeds the app resource for normal binaries, but Rust's library
    // test harness also links native dialog code when testing command dispatch.
    // Ask MSVC to embed the same Common Controls dependency in every executable
    // it links, including that harness. No system DLL replacement is needed.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // The application binary already contains Tauri's resource manifest.
        // Do not also synthesize another resource #1 at link time. Test and
        // example executables still need the generated Common Controls one.
        println!("cargo:rustc-link-arg-bin=browsory=/MANIFEST:NO");
    }
}

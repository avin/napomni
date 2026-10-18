fn main() {
    // Match GPUI Kit's Windows stack reserve for component-heavy debug renders.
    // Apply it only to our executable so dependencies keep their cached builds.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=win-notifi-toast=/STACK:8388608");
    }
}

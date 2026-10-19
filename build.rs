fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("assets/app.rc", ["napomni"], embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed application icon");
    }

    // Match GPUI Kit's Windows stack reserve for component-heavy debug renders.
    // Apply it only to our executable so dependencies keep their cached builds.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=napomni=/STACK:8388608");
    }
}

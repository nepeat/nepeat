//! Tells the crate which glue sdk-glue-sys links (its `links` metadata): the real glue's calls
//! (module `real`) exist only when something may provide them, i.e. not `ob_glue_rust_only`
//! (nothing linked and the feature `rust-fake`: a program on the Rust fake alone);
//! `ob_glue_linked`: the real glue is linked.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(ob_glue_linked)");
    println!("cargo::rustc-check-cfg=cfg(ob_glue_rust_only)");
    println!("cargo::rerun-if-env-changed=DEP_OB_SDK_GLUE_LINKED");
    let linked = std::env::var("DEP_OB_SDK_GLUE_LINKED").unwrap_or_default();
    if linked == "real" {
        println!("cargo::rustc-cfg=ob_glue_linked");
    } else if std::env::var_os("CARGO_FEATURE_RUST_FAKE").is_some() {
        println!("cargo::rustc-cfg=ob_glue_rust_only");
    }
}

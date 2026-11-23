//! Tells the crate which glue sdk-glue-sys links (its `links` metadata): the module with the C
//! fake's test hooks exists only when the C fake is linked (`ob_glue_fake`), the real glue's
//! calls only when something may provide them (not `ob_glue_rust_only`: nothing linked and the
//! feature `rust-fake`, i.e. a program on the Rust fake alone).
fn main() {
    println!("cargo::rustc-check-cfg=cfg(ob_glue_fake)");
    println!("cargo::rustc-check-cfg=cfg(ob_glue_linked)");
    println!("cargo::rustc-check-cfg=cfg(ob_glue_rust_only)");
    println!("cargo::rerun-if-env-changed=DEP_OB_SDK_GLUE_LINKED");
    let linked = std::env::var("DEP_OB_SDK_GLUE_LINKED").unwrap_or_default();
    if linked == "fake" {
        println!("cargo::rustc-cfg=ob_glue_fake");
    }
    if linked == "fake" || linked == "real" {
        println!("cargo::rustc-cfg=ob_glue_linked");
    } else if std::env::var_os("CARGO_FEATURE_RUST_FAKE").is_some() {
        println!("cargo::rustc-cfg=ob_glue_rust_only");
    }
}

//! Tells the crate which glue sdk-glue-sys links (its `links` metadata): the module with the
//! fake's test hooks exists only when the fake is linked, whoever enabled it (this crate's
//! feature, a dependent's, or the dev-dependency of the tests).
fn main() {
    println!("cargo::rustc-check-cfg=cfg(ob_glue_fake)");
    println!("cargo::rerun-if-env-changed=DEP_OB_SDK_GLUE_LINKED");
    if std::env::var("DEP_OB_SDK_GLUE_LINKED").is_ok_and(|linked| linked == "fake") {
        println!("cargo::rustc-cfg=ob_glue_fake");
    }
}

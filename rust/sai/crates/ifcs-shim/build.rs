//! The soname libsai's loader (`ifcs_lib.cpp`, later crate `ifcs`) and the C shim it replaces use.
fn main() {
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libsai-teralynx-shim.so");
}

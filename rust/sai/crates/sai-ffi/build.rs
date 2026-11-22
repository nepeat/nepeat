//! Link settings of libsai.so and, for the C harness test, where the SAI headers are.
fn main() {
    // The name syncd's link line and the dynamic loader look for.
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libsai.so.1");
    // Exported by sai-sys's build script to its direct dependents (links = "sai-headers").
    if let Some(inc) = std::env::var_os("DEP_SAI_HEADERS_INCLUDE") {
        println!("cargo:rustc-env=SAI_FFI_SAI_INC={}", inc.display());
    }
}

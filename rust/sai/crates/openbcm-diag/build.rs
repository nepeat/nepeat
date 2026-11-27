//! No build step any more: this script compiled the C client `openbcm_diag.c` for the live differential, which went with the C / C++
//! in H5 (2026-11-27; the comparison is the golden in `tests/golden` now). The file stays so that the crate's build
//! metadata, which names its symbols, and so the `AS77xx` binaries built from it stay byte-identical.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}

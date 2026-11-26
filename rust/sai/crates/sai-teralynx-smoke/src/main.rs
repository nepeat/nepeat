//! `sai-teralynx-smoke` (crate docs).
#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(sai_teralynx_smoke::main(&args));
}

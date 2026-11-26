//! `sai-script [--libsai L] [--cpp-names] [--tool NAME=CMD ...] PROFILE SCRIPT.json` (crate docs).
#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(sai_script::main(&args));
}

//! `sflow-probe`: see [`tl_frames::sflow`].
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    std::process::ExitCode::from(u8::try_from(tl_frames::sflow::main(&args)).unwrap_or(2))
}

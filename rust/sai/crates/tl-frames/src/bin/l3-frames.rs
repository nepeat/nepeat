//! `l3-frames`: see [`tl_frames::l3`].
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    std::process::ExitCode::from(u8::try_from(tl_frames::l3::main(&args)).unwrap_or(2))
}

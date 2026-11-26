//! `trap-frames`: see [`tl_frames::trap`].
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    std::process::ExitCode::from(u8::try_from(tl_frames::trap::main(&args)).unwrap_or(2))
}

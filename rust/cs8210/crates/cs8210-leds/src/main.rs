//! `cs8210-leds`: LED status, test patterns for a human at the front panel, the
//! system LED keeper.
#![forbid(unsafe_code)]

/// println! that ignores a closed stdout (`| head`) instead of panicking.
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cs8210_leds::{Bus, Colour, I2c, MACSEC_LED, SWITCH_LED, SYSTEM, SystemLed, reg};

const USAGE: &str = "usage:
  cs8210-leds status                      both LED CPLDs (stream / test colour) and the system CPLD's LEDs
  cs8210-leds test COLOUR [--cages 1-16|17-32|all] [--seconds N]
                                          port LEDs of those cages in one colour (red green blue white off;
                                          yellow cyan magenta are inferred codes), back to the stream after N s (30)
  cs8210-leds walk [--step-ms N]          every colour on all ports, then halves, then the system LED states;
                                          ends in stream mode with the system LED as it was
  cs8210-leds stream                      port LEDs back to the ASIC's LED stream
  cs8210-leds system green|amber|red|off|blink-green|blink-amber|blink-red
  cs8210-leds monitor [--interval S]      keep the system LED: green while fand runs without failsafe and the
                                          I2C FPGA is alive, blinking amber otherwise
COLOUR: red green blue white yellow cyan magenta off";

const TEST_MARK: &str = "/run/cs8210-leds.test";

fn opt(a: &[String], name: &str) -> Option<String> {
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).cloned()
}

fn cpls(which: &str) -> Result<Vec<cs8210_leds::Device>, String> {
    Ok(match which {
        "1-16" => vec![MACSEC_LED],
        "17-32" => vec![SWITCH_LED],
        "all" => vec![MACSEC_LED, SWITCH_LED],
        x => return Err(format!("--cages {x}: 1-16, 17-32 or all")),
    })
}

fn status(b: &mut I2c) -> Result<(), String> {
    for (d, cages) in [(MACSEC_LED, "1-16"), (SWITCH_LED, "17-32")] {
        let v = b.read(d, reg::VERSION)?;
        let en = b.read(d, reg::LED_ENABLE)?;
        let c = b.read(d, reg::COLOUR)?;
        let mode = if en & 1 != 0 {
            format!("TEST colour {}", Colour::from_code(c))
        } else {
            "ASIC LED stream".into()
        };
        out!(
            "{:<16} cages {cages:<5} version {v:#04x}  0x09={en:#04x} 0x0a={c:#04x}  {mode}",
            d.name
        );
    }
    let l = b.read(SYSTEM, reg::SYS_LEDS)?;
    let m = b.read(SYSTEM, reg::SYS_LED_MODE)?;
    let manual = |bit: u8| m >> bit & 1 != 0;
    out!(
        "system-cpld      0x20={l:#04x} 0x21={m:#04x}  system LED {}",
        SystemLed::from_code(l)
    );
    let p1 = if manual(4) {
        cs8210_leds::psu_fan_colour(l >> 3)
    } else {
        "automatic"
    };
    let p2 = if manual(5) {
        cs8210_leds::psu_fan_colour(l >> 5)
    } else {
        "automatic"
    };
    let fan = if manual(7) {
        cs8210_leds::psu_fan_colour(m)
    } else {
        "automatic"
    };
    out!("                 PWR1 LED {p1}, PWR2 LED {p2}, fan LED {fan} (test-mode bits 0x21 b4/b5/b7)");
    Ok(())
}

fn mark(on: bool) {
    if on {
        let _ = std::fs::write(TEST_MARK, std::process::id().to_string());
    } else {
        let _ = std::fs::remove_file(TEST_MARK);
    }
}

fn stream_all(b: &mut I2c) -> Result<(), String> {
    for d in [MACSEC_LED, SWITCH_LED] {
        cs8210_leds::set_stream(b, d)?;
    }
    Ok(())
}

fn test(b: &mut I2c, a: &[String]) -> Result<(), String> {
    let c = a
        .first()
        .and_then(|s| Colour::parse(s))
        .ok_or_else(|| USAGE.to_string())?;
    let which = cpls(&opt(a, "--cages").unwrap_or_else(|| "all".into()))?;
    let secs: u64 = opt(a, "--seconds").map_or(Ok(30), |s| s.parse().map_err(|_| "--seconds N"))?;
    mark(true);
    let r = (|| {
        for d in &which {
            cs8210_leds::set_test(b, *d, c)?;
            out!(
                "{}: test colour {c} (code {}{}), read back",
                d.name,
                c.code(),
                if c.documented() { "" } else { ", inferred" }
            );
        }
        std::thread::sleep(Duration::from_secs(secs));
        Ok::<(), String>(())
    })();
    let back = stream_all(b);
    mark(false);
    out!("port LEDs back to the ASIC stream");
    r.and(back)
}

fn walk(b: &mut I2c, a: &[String]) -> Result<(), String> {
    let step = Duration::from_millis(opt(a, "--step-ms").map_or(Ok(1500), |s| s.parse().map_err(|_| "--step-ms N"))?);
    let sys_before = b.read(SYSTEM, reg::SYS_LEDS)?;
    mark(true);
    let r = (|| {
        for c in Colour::ALL {
            out!(
                "all 32 cages: {c}{}",
                if c.documented() { "" } else { " (inferred code)" }
            );
            for d in [MACSEC_LED, SWITCH_LED] {
                cs8210_leds::set_test(b, d, c)?;
            }
            std::thread::sleep(step);
        }
        for (lo, hi) in [
            (Colour::Red, Colour::Green),
            (Colour::Green, Colour::Red),
            (Colour::Blue, Colour::Off),
            (Colour::Off, Colour::Blue),
        ] {
            out!("cages 1-16: {lo}, cages 17-32: {hi}");
            cs8210_leds::set_test(b, MACSEC_LED, lo)?;
            cs8210_leds::set_test(b, SWITCH_LED, hi)?;
            std::thread::sleep(step);
        }
        stream_all(b)?;
        for l in [
            SystemLed::Green,
            SystemLed::Amber,
            SystemLed::Red,
            SystemLed::BlinkGreen,
            SystemLed::BlinkAmber,
            SystemLed::BlinkRed,
            SystemLed::Off,
        ] {
            out!("system LED: {l}");
            cs8210_leds::set_system(b, l)?;
            std::thread::sleep(step.max(Duration::from_secs(2)));
        }
        Ok::<(), String>(())
    })();
    let back = stream_all(b).and_then(|()| cs8210_leds::write_checked(b, SYSTEM, reg::SYS_LEDS, sys_before, 0xff));
    mark(false);
    out!(
        "end: port LEDs on the ASIC stream, system LED back to {}",
        SystemLed::from_code(sys_before)
    );
    r.and(back)
}

fn age_s(p: &Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    SystemTime::now().duration_since(m).ok().map(|d| d.as_secs())
}

fn monitor(b: &mut I2c, a: &[String]) -> Result<(), String> {
    let interval = Duration::from_secs(opt(a, "--interval").map_or(Ok(10), |s| s.parse().map_err(|_| "--interval S"))?);
    let fand = Path::new("/run/fand/state.json");
    let dead = Path::new("/run/cs8210-qsfp.dead");
    if !Path::new(TEST_MARK).exists() {
        stream_all(b)?;
    }
    let mut last = String::new();
    loop {
        let text = std::fs::read_to_string(fand).ok();
        let (want, why) = cs8210_leds::wanted_system(text.as_deref(), age_s(fand), dead.exists());
        if !Path::new(TEST_MARK).exists() {
            match cs8210_leds::set_system(b, want) {
                Ok(changed) => {
                    let line = format!("system LED {want}: {why}");
                    if changed || line != last {
                        eprintln!("cs8210-leds: {line}");
                        last = line;
                    }
                }
                Err(e) => eprintln!("cs8210-leds: {e}"),
            }
        }
        std::thread::sleep(interval);
    }
}

fn run(a: &[String]) -> Result<(), String> {
    let (cmd, rest) = a.split_first().ok_or_else(|| USAGE.to_string())?;
    let mut b = I2c::default();
    match cmd.as_str() {
        "status" => status(&mut b),
        "test" => test(&mut b, rest),
        "walk" => walk(&mut b, rest),
        "stream" => stream_all(&mut b),
        "system" => {
            let l = rest
                .first()
                .and_then(|s| SystemLed::parse(s))
                .ok_or_else(|| USAGE.to_string())?;
            cs8210_leds::set_system(&mut b, l).map(|_| out!("system LED {l} (read back)"))
        }
        "monitor" => monitor(&mut b, rest),
        "-h" | "--help" | "help" => {
            out!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match run(&a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

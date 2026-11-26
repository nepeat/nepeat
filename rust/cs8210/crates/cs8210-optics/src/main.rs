//! `cs8210-optics`: the daemon and its command-line client.
#![forbid(unsafe_code)]
// serde_json::Value["key"] returns Null for a missing key or a non-object: it never panics.
#![allow(clippy::indexing_slicing)]

/// println! that ignores a closed stdout (`| head`) instead of panicking.
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use cs8210_optics::daemon::{Config, Daemon};
use cs8210_optics::fpga::Fpga;
use cs8210_optics::regs::{Cage, MAP_LEN};
use cs8210_optics::{Backend, owner, pci, sim};
use cs8210_xcvr::bringup::PortMode;
use serde_json::{Value, json};

const SOCKET: &str = "/run/cs8210-optics/optics.sock";

const USAGE: &str = "usage:
  cs8210-optics daemon [--cage N[:PxG]]... [--metrics FILE] [--simulate DIR]
                       [--socket P] [--lock F] [--dead-file F] [--pci-root D] [--poll-ms N] [--dom-ms N]
  cs8210-optics status [N]          cages with a module or declared (N: one cage, lanes too)
  cs8210-optics json [N]            the same as JSON
  cs8210-optics sideband            raw presence / ResetL / LPMode / IntL masks
  cs8210-optics read N OFFSET LEN   module memory, optoe flat offsets (page n at 128 + 128 * n)
  cs8210-optics dump N              lower page + the upper pages the module has, hex
  cs8210-optics write N OFFSET BYTE...
  cs8210-optics lpmode N 0|1 | reset N | retry N
  cs8210-optics credo-temps < (phy_shell 'firmware_get_slice_temp slice_id=all')
                                    the Credo die temperatures as Prometheus text (a filter: no FPGA access)
  cs8210-optics credo-spec --spec SPEC [--wait S]
                                    the Credo loader's -m spec with each cage's line media from its module
  cs8210-optics guard --allowed LIST [--wait SECONDS]
                                    exit 0 only if the FPGA answers and every populated cage is in LIST
  (client commands: --socket P; the daemon is the only process that touches the FPGA)";

fn num(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let r = match t.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16),
        None => t.parse(),
    };
    r.map_err(|_| format!("not a number: {s}"))
}

fn num8(s: &str) -> Result<u8, String> {
    u8::try_from(num(s)?).map_err(|_| format!("out of range: {s}"))
}

fn cage_spec(spec: &str) -> Result<(Cage, Option<PortMode>), String> {
    let (number, mode) = spec.split_once(':').map_or((spec, None), |(a, b)| (a, Some(b)));
    let cage = Cage::new(num8(number)?).ok_or_else(|| format!("no cage {number}"))?;
    let Some(mode) = mode else { return Ok((cage, None)) };
    let (ports, gbps) = mode
        .trim_end_matches(['G', 'g'])
        .split_once('x')
        .ok_or_else(|| format!("mode {mode}: want PxG, e.g. 1x400"))?;
    let ports = num8(ports)?;
    if ![1, 2, 4, 8].contains(&ports) {
        return Err(format!("mode {mode}: 1, 2, 4 or 8 ports"));
    }
    let gbps = u32::try_from(num(gbps)?).map_err(|_| format!("mode {mode}: speed"))?;
    Ok((cage, Some(PortMode { ports, gbps })))
}

struct Args {
    rest: Vec<String>,
    socket: PathBuf,
}

fn take_opt(v: &mut Vec<String>, name: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    while let Some(i) = v.iter().position(|a| a == name) {
        if i + 1 >= v.len() {
            return Err(format!("{name} needs a value"));
        }
        out.push(v.remove(i + 1));
        v.remove(i);
    }
    Ok(out)
}

fn last(v: Vec<String>) -> Option<String> {
    v.into_iter().last()
}

fn daemon(mut a: Vec<String>, socket: &Path) -> Result<(), String> {
    let cages = take_opt(&mut a, "--cage")?
        .iter()
        .map(|s| cage_spec(s))
        .collect::<Result<Vec<_>, _>>()?;
    let mut cfg = Config {
        cages,
        ..Config::default()
    };
    cfg.metrics = last(take_opt(&mut a, "--metrics")?).map(PathBuf::from);
    if let Some(f) = last(take_opt(&mut a, "--dead-file")?) {
        cfg.dead_file = PathBuf::from(f);
    }
    if let Some(n) = last(take_opt(&mut a, "--poll-ms")?) {
        cfg.poll_ms = num(&n)?;
    }
    if let Some(n) = last(take_opt(&mut a, "--dom-ms")?) {
        cfg.dom_ms = num(&n)?;
    }
    let lock = last(take_opt(&mut a, "--lock")?).unwrap_or_else(|| owner::LOCK_FILE.into());
    let pci_root = last(take_opt(&mut a, "--pci-root")?).unwrap_or_else(|| "/sys/bus/pci/devices".into());
    let simulate = last(take_opt(&mut a, "--simulate")?);
    if let Some(x) = a.first() {
        return Err(format!("unexpected argument {x}"));
    }
    let _own = owner::acquire(Path::new(&lock), Duration::from_secs(10))?;
    let declared: Vec<String> = cfg
        .cages
        .iter()
        .map(|(c, m)| {
            format!(
                "{}{}",
                c.number(),
                m.map(|m| format!(":{}x{}G", m.ports, m.gbps)).unwrap_or_default()
            )
        })
        .collect();
    eprintln!(
        "cs8210-optics: owner of the I2C FPGA ({lock}); declared cages: {}",
        declared.join(" ")
    );
    let (backend, dev, dead) = match simulate {
        Some(dir) => (
            Backend::Sim(Box::new(
                sim::SimFpga::from_dir(Path::new(&dir)).map_err(|e| format!("{dir}: {e}"))?,
            )),
            None,
            None,
        ),
        None => open_device(Path::new(&pci_root), &cfg.dead_file),
    };
    let poll = if matches!(backend, Backend::Sim(_)) {
        Duration::ZERO
    } else {
        Duration::from_millis(1)
    };
    let mut fpga = Fpga::new(backend, poll);
    if let Some(why) = dead {
        eprintln!("cs8210-optics: I2C FPGA not used: {why}");
        fpga.mark_dead(why);
    }
    let listener = bind(socket)?;
    Daemon::new(cfg, fpga, dev).run(&listener)
}

/// PCI checks, decode, mapping. On a problem: no mapping and the reason (the daemon
/// still serves, answering that the FPGA is unavailable, so `SONiC` sees no modules and
/// the cage guard refuses syncd).
fn open_device(pci_root: &Path, dead_file: &Path) -> (Backend, Option<pci::Device>, Option<String>) {
    if let Some(why) = owner::dead_reason(dead_file) {
        return (
            Backend::None,
            None,
            Some(format!("marked dead ({}): {why}", dead_file.display())),
        );
    }
    let mut dev = match pci::Device::find(pci_root) {
        Ok(d) => d,
        Err(e) => return (Backend::None, None, Some(e)),
    };
    if let Some(why) = dev.upstream_problem() {
        let why = format!("root port {why}");
        let _ = owner::mark_dead(dead_file, &dev.bdf, &why, "startup");
        return (Backend::None, Some(dev), Some(why));
    }
    if let Err(e) = dev.check_bar() {
        return (Backend::None, Some(dev), Some(e));
    }
    match dev.decode_on() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!(
                "cs8210-optics: {}: PCI memory decode off; enabling it (what the vendor probe's pci_enable_device does)",
                dev.bdf
            );
            if let Err(e) = dev.enable() {
                return (Backend::None, Some(dev), Some(format!("enable: {e}")));
            }
            if !matches!(dev.decode_on(), Ok(true)) {
                return (
                    Backend::None,
                    Some(dev),
                    Some("memory decode still off after enable".into()),
                );
            }
        }
        Err(e) => return (Backend::None, Some(dev), Some(format!("config: {e}"))),
    }
    match cs8210_sys::mmio::Mmio::map(&dev.resource0(), MAP_LEN, true) {
        Ok(m) => (Backend::Device(m), Some(dev), None),
        Err(e) => {
            let why = format!("{}: {e}", dev.resource0().display());
            (Backend::None, Some(dev), Some(why))
        }
    }
}

fn bind(socket: &Path) -> Result<UnixListener, String> {
    if let Some(d) = socket.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let _ = std::fs::remove_file(socket);
    let l = UnixListener::bind(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660)).map_err(|e| e.to_string())?;
    Ok(l)
}

fn call(socket: &Path, req: &Value) -> Result<Value, String> {
    let s = UnixStream::connect(socket)
        .map_err(|e| format!("{}: {e} (is cs8210-optics.service running?)", socket.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let mut w = s.try_clone().map_err(|e| e.to_string())?;
    writeln!(w, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("bad reply: {e}"))?;
    if v["ok"] == true {
        Ok(v)
    } else {
        Err(v["error"].as_str().unwrap_or("error").to_string())
    }
}

fn hexdump(data: &[u8], base: usize) {
    for (i, row) in data.chunks(16).enumerate() {
        let hx: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
        let asc: String = row
            .iter()
            .map(|&b| if (0x20..0x7f).contains(&b) { char::from(b) } else { '.' })
            .collect();
        out!("{:04x}: {:<47}  {asc}", base + 16 * i, hx.join(" "));
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .filter_map(|i| s.get(i..i + 2).and_then(|d| u8::from_str_radix(d, 16).ok()))
        .collect()
}

fn f(v: &Value, d: usize) -> String {
    v.as_f64().map_or_else(|| "-".into(), |x| format!("{x:.d$}"))
}

fn s(v: &Value) -> String {
    v.as_str().map_or_else(|| "-".into(), str::to_string)
}

fn phase(c: &Value) -> String {
    let st = &c["bringup"]["state"];
    match st["phase"].as_str() {
        Some("ready") => format!(
            "ready{}",
            st["app"].as_u64().map(|a| format!(" app {a}")).unwrap_or_default()
        ),
        Some("failed") => format!("FAILED: {}", s(&st["reason"])),
        Some(p) => p.to_string(),
        None => "-".into(),
    }
}

fn status(socket: &Path, cage: Option<u8>) -> Result<(), String> {
    let v = call(socket, &json!({"op": "status", "cage": cage}))?;
    if let Some(d) = v["dead"].as_str() {
        out!("I2C FPGA UNAVAILABLE: {d}");
    } else if let Some(ver) = v["version"].as_u64() {
        out!(
            "I2C FPGA version {:#06x} board {:#04x}",
            (ver >> 8) & 0xffff,
            ver & 0xff
        );
    }
    out!(
        "cage pres rst lp int  decl mode    type     vendor           part             serial            temp C  vcc V  bring-up"
    );
    let empty = Vec::new();
    for c in v["cages"].as_array().unwrap_or(&empty) {
        let b = |k: &str| if c[k] == true { "yes" } else { "no" };
        let id = &c["identity"];
        let mon = &c["monitors"];
        out!(
            "{:>4} {:<4} {:<3} {:<3} {:<4} {:<4} {:<7} {:<8} {:<16} {:<16} {:<17} {:>6} {:>6}  {}",
            c["cage"].as_u64().unwrap_or(0),
            b("present"),
            b("reset"),
            b("lpmode"),
            b("interrupt"),
            b("managed"),
            s(&c["mode"]),
            s(&id["form_factor"]),
            s(&id["vendor"]),
            s(&id["part_number"]),
            s(&id["serial"]),
            f(&mon["temperature_c"], 1),
            f(&mon["voltage_v"], 3),
            phase(c)
        );
        if cage.is_some() {
            if let Some(apps) = id["applications"].as_array() {
                for a in apps {
                    out!(
                        "     application {}: host {:#04x} media {:#04x} {}x{} lanes",
                        a["sel"],
                        a["host_if"].as_u64().unwrap_or(0),
                        a["media_if"].as_u64().unwrap_or(0),
                        a["host_lanes"],
                        a["media_lanes"]
                    );
                }
            }
            out!(
                "     module state {}  data path {}  active ApSel {}",
                s(&c["module_state"]),
                c["datapath"],
                c["active_apsel"]
            );
            for l in mon["lanes"].as_array().unwrap_or(&empty) {
                let dbm = |k: &str| {
                    l[k].as_f64()
                        .filter(|x| *x > 0.0)
                        .map_or_else(|| "-inf".into(), |x| format!("{:.2}", 10.0 * x.log10()))
                };
                out!(
                    "     lane {}  tx {} mW ({} dBm)  rx {} mW ({} dBm)  bias {} mA  rx_los {}  tx_fault {}",
                    l["lane"],
                    f(&l["tx_power_mw"], 4),
                    dbm("tx_power_mw"),
                    f(&l["rx_power_mw"], 4),
                    dbm("rx_power_mw"),
                    f(&l["tx_bias_ma"], 2),
                    l["rx_los"],
                    l["tx_fault"]
                );
            }
            out!("     i2c errors {}  last error {}", c["i2c_errors"], c["last_error"]);
        }
    }
    Ok(())
}

fn dump(socket: &Path, cage: u8) -> Result<(), String> {
    let st = call(socket, &json!({"op": "status", "cage": cage}))?;
    let kind = &st["cages"][0]["kind"];
    let pages: Vec<u8> = match (kind["spec"].as_str(), kind["flat"].as_bool()) {
        (Some("cmis"), Some(false)) => vec![0x01, 0x02, 0x10, 0x11],
        (Some("sff8636"), Some(false)) => vec![0x01, 0x02, 0x03],
        _ => Vec::new(),
    };
    let r = call(socket, &json!({"op": "read", "cage": cage, "offset": 0, "len": 256}))?;
    out!("== cage {cage}: lower page + upper page 00h");
    hexdump(&unhex(r["data"].as_str().unwrap_or("")), 0);
    for p in pages {
        let r = call(
            socket,
            &json!({"op": "read", "cage": cage, "offset": 128 + 128 * usize::from(p), "len": 128}),
        )?;
        out!("== cage {cage}: upper page {p:02x}h (bytes 128-255)");
        hexdump(&unhex(r["data"].as_str().unwrap_or("")), 128);
    }
    Ok(())
}

fn guard(mut a: Vec<String>, socket: &Path) -> Result<(), String> {
    let allowed: Vec<u8> = last(take_opt(&mut a, "--allowed")?)
        .ok_or("--allowed LIST")?
        .split([',', ' '])
        .filter(|x| *x != "none")
        .filter(|x| !x.is_empty())
        .map(num8)
        .collect::<Result<_, _>>()?;
    let wait = last(take_opt(&mut a, "--wait")?).map_or(Ok(30), |w| num(&w))?;
    let mut tries = 0;
    let sb = loop {
        match call(socket, &json!({"op": "sideband"})) {
            Ok(v) => break v,
            Err(e) if tries < wait => {
                tries += 1;
                if tries == 1 {
                    eprintln!("cage guard: waiting for the optics daemon: {e}");
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(e) => return Err(format!("cage guard: {e}; not starting syncd")),
        }
    };
    if let Some(d) = sb["dead"].as_str() {
        return Err(format!("cage guard: I2C FPGA unavailable ({d}); not starting syncd"));
    }
    let present = sb["present"].as_u64().unwrap_or(0);
    let bad: Vec<String> = (1..=32u8)
        .filter(|c| present >> (c - 1) & 1 != 0 && !allowed.contains(c))
        .map(|c| c.to_string())
        .collect();
    if !bad.is_empty() {
        return Err(format!(
            "cage guard: modules in undeclared cages {}; declared: {allowed:?}; not starting syncd",
            bad.join(" ")
        ));
    }
    out!("cage guard: present mask {present:#010x}, every populated cage declared ({allowed:?})");
    Ok(())
}

/// The Credo loader's -m spec with each cage's line-side media taken from the module in it
/// ([`cs8210_optics::credo`]); waits up to --wait seconds for present modules to be identified.
fn credo_spec(mut a: Vec<String>, socket: &Path) -> Result<(), String> {
    let spec = last(take_opt(&mut a, "--spec")?).ok_or("--spec SPEC")?;
    let wait = last(take_opt(&mut a, "--wait")?).map_or(Ok(30), |w| num(&w))?;
    cs8210_optics::credo::rewrite(&spec, &std::collections::BTreeMap::new())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(wait);
    let media = loop {
        let v = call(socket, &json!({"op": "status"}));
        let (known, pending) = v.as_ref().map(cs8210_optics::credo::media).unwrap_or_default();
        if (v.is_ok() && pending.is_empty()) || std::time::Instant::now() > deadline {
            if !pending.is_empty() || v.is_err() {
                eprintln!("credo-spec: modules not identified in time (cages {pending:?}): their default kept");
            }
            break known;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let (out, notes) = cs8210_optics::credo::rewrite(&spec, &media)?;
    for n in notes {
        eprintln!("credo-spec: {n}");
    }
    out!("{out}");
    Ok(())
}

fn run(argv: Vec<String>) -> Result<(), String> {
    let mut v = argv;
    let socket = last(take_opt(&mut v, "--socket")?).map_or_else(|| PathBuf::from(SOCKET), PathBuf::from);
    let Args { rest, socket } = Args { rest: v, socket };
    let (cmd, a) = rest.split_first().ok_or(USAGE)?;
    let a: Vec<String> = a.to_vec();
    let arg = |i: usize| a.get(i).ok_or_else(|| USAGE.to_string());
    match cmd.as_str() {
        "daemon" => daemon(a, &socket),
        "status" => status(&socket, a.first().map(|x| num8(x)).transpose()?),
        "json" => {
            let v = call(
                &socket,
                &json!({"op": "status", "cage": a.first().map(|x| num8(x)).transpose()?}),
            )?;
            out!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            Ok(())
        }
        "sideband" => {
            let v = call(&socket, &json!({"op": "sideband"}))?;
            let m = |k: &str| v[k].as_u64().unwrap_or(0);
            out!(
                "present={:08x} reset={:08x} lpmode={:08x} int={:08x} dead={}",
                m("present"),
                m("reset"),
                m("lpmode"),
                m("interrupt"),
                v["dead"]
            );
            Ok(())
        }
        "read" => {
            let (c, off, len) = (num8(arg(0)?)?, num(arg(1)?)?, num(arg(2)?)?);
            let r = call(&socket, &json!({"op": "read", "cage": c, "offset": off, "len": len}))?;
            hexdump(
                &unhex(r["data"].as_str().unwrap_or("")),
                usize::try_from(off).unwrap_or(0),
            );
            Ok(())
        }
        "dump" => dump(&socket, num8(arg(0)?)?),
        "write" => {
            let (c, off) = (num8(arg(0)?)?, num(arg(1)?)?);
            let bytes: Vec<u8> = a
                .get(2..)
                .unwrap_or(&[])
                .iter()
                .map(|b| num8(b))
                .collect::<Result<_, _>>()?;
            let data = bytes.iter().fold(String::new(), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            });
            call(&socket, &json!({"op": "write", "cage": c, "offset": off, "data": data})).map(|_| ())
        }
        "lpmode" => call(
            &socket,
            &json!({"op": "lpmode", "cage": num8(arg(0)?)?, "on": num(arg(1)?)? != 0}),
        )
        .map(|_| ()),
        "reset" => call(&socket, &json!({"op": "reset", "cage": num8(arg(0)?)?})).map(|_| ()),
        "retry" => call(&socket, &json!({"op": "retry", "cage": num8(arg(0)?)?})).map(|_| ()),
        "guard" => guard(a, &socket),
        "credo-spec" => credo_spec(a, &socket),
        "credo-temps" => {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).map_err(|e| e.to_string())?;
            let prom = cs8210_optics::metrics::credo_temps(&text).ok_or("no \"slice N temp=\" line on stdin")?;
            out!("{}", prom.trim_end());
            Ok(())
        }
        "-h" | "--help" | "help" => {
            out!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

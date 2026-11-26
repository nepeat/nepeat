//! `cs8210-psu`: the PSUs' status (system CPLD) and `PMBus` readings: `monitor` (the service: state
//! files + Prometheus text every interval), `show` and `metrics` (one-shot).
#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cs8210_psu::{
    Bus, Identity, PMBUS_ADDR, Poll, SLOTS, Status, adapter, identity, metrics, poll, serial, state_json, status,
};
use cs8210_sys::i2cdev::I2cDev;

const USAGE: &str =
    "usage: cs8210-psu monitor [--interval S] [--metrics FILE (\"\" = none)] [--state DIR] | metrics | show";
const SYS_CPLD: u8 = 0x60;
const SYS_CPLD_NODE: &str = "i2c-mux@74/i2c@0/i2c-mux@71/i2c@2";
/// Serialises `PMBus` page use between the service and a one-shot `show` / `metrics`.
const LOCK: &str = "/run/lock/cs8210-psu.lock";

macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

/// One PSU's i2c-dev adapter, addressed at 0x59.
struct Pmbus(I2cDev);

impl Bus for Pmbus {
    fn read_byte(&mut self, cmd: u8) -> io::Result<u8> {
        self.0.read_byte_data(PMBUS_ADDR, cmd)
    }
    fn read_word(&mut self, cmd: u8) -> io::Result<u16> {
        self.0.read_word_data(PMBUS_ADDR, cmd)
    }
    fn read_block(&mut self, cmd: u8) -> io::Result<Vec<u8>> {
        self.0.read_block_data(PMBUS_ADDR, cmd)
    }
    fn write_byte(&mut self, cmd: u8, value: u8) -> io::Result<()> {
        self.0.write_byte_data(PMBUS_ADDR, cmd, value)
    }
}

fn reg08() -> Option<u8> {
    let dir = fs::read_dir("/sys/bus/i2c/devices").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if fs::canonicalize(p.join("of_node")).is_ok_and(|n| n.to_string_lossy().ends_with(SYS_CPLD_NODE)) {
            let dev = I2cDev::open(&Path::new("/dev").join(e.file_name())).ok()?;
            return (0..3).find_map(|i| {
                if i > 0 {
                    std::thread::sleep(cs8210_psu::RETRY_PAUSE);
                }
                dev.read_byte_data(SYS_CPLD, 0x08).ok()
            });
        }
    }
    None
}

fn lock() -> Option<File> {
    let f = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(LOCK)
        .ok()?;
    f.lock().ok()?;
    Some(f)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Write via a temporary file in the same directory and rename (readers never see half a file).
fn write_atomic(path: &Path, data: &str) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

fn flags(s: Status) -> String {
    format!(
        "present={} input_good={} output_good={}",
        u8::from(s.present),
        u8::from(s.input_good),
        u8::from(s.output_good)
    )
}

/// One bay: its adapter (opened once found), the last identity / status / `PMBus` state.
struct Slot {
    psu: u8,
    ch: u8,
    bus: Option<(u32, Pmbus)>,
    id: Option<Identity>,
    /// this poll's CPLD status
    status: Option<Status>,
    /// the last readable one (for change logs)
    seen: Option<Status>,
    up: Option<bool>,
    poll: Poll,
}

impl Slot {
    fn new(psu: u8, ch: u8) -> Self {
        Self {
            psu,
            ch,
            bus: None,
            id: None,
            status: None,
            seen: None,
            up: None,
            poll: Poll::default(),
        }
    }

    /// Find and open the adapter; with `evict`, delete a kernel client at 0x59 first (the old
    /// cs8210-psu-bind unit's `pmbus` device, which would hold the address).
    fn open(&mut self, evict: bool, log: &mut Vec<String>) {
        if self.bus.is_some() {
            return;
        }
        let Some((bus, dev)) = adapter(Path::new("/sys"), self.ch) else {
            return;
        };
        if evict && dev.exists() {
            let drv = fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "no driver".into());
            let del = format!("/sys/bus/i2c/devices/i2c-{bus}/delete_device");
            match fs::write(&del, format!("0x{PMBUS_ADDR:02x}")) {
                Ok(()) => log.push(format!(
                    "PSU {}: deleted kernel device {bus}-00{PMBUS_ADDR:02x} ({drv})",
                    self.psu
                )),
                Err(e) => log.push(format!("PSU {}: {del} ({drv}): {e}", self.psu)),
            }
        }
        match I2cDev::open(Path::new(&format!("/dev/i2c-{bus}"))) {
            Ok(d) => self.bus = Some((bus, Pmbus(d))),
            Err(e) => log.push(format!("PSU {}: /dev/i2c-{bus}: {e}", self.psu)),
        }
    }

    /// One poll: CPLD status, `PMBus` readings, identity when new; log lines for every change.
    fn refresh(&mut self, reg08: Option<u8>, evict: bool) -> Vec<String> {
        let mut log = Vec::new();
        let psu = self.psu;
        let st = reg08.map(|r| status(r, psu));
        match (self.status, st) {
            (Some(_), None) => log.push(format!("PSU {psu}: system CPLD unreadable")),
            (None, Some(s)) if self.seen.is_some() => {
                log.push(format!("PSU {psu}: system CPLD readable, {}", flags(s)));
            }
            _ => {}
        }
        match (self.seen, st) {
            (None, Some(s)) => log.push(format!("PSU {psu}: {}", flags(s))),
            (Some(a), Some(b)) => {
                for (name, x, y) in [
                    ("present", a.present, b.present),
                    ("input_good", a.input_good, b.input_good),
                    ("output_good", a.output_good, b.output_good),
                ] {
                    if x != y {
                        log.push(format!("PSU {psu}: {name} {} -> {}", u8::from(x), u8::from(y)));
                    }
                }
            }
            _ => {}
        }
        self.status = st;
        if st.is_some() {
            self.seen = st;
        }
        self.open(evict, &mut log);
        let absent = st.is_some_and(|s| !s.present);
        if absent {
            self.id = None;
        }
        let Some((bus, dev)) = self.bus.as_mut().filter(|_| !absent) else {
            self.poll = Poll::default();
            return log;
        };
        self.poll = poll(dev);
        // identity: until read, and again when the serial changes (a swapped PSU)
        let known = self.id.as_ref().map(|i| i.serial.clone());
        if self.poll.up && (known.is_none() || serial(dev).is_some_and(|s| Some(s) != known)) {
            let new = identity(dev);
            if let Some(i) = new.as_ref().filter(|&n| self.id.as_ref() != Some(n)) {
                log.push(format!(
                    "PSU {psu}: {} {} revision {} serial {} (PMBus revision 0x{:02x})",
                    i.vendor, i.model, i.revision, i.serial, i.pmbus_revision
                ));
            }
            if new.is_some() {
                self.id = new;
            }
        }
        if self.up != Some(self.poll.up) {
            let what = if self.poll.up { "readable" } else { "unreadable" };
            log.push(format!("PSU {psu}: PMBus on i2c-{bus} {what}"));
            self.up = Some(self.poll.up);
        }
        log
    }

    fn state(&self, updated: u64) -> Option<String> {
        let (bus, _) = self.bus.as_ref()?;
        Some(state_json(self.psu, *bus, self.id.as_ref(), self.status, &self.poll, updated).to_string())
    }
}

/// Poll every bay under the lock.
fn refresh(slots: &mut [Slot], evict: bool) -> (Option<u8>, Vec<String>) {
    let r = reg08();
    let _l = lock();
    let log = slots.iter_mut().flat_map(|s| s.refresh(r, evict)).collect();
    (r, log)
}

fn slots() -> Vec<Slot> {
    SLOTS.iter().map(|&(psu, ch)| Slot::new(psu, ch)).collect()
}

fn text(r: Option<u8>, slots: &[Slot]) -> String {
    metrics(r, &slots.iter().map(|s| (s.psu, &s.poll)).collect::<Vec<_>>())
}

struct Args {
    interval: Duration,
    metrics: Option<PathBuf>,
    state: PathBuf,
}

fn args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        interval: Duration::from_secs(15),
        metrics: Some("/run/prometheus-textfile/cs8210-psu.prom".into()),
        state: "/run/cs8210-psu".into(),
    };
    while let Some(k) = it.next() {
        let v = it.next().ok_or(USAGE)?;
        match k.as_str() {
            "--interval" => {
                let s: u64 = v.parse().map_err(|_| USAGE)?;
                a.interval = Duration::from_secs(s.max(1));
            }
            "--metrics" => a.metrics = (!v.is_empty()).then(|| v.into()),
            "--state" => a.state = v.into(),
            _ => return Err(USAGE.into()),
        }
    }
    Ok(a)
}

fn monitor(a: &Args) -> Result<(), String> {
    fs::create_dir_all(&a.state).map_err(|e| format!("{}: {e}", a.state.display()))?;
    let mut slots = slots();
    let mut write_err: Option<String> = None;
    loop {
        let t0 = Instant::now();
        let (r, log) = refresh(&mut slots, true);
        for l in log {
            eprintln!("{l}");
        }
        let t = now();
        let mut errs = Vec::new();
        for s in &slots {
            if let Some(j) = s.state(t) {
                let p = a.state.join(format!("psu{}.json", s.psu));
                if let Err(e) = write_atomic(&p, &j) {
                    errs.push(format!("{}: {e}", p.display()));
                }
            }
        }
        if let Some(m) = &a.metrics
            && let Err(e) = write_atomic(m, &text(r, &slots))
        {
            errs.push(format!("{}: {e}", m.display()));
        }
        let err = (!errs.is_empty()).then(|| errs.join("; "));
        if err != write_err {
            eprintln!("{}", err.as_deref().unwrap_or("state and metrics written again"));
            write_err = err;
        }
        std::thread::sleep(a.interval.saturating_sub(t0.elapsed()));
    }
}

fn show() {
    let mut slots = slots();
    let (_, log) = refresh(&mut slots, false);
    for l in log.iter().filter(|l| l.contains(": /dev/")) {
        eprintln!("{l}");
    }
    for s in &slots {
        let st = s.status.map_or_else(|| "CPLD unreadable".into(), flags);
        let bus = s
            .bus
            .as_ref()
            .map_or_else(|| "no adapter".into(), |(b, _)| format!("i2c-{b}"));
        out!(
            "PSU {} ({bus}): {st}; PMBus {}",
            s.psu,
            if s.poll.up { "up" } else { "down" }
        );
        if let Some(i) = &s.id {
            out!(
                "  {} {} revision {} serial {} (PMBus revision 0x{:02x})",
                i.vendor,
                i.model,
                i.revision,
                i.serial,
                i.pmbus_revision
            );
        }
        if let Some(w) = s.poll.status_word {
            out!("  STATUS_WORD 0x{w:04x}");
        }
        for (sn, v) in &s.poll.readings {
            out!("  {:<6} {v:>10.3} {}", sn.label, sn.kind.unit());
        }
        if let Some(e) = &s.poll.page_error {
            out!("  ({e})");
        }
    }
}

fn run() -> Result<(), String> {
    let mut it = std::env::args().skip(1);
    match it.next().as_deref() {
        Some("monitor") => monitor(&args(it)?),
        Some("metrics") => {
            let mut s = slots();
            let (r, _) = refresh(&mut s, false);
            if r.is_none() && s.iter().all(|s| !s.poll.up) {
                return Err("system CPLD unreadable and no PMBus answer".into());
            }
            out!("{}", text(r, &s).trim_end());
            Ok(())
        }
        Some("show") => {
            show();
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

//! The CS8210-32X power supplies (two Delta TDPS-1500AB-6 B bays on this 19-inch variant).
//!
//! - Status: system CPLD 0x60 reg 0x08: PSU1 present bit 4, PSU2 bit 5 (active low), input good
//!   bits 2 / 3, output good bits 0 / 1 (vendor `sys_cpld.ko`). Reg 0xa8 carries PSU 3/4 on the
//!   21-inch variant only (sys CPLD 0x09 bit 0 = 0); here it reads 0 and is not wired.
//! - `PMBus` at 0x59 on mux 0x72 (behind 0x70:2 -> 0x74:0): PSU1 on channel 1, PSU2 on channel 0
//!   (the vendor's 15-0059 / 14-0059). A PSU without AC still answers from the standby rail.
//!   Page 0 = the 12 V main output (`VOUT_MODE` 0x17: LINEAR16, exponent -9), page 1 = the
//!   3.3 V standby rail (0x19: -7).
//!
//! This crate is the one reader of that `PMBus`: [`poll`] over a [`Bus`] (i2c-dev in the
//! binary, a fake in the tests). No kernel driver: the generic `pmbus` driver probed PSU1
//! inconsistently (2026-11-26: VOUT exponent 0, so vout1 = 6176 V instead of 12.06, sometimes
//! only one page; fan2/fan3 = -1 rpm and vcap -0.5 V on both) while raw reads were 100/100
//! reliable and identical on both PSUs. Every transaction gets up to [`ATTEMPTS`] tries, a
//! value outside its [`Sensor`]'s sane range is dropped, and the only write is PAGE (0x00):
//! 1 for the standby reads, then always back to 0, each read back to verify (no
//! `CLEAR_FAULTS`, no OPERATION, nothing else).
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

/// `PMBus` address of the PSU controller.
pub const PMBUS_ADDR: u8 = 0x59;
/// mux 0x72 channel per PSU slot.
pub const SLOTS: [(u8, u8); 2] = [(1, 1), (2, 0)];
/// Tries per `PMBus` transaction.
pub const ATTEMPTS: u32 = 3;
/// Pause between two tries.
pub const RETRY_PAUSE: Duration = Duration::from_millis(2);

/// The `PMBus` commands used (all reads but PAGE).
pub mod cmd {
    /// PAGE (read / write byte): 0 = main output, 1 = standby rail.
    pub const PAGE: u8 = 0x00;
    /// `VOUT_MODE` (byte, per page): the LINEAR16 exponent.
    pub const VOUT_MODE: u8 = 0x20;
    /// `STATUS_WORD` (word).
    pub const STATUS_WORD: u8 = 0x79;
    /// `PMBUS_REVISION` (byte).
    pub const PMBUS_REVISION: u8 = 0x98;
    /// `MFR_ID` (block).
    pub const MFR_ID: u8 = 0x99;
    /// `MFR_MODEL` (block).
    pub const MFR_MODEL: u8 = 0x9a;
    /// `MFR_REVISION` (block).
    pub const MFR_REVISION: u8 = 0x9b;
    /// `MFR_SERIAL` (block).
    pub const MFR_SERIAL: u8 = 0x9e;
}

/// One PSU's `PMBus` device (the address is the implementation's).
pub trait Bus {
    /// `SMBus` read byte data.
    fn read_byte(&mut self, cmd: u8) -> io::Result<u8>;
    /// `SMBus` read word data.
    fn read_word(&mut self, cmd: u8) -> io::Result<u16>;
    /// `SMBus` block read.
    fn read_block(&mut self, cmd: u8) -> io::Result<Vec<u8>>;
    /// `SMBus` write byte data (only ever PAGE here).
    fn write_byte(&mut self, cmd: u8, value: u8) -> io::Result<()>;
    /// Wait between two tries of a transaction.
    fn pause(&mut self) {
        std::thread::sleep(RETRY_PAUSE);
    }
}

/// Status bits of one PSU from the system CPLD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// a PSU is in the bay
    pub present: bool,
    /// its AC input is good
    pub input_good: bool,
    /// its output is good
    pub output_good: bool,
}

/// Decode reg 0x08 for PSU 1 or 2.
#[must_use]
pub fn status(reg08: u8, psu: u8) -> Status {
    let (p, i, o) = if psu == 1 { (4, 2, 0) } else { (5, 3, 1) };
    Status {
        present: reg08 >> p & 1 == 0,
        input_good: reg08 >> i & 1 == 1,
        output_good: reg08 >> o & 1 == 1,
    }
}

/// The adapter of mux 0x72's channel `ch` (`sys` = /sys normally): its number and the sysfs
/// path a kernel client at 0x59 would have.
#[must_use]
pub fn adapter(sys: &Path, ch: u8) -> Option<(u32, PathBuf)> {
    let devs = sys.join("bus/i2c/devices");
    for e in fs::read_dir(&devs).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.ends_with("-0072") {
            continue;
        }
        let link = fs::read_link(e.path().join(format!("channel-{ch}"))).ok()?;
        let bus = link.file_name()?.to_string_lossy().strip_prefix("i2c-")?.parse().ok()?;
        return Some((bus, devs.join(format!("{bus}-00{PMBUS_ADDR:02x}"))));
    }
    None
}

/// `PMBus` LINEAR11 to a float.
#[must_use]
pub fn linear11(raw: u16) -> f64 {
    let exp = i32::from(raw.cast_signed() >> 11);
    let mant = i32::from(((raw & 0x7ff) << 5).cast_signed() >> 5);
    f64::from(mant) * 2f64.powi(exp)
}

/// The LINEAR16 exponent of a `VOUT_MODE` byte: bits 4:0, signed; None unless the mode
/// (bits 7:5) is 000 = linear.
#[must_use]
pub fn vout_exponent(vout_mode: u8) -> Option<i32> {
    if vout_mode >> 5 != 0 {
        return None;
    }
    let e = i32::from(vout_mode & 0x1f);
    Some(if e >= 16 { e - 32 } else { e })
}

/// `PMBus` LINEAR16 (unsigned mantissa, exponent from `VOUT_MODE`) to a float.
#[must_use]
pub fn linear16(raw: u16, vout_mode: u8) -> Option<f64> {
    vout_exponent(vout_mode).map(|e| f64::from(raw) * 2f64.powi(e))
}

/// What a reading measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// V
    Volts,
    /// A
    Amperes,
    /// W
    Watts,
    /// degrees C
    Celsius,
    /// fan speed
    Rpm,
}

impl Kind {
    /// The unit for humans.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::Volts => "V",
            Self::Amperes => "A",
            Self::Watts => "W",
            Self::Celsius => "C",
            Self::Rpm => "rpm",
        }
    }
}

/// How a reading is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// LINEAR11 (exponent in the word)
    Linear11,
    /// LINEAR16 (exponent from the page's `VOUT_MODE`)
    Linear16,
}

/// One `PMBus` reading.
#[derive(Debug, PartialEq)]
pub struct Sensor {
    /// key in the state file's "readings"
    pub key: &'static str,
    /// the metrics' sensor label (the hwmon labels the kernel driver used)
    pub label: &'static str,
    /// what it measures
    pub kind: Kind,
    /// PAGE it is read on
    pub page: u8,
    /// the read word command
    pub cmd: u8,
    /// its encoding
    pub format: Format,
    /// lowest sane value (inclusive)
    pub min: f64,
    /// highest sane value (inclusive)
    pub max: f64,
}

impl Sensor {
    /// The value is within the sane range (else it is dropped).
    #[must_use]
    pub fn sane(&self, v: f64) -> bool {
        v.is_finite() && v >= self.min && v <= self.max
    }
}

const fn sensor(
    key: &'static str,
    label: &'static str,
    kind: Kind,
    page: u8,
    cmd: u8,
    format: Format,
    (min, max): (f64, f64),
) -> Sensor {
    Sensor {
        key,
        label,
        kind,
        page,
        cmd,
        format,
        min,
        max,
    }
}

/// Everything read per poll, in output order. Ranges: a 1500 W PSU, 90-264 V AC in, 12 V and
/// 3.3 V out.
pub static SENSORS: [Sensor; 13] = {
    use Format::{Linear11 as L11, Linear16 as L16};
    use Kind::{Amperes, Celsius, Rpm, Volts, Watts};
    [
        sensor("vin", "vin", Volts, 0, 0x88, L11, (0.0, 300.0)),
        sensor("iin", "iin", Amperes, 0, 0x89, L11, (0.0, 30.0)),
        sensor("pin", "pin", Watts, 0, 0x97, L11, (0.0, 3000.0)),
        sensor("vout", "vout1", Volts, 0, 0x8b, L16, (0.0, 20.0)),
        sensor("iout", "iout1", Amperes, 0, 0x8c, L11, (0.0, 250.0)),
        sensor("pout", "pout1", Watts, 0, 0x96, L11, (0.0, 3000.0)),
        sensor("temp1", "temp1", Celsius, 0, 0x8d, L11, (-40.0, 150.0)),
        sensor("temp2", "temp2", Celsius, 0, 0x8e, L11, (-40.0, 150.0)),
        sensor("temp3", "temp3", Celsius, 0, 0x8f, L11, (-40.0, 150.0)),
        sensor("fan1", "fan1", Rpm, 0, 0x90, L11, (0.0, 50000.0)),
        sensor("vout_sb", "vout2", Volts, 1, 0x8b, L16, (0.0, 6.0)),
        sensor("iout_sb", "iout2", Amperes, 1, 0x8c, L11, (0.0, 20.0)),
        sensor("pout_sb", "pout2", Watts, 1, 0x96, L11, (0.0, 100.0)),
    ]
};

/// Run a transaction up to [`ATTEMPTS`] times, pausing between tries.
pub fn retry<B: Bus + ?Sized, T>(bus: &mut B, mut f: impl FnMut(&mut B) -> io::Result<T>) -> io::Result<T> {
    let mut n = 1;
    loop {
        match f(bus) {
            Ok(v) => return Ok(v),
            Err(e) if n >= ATTEMPTS => return Err(e),
            Err(_) => {
                n += 1;
                bus.pause();
            }
        }
    }
}

/// Write PAGE and read it back.
pub fn set_page<B: Bus + ?Sized>(bus: &mut B, page: u8) -> io::Result<()> {
    retry(bus, |b| b.write_byte(cmd::PAGE, page))?;
    let got = retry(bus, |b| b.read_byte(cmd::PAGE))?;
    if got == page {
        Ok(())
    } else {
        Err(io::Error::other(format!("PAGE reads back {got} after writing {page}")))
    }
}

/// One PSU's readings of one poll.
#[derive(Debug, Default, PartialEq)]
pub struct Poll {
    /// the `PMBus` answered (PAGE readable)
    pub up: bool,
    /// the sane readings, in [`SENSORS`] order
    pub readings: Vec<(&'static Sensor, f64)>,
    /// `STATUS_WORD`
    pub status_word: Option<u16>,
    /// selecting page 1 or restoring page 0 failed (the standby readings may be missing)
    pub page_error: Option<String>,
}

impl Poll {
    /// A reading by state-file key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<f64> {
        self.readings.iter().find(|(s, _)| s.key == key).map(|&(_, v)| v)
    }
}

fn read_page<B: Bus + ?Sized>(bus: &mut B, page: u8, out: &mut Vec<(&'static Sensor, f64)>) {
    let mode = retry(bus, |b| b.read_byte(cmd::VOUT_MODE)).ok();
    for s in SENSORS.iter().filter(|s| s.page == page) {
        let Ok(raw) = retry(bus, |b| b.read_word(s.cmd)) else {
            continue;
        };
        let v = match s.format {
            Format::Linear11 => Some(linear11(raw)),
            Format::Linear16 => mode.and_then(|m| linear16(raw, m)),
        };
        if let Some(v) = v.filter(|&v| s.sane(v)) {
            out.push((s, v));
        }
    }
}

/// Read one PSU: page 0 (PAGE set to 0 first if something left it elsewhere), `STATUS_WORD`,
/// page 1, and PAGE back to 0 whatever happened on page 1.
pub fn poll<B: Bus + ?Sized>(bus: &mut B) -> Poll {
    let mut p = Poll::default();
    let Ok(page) = retry(bus, |b| b.read_byte(cmd::PAGE)) else {
        return p;
    };
    p.up = true;
    if page != 0
        && let Err(e) = set_page(bus, 0)
    {
        p.page_error = Some(format!("page 0: {e}"));
        return p;
    }
    read_page(bus, 0, &mut p.readings);
    p.status_word = retry(bus, |b| b.read_word(cmd::STATUS_WORD)).ok();
    let standby = set_page(bus, 1).map(|()| read_page(bus, 1, &mut p.readings));
    let back = set_page(bus, 0);
    if let Err(e) = standby {
        p.page_error = Some(format!("page 1: {e}"));
    }
    if let Err(e) = back {
        p.page_error = Some(format!("PAGE not restored to 0: {e}"));
    }
    p
}

/// A PSU's identity (MFR_* blocks).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    /// `PMBUS_REVISION`
    pub pmbus_revision: u8,
    /// `MFR_ID`
    pub vendor: String,
    /// `MFR_MODEL`
    pub model: String,
    /// `MFR_REVISION`
    pub revision: String,
    /// `MFR_SERIAL`
    pub serial: String,
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_string()
}

fn block<B: Bus + ?Sized>(bus: &mut B, c: u8) -> Option<String> {
    retry(bus, |b| b.read_block(c)).ok().map(|b| text(&b))
}

/// `MFR_SERIAL` alone (to notice a swapped PSU).
pub fn serial<B: Bus + ?Sized>(bus: &mut B) -> Option<String> {
    block(bus, cmd::MFR_SERIAL)
}

/// The identity; None if `PMBUS_REVISION` does not answer (a missing block reads as "").
pub fn identity<B: Bus + ?Sized>(bus: &mut B) -> Option<Identity> {
    let pmbus_revision = retry(bus, |b| b.read_byte(cmd::PMBUS_REVISION)).ok()?;
    Some(Identity {
        pmbus_revision,
        vendor: block(bus, cmd::MFR_ID).unwrap_or_default(),
        model: block(bus, cmd::MFR_MODEL).unwrap_or_default(),
        revision: block(bus, cmd::MFR_REVISION).unwrap_or_default(),
        serial: block(bus, cmd::MFR_SERIAL).unwrap_or_default(),
    })
}

/// The state file /run/cs8210-psu/psuN.json (what `sonic_platform` reads): identity, CPLD status,
/// the readings (absent = not read), `STATUS_WORD`, the poll's unix time.
#[must_use]
pub fn state_json(psu: u8, bus: u32, id: Option<&Identity>, st: Option<Status>, p: &Poll, updated: u64) -> Value {
    let mut o = Map::new();
    o.insert("psu".into(), psu.into());
    o.insert("bus".into(), bus.into());
    if let Some(id) = id {
        o.insert("vendor".into(), id.vendor.clone().into());
        o.insert("model".into(), id.model.clone().into());
        o.insert("revision".into(), id.revision.clone().into());
        o.insert("serial".into(), id.serial.clone().into());
        o.insert("pmbus_revision".into(), id.pmbus_revision.into());
    }
    if let Some(s) = st {
        o.insert(
            "status".into(),
            json!({"present": s.present, "input_good": s.input_good, "output_good": s.output_good}),
        );
    }
    let r: Map<String, Value> = p.readings.iter().map(|(s, v)| (s.key.to_string(), json!(v))).collect();
    o.insert("readings".into(), r.into());
    if let Some(w) = p.status_word {
        o.insert("status_word".into(), w.into());
    }
    o.insert("updated".into(), updated.into());
    Value::Object(o)
}

/// Prometheus text for the PSUs: the CPLD bits (if reg 0x08 was readable), `PMBus` up, the
/// readings, `STATUS_WORD`.
#[must_use]
pub fn metrics(reg08: Option<u8>, polls: &[(u8, &Poll)]) -> String {
    let mut s = String::new();
    let fam = |s: &mut String, name: &str, help: &str| {
        let _ = writeln!(s, "# HELP {name} {help}\n# TYPE {name} gauge");
    };
    if let Some(r) = reg08 {
        for (name, help, f) in [
            ("switch_psu_present", "1 if a PSU is in the bay (system CPLD).", 0usize),
            (
                "switch_psu_input_good",
                "1 if the PSU's AC input is good (system CPLD).",
                1,
            ),
            (
                "switch_psu_output_good",
                "1 if the PSU's output is good (system CPLD).",
                2,
            ),
        ] {
            fam(&mut s, name, help);
            for (psu, _) in SLOTS {
                let st = status(r, psu);
                let v = match f {
                    0 => st.present,
                    1 => st.input_good,
                    _ => st.output_good,
                };
                let _ = writeln!(s, "{name}{{psu=\"{psu}\"}} {}", u8::from(v));
            }
        }
    }
    if !polls.is_empty() {
        fam(
            &mut s,
            "switch_psu_pmbus_up",
            "1 if the PSU answered on PMBus this poll.",
        );
        for (psu, p) in polls {
            let _ = writeln!(s, "switch_psu_pmbus_up{{psu=\"{psu}\"}} {}", u8::from(p.up));
        }
    }
    for (kind, name, help) in [
        (
            Kind::Volts,
            "switch_psu_voltage_volts",
            "PSU PMBus voltage reading (sensor = vin / vout1 / vout2 = standby).",
        ),
        (
            Kind::Amperes,
            "switch_psu_current_amperes",
            "PSU PMBus current reading.",
        ),
        (
            Kind::Watts,
            "switch_psu_power_watts",
            "PSU PMBus power reading (pin = from the wall, pout1 = delivered, pout2 = standby).",
        ),
        (
            Kind::Celsius,
            "switch_psu_temperature_celsius",
            "PSU PMBus temperature.",
        ),
        (Kind::Rpm, "switch_psu_fan_rpm", "PSU fan speed."),
    ] {
        let rows: Vec<String> = polls
            .iter()
            .flat_map(|(psu, p)| {
                p.readings
                    .iter()
                    .filter(|(sn, _)| sn.kind == kind)
                    .map(move |(sn, v)| format!("{name}{{psu=\"{psu}\",sensor=\"{}\"}} {v}", sn.label))
            })
            .collect();
        if !rows.is_empty() {
            fam(&mut s, name, help);
            for r in rows {
                let _ = writeln!(s, "{r}");
            }
        }
    }
    let words: Vec<(u8, u16)> = polls
        .iter()
        .filter_map(|(psu, p)| p.status_word.map(|w| (*psu, w)))
        .collect();
    if !words.is_empty() {
        fam(
            &mut s,
            "switch_psu_status_word",
            "PSU PMBus STATUS_WORD (0 = no fault or warning).",
        );
        for (psu, w) in words {
            let _ = writeln!(s, "switch_psu_status_word{{psu=\"{psu}\"}} {w}");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// LINEAR11 encode (tests): mantissa = v / 2^exp.
    fn l11(v: f64, exp: i32) -> u16 {
        #[allow(clippy::cast_possible_truncation)]
        let m = (v / 2f64.powi(exp)).round() as i32;
        (((exp & 0x1f) << 11) | (m & 0x7ff)).try_into().unwrap()
    }

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-6)
    }

    /// A PSU on a fake bus: paged registers, per-register failures, a write log.
    #[derive(Default)]
    struct Fake {
        page: u8,
        regs: HashMap<(u8, u8), u16>,
        blocks: HashMap<u8, Vec<u8>>,
        /// reads of (page, cmd) still to fail
        fail: HashMap<(u8, u8), u32>,
        /// every read fails (bay empty)
        dead: bool,
        /// a write of PAGE = this value fails
        refuse_page: Option<u8>,
        /// PAGE writes are acknowledged but ignored
        stuck: bool,
        writes: Vec<(u8, u8)>,
        tries: HashMap<(u8, u8), u32>,
        pauses: u32,
    }

    impl Fake {
        /// The 2026-11-26 PSU2: fed, ~350 W, the raw words from i2cget.
        fn psu() -> Self {
            let mut f = Self::default();
            for (page, cmd, v) in [
                (0, cmd::VOUT_MODE, 0x17),
                (0, 0x88, 0xebaa),         // 117.25 V
                (0, 0x89, l11(3.0, -6)),   // 3 A
                (0, 0x97, 0x02b4),         // 692 W
                (0, 0x8b, 0x1820),         // 12.0625 V
                (0, 0x8c, l11(28.5, -4)),  // 28.5 A
                (0, 0x96, l11(344.0, 0)),  // 344 W
                (0, 0x8d, l11(37.0, -2)),  // 37 C
                (0, 0x8e, l11(41.5, -1)),  // 41.5 C
                (0, 0x8f, l11(-5.0, 0)),   // -5 C
                (0, 0x90, l11(8000.0, 3)), // 8000 rpm
                (0, cmd::STATUS_WORD, 0x0000),
                (1, cmd::VOUT_MODE, 0x19),
                (1, 0x8b, 422), // 3.296_875 V
                (1, 0x8c, l11(0.5, -4)),
                (1, 0x96, l11(1.75, -2)),
            ] {
                f.regs.insert((page, cmd), v);
            }
            f.blocks.insert(cmd::MFR_ID, b"DELTA".to_vec());
            f.blocks.insert(cmd::MFR_MODEL, b"TDPS-1500AB-6 B\0".to_vec());
            f.blocks.insert(cmd::MFR_REVISION, b"S1F ".to_vec());
            f.blocks.insert(cmd::MFR_SERIAL, b"DSTD1234567".to_vec());
            f.regs.insert((0, cmd::PMBUS_REVISION), 0x22);
            f
        }

        fn read(&mut self, cmd: u8) -> io::Result<u16> {
            let k = (if cmd == cmd::PAGE || cmd >= 0x98 { 0 } else { self.page }, cmd);
            *self.tries.entry(k).or_default() += 1;
            if self.dead {
                return Err(io::Error::other("nack"));
            }
            if let Some(n) = self.fail.get_mut(&k).filter(|n| **n > 0) {
                *n -= 1;
                return Err(io::Error::other("nack"));
            }
            if cmd == cmd::PAGE {
                return Ok(self.page.into());
            }
            self.regs
                .get(&k)
                .copied()
                .ok_or_else(|| io::Error::other("unsupported"))
        }
    }

    impl Bus for Fake {
        fn read_byte(&mut self, cmd: u8) -> io::Result<u8> {
            self.read(cmd).map(|v| (v & 0xff).try_into().unwrap())
        }
        fn read_word(&mut self, cmd: u8) -> io::Result<u16> {
            self.read(cmd)
        }
        fn read_block(&mut self, cmd: u8) -> io::Result<Vec<u8>> {
            if self.dead {
                return Err(io::Error::other("nack"));
            }
            self.blocks
                .get(&cmd)
                .cloned()
                .ok_or_else(|| io::Error::other("unsupported"))
        }
        fn write_byte(&mut self, cmd: u8, value: u8) -> io::Result<()> {
            self.writes.push((cmd, value));
            if self.dead || (cmd == cmd::PAGE && self.refuse_page == Some(value)) {
                return Err(io::Error::other("nack"));
            }
            if cmd == cmd::PAGE && !self.stuck {
                self.page = value;
            }
            Ok(())
        }
        fn pause(&mut self) {
            self.pauses += 1;
        }
    }

    #[test]
    fn cpld_status() {
        // 2026-11-26: PSU2 fed (input + output good), PSU1 without AC, both present
        assert_eq!(
            status(0x4a, 2),
            Status {
                present: true,
                input_good: true,
                output_good: true
            }
        );
        assert_eq!(
            status(0x4a, 1),
            Status {
                present: true,
                input_good: false,
                output_good: false
            }
        );
        assert!(status(0x45, 1).output_good);
        assert!(!status(0x65, 2).present);
    }

    #[test]
    fn decoders() {
        assert!((linear11(0xebaa) - 117.25).abs() < 1e-9);
        assert!((linear11(0x02b4) - 692.0).abs() < 1e-9);
        assert!((linear11(l11(-5.0, 0)) + 5.0).abs() < 1e-9);
        assert!((linear11(0x07ff) + 1.0).abs() < 1e-9); // the -1 rpm "fan2"
        assert_eq!(vout_exponent(0x17), Some(-9));
        assert_eq!(vout_exponent(0x19), Some(-7));
        assert_eq!(vout_exponent(0x0f), Some(15));
        assert_eq!(vout_exponent(0x00), Some(0));
        assert!(close(linear16(0x1820, 0x17), 12.0625));
        assert!(close(linear16(422, 0x19), 3.296_875));
        assert_eq!(linear16(0x1820, 0x40), None); // VID
        assert_eq!(linear16(0x1820, 0x57), None); // direct
        assert_eq!(linear16(0x1820, 0x97), None);
    }

    #[test]
    fn poll_both_pages() {
        let mut f = Fake::psu();
        let p = poll(&mut f);
        assert!(p.up);
        assert_eq!(p.page_error, None);
        for (k, v) in [
            ("vin", 117.25),
            ("iin", 3.0),
            ("pin", 692.0),
            ("vout", 12.0625),
            ("iout", 28.5),
            ("pout", 344.0),
            ("temp1", 37.0),
            ("temp2", 41.5),
            ("temp3", -5.0),
            ("fan1", 8000.0),
            ("vout_sb", 3.296_875),
            ("iout_sb", 0.5),
            ("pout_sb", 1.75),
        ] {
            assert!(close(p.get(k), v), "{k}: {:?}", p.get(k));
        }
        assert_eq!(p.readings.len(), SENSORS.len());
        assert_eq!(p.status_word, Some(0));
        // the only writes: PAGE 1, then back to 0
        assert_eq!(f.writes, [(cmd::PAGE, 1), (cmd::PAGE, 0)]);
        assert_eq!(f.page, 0);
        assert_eq!(f.pauses, 0);
    }

    #[test]
    fn page_left_at_1_is_restored_first() {
        let mut f = Fake::psu();
        f.page = 1;
        let p = poll(&mut f);
        assert!(close(p.get("vout"), 12.0625));
        assert_eq!(f.writes, [(cmd::PAGE, 0), (cmd::PAGE, 1), (cmd::PAGE, 0)]);
        assert_eq!(f.page, 0);
    }

    #[test]
    fn page_restored_on_page1_errors() {
        // PAGE = 1 refused: no standby readings, PAGE written back to 0 anyway
        let mut f = Fake::psu();
        f.refuse_page = Some(1);
        let p = poll(&mut f);
        assert!(close(p.get("vout"), 12.0625));
        assert_eq!(p.get("vout_sb"), None);
        assert!(p.page_error.as_deref().is_some_and(|e| e.starts_with("page 1")));
        assert_eq!(f.writes.last(), Some(&(cmd::PAGE, 0)));
        assert_eq!(f.page, 0);
        // PAGE acknowledged but not taken: the readback catches it (no page-0 values as standby)
        let mut f = Fake::psu();
        f.stuck = true;
        let p = poll(&mut f);
        assert_eq!(p.get("vout_sb"), None);
        assert!(p.page_error.as_deref().is_some_and(|e| e.contains("reads back 0")));
        assert_eq!(f.writes, [(cmd::PAGE, 1), (cmd::PAGE, 0)]);
        // every page-1 read failing: still back to 0
        let mut f = Fake::psu();
        for c in [cmd::VOUT_MODE, 0x8b, 0x8c, 0x96] {
            f.fail.insert((1, c), 99);
        }
        let p = poll(&mut f);
        assert_eq!(p.get("iout_sb"), None);
        assert_eq!(p.page_error, None);
        assert_eq!(f.writes, [(cmd::PAGE, 1), (cmd::PAGE, 0)]);
        assert_eq!(f.page, 0);
    }

    #[test]
    fn retries() {
        // two failures then an answer: read on the third try
        let mut f = Fake::psu();
        f.fail.insert((0, 0x88), 2);
        let p = poll(&mut f);
        assert!(close(p.get("vin"), 117.25));
        assert_eq!(f.tries[&(0, 0x88)], 3);
        assert_eq!(f.pauses, 2);
        // three failures: dropped after three tries
        let mut f = Fake::psu();
        f.fail.insert((0, 0x88), 3);
        let p = poll(&mut f);
        assert_eq!(p.get("vin"), None);
        assert!(close(p.get("iin"), 3.0));
        assert_eq!(f.tries[&(0, 0x88)], ATTEMPTS);
        // nothing answers: down, no readings, no write
        let mut f = Fake::psu();
        f.dead = true;
        let p = poll(&mut f);
        assert_eq!(p, Poll::default());
        assert_eq!(f.writes, []);
        assert_eq!(f.tries[&(0, cmd::PAGE)], ATTEMPTS);
        assert_eq!(identity(&mut f), None);
    }

    #[test]
    fn sanity_drop() {
        let mut f = Fake::psu();
        f.regs.insert((0, cmd::VOUT_MODE), 0x00); // the kernel driver's exponent 0: 6176 V
        f.regs.insert((0, 0x90), 0x07ff); // -1 rpm
        f.regs.insert((0, 0x8d), l11(200.0, 0));
        f.regs.insert((0, 0x88), l11(-0.5, -1));
        f.regs.insert((1, cmd::VOUT_MODE), 0x57); // not linear: no standby vout
        let p = poll(&mut f);
        for k in ["vout", "fan1", "temp1", "vin", "vout_sb"] {
            assert_eq!(p.get(k), None, "{k}");
        }
        assert!(close(p.get("iout"), 28.5));
        assert!(close(p.get("iout_sb"), 0.5));
    }

    #[test]
    fn identity_and_json() {
        let mut f = Fake::psu();
        let id = identity(&mut f).unwrap();
        assert_eq!(
            id,
            Identity {
                pmbus_revision: 0x22,
                vendor: "DELTA".into(),
                model: "TDPS-1500AB-6 B".into(),
                revision: "S1F".into(),
                serial: "DSTD1234567".into(),
            }
        );
        assert_eq!(serial(&mut f).as_deref(), Some("DSTD1234567"));
        assert_eq!(f.writes, []);
        f.regs.remove(&(0, 0x90));
        let p = poll(&mut f);
        let j = state_json(2, 14, Some(&id), Some(status(0x4a, 2)), &p, 1_790_000_000);
        assert_eq!(j["psu"], 2);
        assert_eq!(j["bus"], 14);
        assert_eq!(j["model"], "TDPS-1500AB-6 B");
        assert_eq!(j["pmbus_revision"], 0x22);
        assert_eq!(
            j["status"],
            json!({"present": true, "input_good": true, "output_good": true})
        );
        assert_eq!(j["readings"]["vout"], 12.0625);
        assert_eq!(j["readings"]["vout_sb"], 3.296_875);
        assert!(j["readings"].get("fan1").is_none()); // not read: omitted
        assert_eq!(j["readings"].as_object().unwrap().len(), SENSORS.len() - 1);
        assert_eq!(j["status_word"], 0);
        assert_eq!(j["updated"], 1_790_000_000u64);
        // nothing known: no identity, no status, empty readings
        let j = state_json(1, 15, None, None, &Poll::default(), 5);
        let keys: Vec<&str> = j.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["bus", "psu", "readings", "updated"]);
    }

    #[test]
    fn metrics_text() {
        let p2 = poll(&mut Fake::psu());
        let p1 = Poll::default();
        let m = metrics(Some(0x4a), &[(1, &p1), (2, &p2)]);
        for line in [
            "switch_psu_present{psu=\"1\"} 1",
            "switch_psu_input_good{psu=\"1\"} 0",
            "switch_psu_output_good{psu=\"2\"} 1",
            "switch_psu_pmbus_up{psu=\"1\"} 0",
            "switch_psu_pmbus_up{psu=\"2\"} 1",
            "switch_psu_voltage_volts{psu=\"2\",sensor=\"vin\"} 117.25",
            "switch_psu_voltage_volts{psu=\"2\",sensor=\"vout1\"} 12.0625",
            "switch_psu_voltage_volts{psu=\"2\",sensor=\"vout2\"} 3.296875",
            "switch_psu_current_amperes{psu=\"2\",sensor=\"iout1\"} 28.5",
            "switch_psu_power_watts{psu=\"2\",sensor=\"pin\"} 692",
            "switch_psu_power_watts{psu=\"2\",sensor=\"pout2\"} 1.75",
            "switch_psu_temperature_celsius{psu=\"2\",sensor=\"temp3\"} -5",
            "switch_psu_fan_rpm{psu=\"2\",sensor=\"fan1\"} 8000",
            "switch_psu_status_word{psu=\"2\"} 0",
            "# TYPE switch_psu_status_word gauge",
        ] {
            assert!(m.lines().any(|l| l == line), "missing {line}\n{m}");
        }
        assert!(!m.contains("psu=\"1\",sensor"));
        assert!(!m.contains("switch_psu_status_word{psu=\"1\"}"));
        // every family announced once
        assert_eq!(m.matches("# TYPE switch_psu_voltage_volts gauge").count(), 1);
        // CPLD unreadable: no status families, PMBus ones still there
        let m = metrics(None, &[(2, &p2)]);
        assert!(!m.contains("switch_psu_present"));
        assert!(m.contains("switch_psu_pmbus_up{psu=\"2\"} 1"));
    }
}

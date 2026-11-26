//! The board layer's text files (`ifcs_config.{h,cpp}`): the port map (`SAI_TERALYNX_PORT_MAP`), the
//! init command files (`SAI_TERALYNX_INIT_CMDS`), and the run-time config (`SAI_INIT_CONFIG_FILE` +
//! `SAI_TERALYNX_CONFIG_APPEND` merged into `SAI_TERALYNX_RUN_DIR/config.yaml`, `tl_switch.cpp`).
//! Pure parsing and file writing: no IFCS call. Messages are the C++'s, byte for byte.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ifcs::sys;

/// One front-panel devport: `devport lanes speed_mbps fec`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortSpec {
    /// 1..=1023.
    pub devport: u32,
    /// `serdes_group * 8 + lane_in_group + 1`, 1..=8 of them.
    pub lanes: Vec<u32>,
    /// Mb/s.
    pub speed: u32,
    /// `none` / `rs` / `fc`.
    pub fec: String,
}

/// `trim` of `ifcs_config`.cpp: " \t\r\n" off both ends.
fn trim(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\r', '\n'])
}

/// `to_int`: decimal only (Python's `int()` of the vendor CLI setters), the whole text, in range
/// of `long long`.
fn to_int(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    // strtoll: leading white space and a sign are accepted, then digits to the end
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<i64>().ok()
}

/// Lines of a file as `std::getline` gives them (split on \n; a final line without one counts).
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n').filter(move |_| !text.is_empty())
}

/// `parse_port_map`: every spec, or `"<file>:<line>: <why>"`.
pub fn parse_port_map(path: &str) -> Result<Vec<PortSpec>, String> {
    let Ok(text) = std::fs::read(path) else {
        return Err(format!("{path}: cannot open"));
    };
    let text = String::from_utf8_lossy(&text);
    let mut out = Vec::new();
    let (mut seen_dp, mut seen_lane) = (BTreeSet::new(), BTreeSet::new());
    for (i, raw) in lines(&text).enumerate() {
        let n = i + 1;
        let w = format!("{path}:{n}: ");
        let line = trim(raw.split('#').next().unwrap_or_default());
        if line.is_empty() {
            continue;
        }
        let tok: Vec<&str> = line.split_whitespace().collect();
        let [dp, lanes, speed, fec] = tok.as_slice() else {
            return Err(format!("{w}expected \"devport lanes speed_mbps fec\""));
        };
        let devport = to_int(dp)
            .filter(|v| (1..=1023).contains(v))
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("{w}bad devport '{dp}'"))?;
        if !seen_dp.insert(devport) {
            return Err(format!("{w}devport {dp} listed twice"));
        }
        let mut lane_list = Vec::new();
        // std::getline(ls, l, ','): empty fields count, a trailing ',' does not make one
        let fields = lanes.strip_suffix(',').unwrap_or(lanes);
        for l in fields.split(',').filter(|_| !lanes.is_empty()) {
            let v = to_int(l)
                .filter(|v| (1..=256).contains(v))
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| format!("{w}bad lane '{l}' (1..256)"))?;
            if !seen_lane.insert(v) {
                return Err(format!("{w}lane {l} used twice"));
            }
            lane_list.push(v);
        }
        if lane_list.is_empty() || lane_list.len() > 8 {
            return Err(format!("{w}1..8 lanes expected"));
        }
        let speed_v = to_int(speed)
            .filter(|v| *v > 0 && *v <= 400_000)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("{w}bad speed '{speed}'"))?;
        if !matches!(*fec, "none" | "rs" | "fc") {
            return Err(format!("{w}fec must be none|rs|fc"));
        }
        out.push(PortSpec {
            devport,
            lanes: lane_list,
            speed: speed_v,
            fec: (*fec).to_owned(),
        });
    }
    if out.is_empty() {
        return Err(format!("{path}: no ports"));
    }
    Ok(out)
}

/// How an init command's value is converted (the vendor CLI's setter for the attribute).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValKind {
    /// `value.u32` (`enum` / `data` / `u32` setters: `int()`).
    U32,
    /// `value.i8_list`.
    I8List,
    /// `value.u8_list`.
    U8List,
    /// `value.u32_list`.
    U32List,
}

/// `ifcs set devport <id> <attr> <value|[v,v,...]>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevportCmd {
    /// The devport.
    pub devport: u32,
    /// `IFCS_DEVPORT_ATTR_*`.
    pub attr: u32,
    /// The attribute's name as written.
    pub name: String,
    /// Its conversion.
    pub kind: ValKind,
    /// One value for `U32`.
    pub values: Vec<i64>,
    /// `file:line`.
    pub at: String,
}

/// The parsed init command files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitCmds {
    /// The commands applied, in file order.
    pub cmds: Vec<DevportCmd>,
    /// Skipped lines per kind (`port enable`, `pci write`, ...), in the C++ map's (byte) order.
    pub skipped: BTreeMap<String, u32>,
}

/// The attribute table: what the vendor's `SDK/cel_cmds` files use (`misc_config_32.cmd`,
/// `dynamic_misc.cmd`, `dynamic_preemphasis.cmd`, `preemphasis_*.cmd`).
const ATTRS: [(&str, u32, ValKind); 14] = [
    ("rx_eq_fine_tune", sys::IFCS_DEVPORT_ATTR_RX_EQ_FINE_TUNE, ValKind::U32),
    ("link_training", sys::IFCS_DEVPORT_ATTR_LINK_TRAINING, ValKind::U32),
    ("tx_eq_pre1", sys::IFCS_DEVPORT_ATTR_TX_EQ_PRE1, ValKind::I8List),
    ("tx_eq_pre2", sys::IFCS_DEVPORT_ATTR_TX_EQ_PRE2, ValKind::I8List),
    ("tx_eq_pre3", sys::IFCS_DEVPORT_ATTR_TX_EQ_PRE3, ValKind::I8List),
    ("tx_eq_attn", sys::IFCS_DEVPORT_ATTR_TX_EQ_ATTN, ValKind::I8List),
    ("tx_eq_post", sys::IFCS_DEVPORT_ATTR_TX_EQ_POST, ValKind::I8List),
    ("rx_ctle_lf", sys::IFCS_DEVPORT_ATTR_RX_CTLE_LF, ValKind::U8List),
    ("rx_ctle_hf", sys::IFCS_DEVPORT_ATTR_RX_CTLE_HF, ValKind::U8List),
    ("rx_ctle_bw", sys::IFCS_DEVPORT_ATTR_RX_CTLE_BW, ValKind::U8List),
    ("rx_gainshape1", sys::IFCS_DEVPORT_ATTR_RX_GAINSHAPE1, ValKind::U32List),
    ("rx_gainshape2", sys::IFCS_DEVPORT_ATTR_RX_GAINSHAPE2, ValKind::U32List),
    ("rx_agc_target", sys::IFCS_DEVPORT_ATTR_RX_AGC_TARGET, ValKind::U32List),
    (
        "rx_eye_disqualify_threshold_25g",
        sys::IFCS_DEVPORT_ATTR_RX_EYE_DISQUALIFY_THRESHOLD_25G,
        ValKind::U32List,
    ),
];

/// `devport_attr_lookup`.
pub fn devport_attr_lookup(name: &str) -> Option<(u32, ValKind)> {
    ATTRS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|&(_, id, kind)| (id, kind))
}

/// `parse_init_cmds`: appends `path`'s commands to `out`; an unreadable file or a malformed line
/// of a known attribute is the error.
pub fn parse_init_cmds(path: &str, out: &mut InitCmds) -> Result<(), String> {
    let Ok(text) = std::fs::read(path) else {
        return Err(format!("{path}: cannot open"));
    };
    let text = String::from_utf8_lossy(&text);
    for (i, raw) in lines(&text).enumerate() {
        let w = format!("{path}:{}", i + 1);
        let line = trim(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tok: Vec<&str> = line.split_whitespace().collect();
        let ours = tok.len() >= 3
            && tok.first() == Some(&"ifcs")
            && tok.get(1) == Some(&"set")
            && tok.get(2) == Some(&"devport");
        if !ours {
            let mut kind = (*tok.first().unwrap_or(&"")).to_owned();
            if let Some(t1) = tok.get(1) {
                kind = format!("{kind} {t1}");
            }
            if tok.len() > 2 && tok.first() == Some(&"ifcs") {
                kind = format!("{kind} {}", tok.get(2).unwrap_or(&""));
            }
            *out.skipped.entry(kind).or_insert(0) += 1;
            continue;
        }
        let (Some(id), Some(name)) = (tok.get(3), tok.get(4)) else {
            return Err(format!("{w}: expected \"ifcs set devport <id> <attr> <value>\""));
        };
        let Some((attr, kind)) = devport_attr_lookup(name) else {
            *out.skipped.entry(format!("ifcs set devport {name}")).or_insert(0) += 1;
            continue;
        };
        let devport = to_int(id)
            .filter(|v| (0..=1023).contains(v))
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("{w}: bad devport '{id}'"))?;
        // the value may contain spaces inside the brackets: rejoin the rest
        let val: String = tok.get(5..).unwrap_or_default().concat();
        if val.is_empty() {
            return Err(format!("{w}: {name}: missing value"));
        }
        let mut values = Vec::new();
        if kind == ValKind::U32 {
            let v = to_int(&val)
                .filter(|v| (0..=0xffff_ffff).contains(v))
                .ok_or_else(|| format!("{w}: {name}: bad value '{val}'"))?;
            values.push(v);
        } else {
            let Some(inner) = val
                .strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .filter(|_| val.len() >= 2)
            else {
                return Err(format!("{w}: {name}: list value [a,b,...] expected"));
            };
            let (lo, hi) = match kind {
                ValKind::I8List => (-128, 127),
                ValKind::U8List => (0, 255),
                _ => (0, 0xffff_ffff),
            };
            let fields = inner.strip_suffix(',').unwrap_or(inner);
            for e in fields.split(',').filter(|_| !inner.is_empty()) {
                let v = to_int(e)
                    .filter(|v| (lo..=hi).contains(v))
                    .ok_or_else(|| format!("{w}: {name}: bad list element '{e}'"))?;
                values.push(v);
            }
            if values.is_empty() || values.len() > 8 {
                return Err(format!("{w}: {name}: 1..8 list elements expected"));
            }
        }
        out.cmds.push(DevportCmd {
            devport,
            attr,
            name: (*name).to_owned(),
            kind,
            values,
            at: w,
        });
    }
    Ok(())
}

/// One write of the post-init register list: `new = (old & !mask) | value` at BAR0 `offset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegWrite {
    /// BAR0 byte offset, 4-byte aligned.
    pub offset: u32,
    /// The bits written (`0xffffffff`: the whole register).
    pub mask: u32,
    /// Their value (no bit outside `mask`).
    pub value: u32,
    /// `file:line`.
    pub at: String,
}

/// A number of the register list: `0x` hex or decimal, the whole word.
fn reg_number(s: &str) -> Option<u32> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return (!h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| u32::from_str_radix(h, 16).ok())
            .flatten();
    }
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// The most entries of a post-init register list (`SAI_TERALYNX_POST_INIT_REGS`).
pub const MAX_POST_INIT_REGS: usize = 4096;
/// The most registers a diag watch list may name, ranges expanded (`SAI_TERALYNX_DIAG_REGS`).
pub const MAX_DIAG_REGS: usize = 1024;

/// `SAI_TERALYNX_POST_INIT_REGS`: one `offset mask value` per line, in order (`#` comments, blank
/// lines), any number up to [`MAX_POST_INIT_REGS`], applied in file order (the same offset may come
/// more than once: each entry is its own read-modify-write). The list has no meaning to the
/// backend: board data (an LED stream setup, per-PIC LED overrides from 0x13700, ...). An offset
/// not 4-byte aligned, a value with bits outside its mask, a zero mask, a malformed line or too
/// many entries is the error (`"<file>:<line>: <why>"`): the switch is refused before the SDK starts.
pub fn parse_regs(path: &str) -> Result<Vec<RegWrite>, String> {
    let Ok(text) = std::fs::read(path) else {
        return Err(format!("{path}: cannot open"));
    };
    let text = String::from_utf8_lossy(&text);
    let mut out = Vec::new();
    for (i, raw) in lines(&text).enumerate() {
        let at = format!("{path}:{}", i + 1);
        let line = trim(raw.split('#').next().unwrap_or_default());
        if line.is_empty() {
            continue;
        }
        let tok: Vec<&str> = line.split_whitespace().collect();
        let [o, m, v] = tok.as_slice() else {
            return Err(format!("{at}: expected \"offset mask value\""));
        };
        let (Some(offset), Some(mask), Some(value)) = (reg_number(o), reg_number(m), reg_number(v)) else {
            return Err(format!("{at}: not a number (0x hex or decimal, 32 bits)"));
        };
        if offset % 4 != 0 {
            return Err(format!("{at}: offset {offset:#x} is not 4-byte aligned"));
        }
        if mask == 0 || value & !mask != 0 {
            return Err(format!(
                "{at}: value {value:#x} has bits outside mask {mask:#x} (or the mask is 0)"
            ));
        }
        if out.len() == MAX_POST_INIT_REGS {
            return Err(format!("{at}: more than {MAX_POST_INIT_REGS} entries"));
        }
        out.push(RegWrite {
            offset,
            mask,
            value,
            at,
        });
    }
    Ok(out)
}

/// `SAI_TERALYNX_DIAG_REGS`: the registers the diag shell's `regs` may read, read only: besides the
/// post-init list the only BAR offsets it ever reads (an MMIO read nothing answers can stall the
/// `SoC`, so nothing from the diag socket names an offset). One `offset` or an inclusive range
/// `first-last` (every 4 bytes) per line, `0x` hex or decimal, `#` comments; 4-byte aligned; at most
/// [`MAX_DIAG_REGS`] registers in all; read in file order.
pub fn parse_diag_regs(path: &str) -> Result<Vec<u32>, String> {
    let Ok(text) = std::fs::read(path) else {
        return Err(format!("{path}: cannot open"));
    };
    let text = String::from_utf8_lossy(&text);
    let mut out: Vec<u32> = Vec::new();
    for (i, raw) in lines(&text).enumerate() {
        let at = format!("{path}:{}", i + 1);
        let line = trim(raw.split('#').next().unwrap_or_default());
        if line.is_empty() {
            continue;
        }
        let (first, last) = match line.split_once('-') {
            Some((a, b)) => (reg_number(trim(a)), reg_number(trim(b))),
            None if !line.contains(char::is_whitespace) => (reg_number(line), reg_number(line)),
            None => (None, None),
        };
        let (Some(first), Some(last)) = (first, last) else {
            return Err(format!(
                "{at}: expected \"offset\" or \"first-last\" (0x hex or decimal, 32 bits)"
            ));
        };
        if first % 4 != 0 || last % 4 != 0 || first > last {
            return Err(format!(
                "{at}: {first:#x}-{last:#x}: offsets 4-byte aligned, first <= last"
            ));
        }
        let n = usize::try_from((last - first) / 4)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        if out.len().saturating_add(n) > MAX_DIAG_REGS {
            return Err(format!("{at}: more than {MAX_DIAG_REGS} registers in all"));
        }
        out.extend((0..n).filter_map(|k| u32::try_from(k).ok().and_then(|k| first.checked_add(k * 4))));
    }
    Ok(out)
}

/// `write_atomic`: `path.tmp` written, then renamed over `path`; the error is the C++'s text.
pub fn write_atomic(path: &str, data: &[u8]) -> Result<(), String> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, data).map_err(|e| format!("cannot write {tmp}: {}", strerror(&e)))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {tmp} -> {path}: {}", strerror(&e)))
}

/// `strerror(errno)`'s text for an I/O error (Rust's message without its " (os error N)").
fn strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) => s.get(..i).unwrap_or(&s).to_owned(),
        None => s,
    }
}

/// The run-time config: the init yaml, then (if set) the append file after a newline when the
/// first does not end in one (`tl_switch.cpp`). Errors are the C++ log lines.
pub fn merged_config(config: &str, append: &str) -> Result<Vec<u8>, String> {
    let mut yaml = std::fs::read(config).map_err(|_| format!("cannot read {config}"))?;
    if !append.is_empty() {
        let extra = std::fs::read(append)
            .map_err(|_| format!("SAI_TERALYNX_CONFIG_APPEND {append} unreadable; refusing to start"))?;
        if yaml.last().is_some_and(|&c| c != b'\n') {
            yaml.push(b'\n');
        }
        yaml.extend_from_slice(&extra);
    }
    Ok(yaml)
}

/// `access(path, R_OK) == 0`: readable by this process (a directory counts, as in C).
pub fn readable(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_dir() {
        return std::fs::read_dir(p).is_ok();
    }
    std::fs::File::open(p).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join(format!("tl-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_str().unwrap().to_owned()
    }

    #[test]
    fn port_map_as_ifcs_config_cpp() {
        let p = tmp(
            "ports.map",
            "# devport lanes speed fec\n1 249,250,251,252,253,254,255,256 400000 rs\n2 241,242,243,244,245,246,247,248 400000 rs   # cage 2\n3 121 10000 none\n",
        );
        let specs = parse_port_map(&p).unwrap();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].lanes, (249..=256).collect::<Vec<_>>());
        assert_eq!(
            specs[2],
            PortSpec {
                devport: 3,
                lanes: vec![121],
                speed: 10000,
                fec: "none".into()
            }
        );
        let bad = [
            ("1 2 3", "expected \"devport lanes speed_mbps fec\""),
            ("1 2 3 rs x", "expected \"devport lanes speed_mbps fec\""),
            ("0 1 100 rs", "bad devport '0'"),
            ("x 1 100 rs", "bad devport 'x'"),
            ("1 1 100 rs\n1 2 100 rs", "devport 1 listed twice"),
            ("1 0 100 rs", "bad lane '0' (1..256)"),
            ("1 1,,2 100 rs", "bad lane '' (1..256)"),
            ("1 1,1 100 rs", "lane 1 used twice"),
            ("1 1,2,3,4,5,6,7,8,9 100 rs", "1..8 lanes expected"),
            ("1 1 0 rs", "bad speed '0'"),
            ("1 1 400001 rs", "bad speed '400001'"),
            ("1 1 100 kr", "fec must be none|rs|fc"),
        ];
        for (i, (text, why)) in bad.iter().enumerate() {
            let p = tmp(&format!("bad{i}.map"), text);
            let e = parse_port_map(&p).unwrap_err();
            assert!(e.ends_with(why) && e.starts_with(&p), "{text}: {e}");
        }
        let p = tmp("empty.map", "# nothing\n\n");
        assert_eq!(parse_port_map(&p).unwrap_err(), format!("{p}: no ports"));
        assert_eq!(parse_port_map("/nonexistent").unwrap_err(), "/nonexistent: cannot open");
    }

    #[test]
    fn init_cmds_as_ifcs_config_cpp() {
        let a = tmp(
            "a.cmd",
            "node create 0\nifcs set devport 1 rx_eq_fine_tune 1\nifcs set devport 1 tx_eq_pre1 [2,2,2,2,2,2,2,2]\r\nifcs set devport 2 rx_ctle_lf [1, 2, 3, 4]\npci write led_cfg sck_rate_f 0x8\npci write led_refresh_cliff 0x020\n",
        );
        let b = tmp(
            "b.cmd",
            "ifcs set devport 3 rx_gainshape1 [5]\nifcs set devport 1 link_training 1\nifcs set devport 1 bogus_attr 3\nport enable 1\n# comment\n",
        );
        let mut ic = InitCmds::default();
        parse_init_cmds(&a, &mut ic).unwrap();
        parse_init_cmds(&b, &mut ic).unwrap();
        let got: Vec<(u32, u32, Vec<i64>)> = ic.cmds.iter().map(|c| (c.devport, c.attr, c.values.clone())).collect();
        assert_eq!(
            got,
            vec![
                (1, 49, vec![1]),
                (1, 43, vec![2; 8]),
                (2, 50, vec![1, 2, 3, 4]),
                (3, 53, vec![5]),
                (1, 21, vec![1])
            ]
        );
        assert_eq!(ic.cmds[1].at, format!("{a}:3"));
        let skipped: Vec<(&str, u32)> = ic.skipped.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(
            skipped,
            vec![
                ("ifcs set devport bogus_attr", 1),
                ("node create", 1),
                ("pci write", 2),
                ("port enable", 1)
            ]
        );
        let bad = [
            (
                "ifcs set devport 1",
                "expected \"ifcs set devport <id> <attr> <value>\"",
            ),
            ("ifcs set devport x rx_eq_fine_tune 1", "bad devport 'x'"),
            ("ifcs set devport 1024 rx_eq_fine_tune 1", "bad devport '1024'"),
            ("ifcs set devport 1 rx_eq_fine_tune", "rx_eq_fine_tune: missing value"),
            (
                "ifcs set devport 1 rx_eq_fine_tune 0x1",
                "rx_eq_fine_tune: bad value '0x1'",
            ),
            (
                "ifcs set devport 1 tx_eq_pre1 2",
                "tx_eq_pre1: list value [a,b,...] expected",
            ),
            (
                "ifcs set devport 1 tx_eq_pre1 [200]",
                "tx_eq_pre1: bad list element '200'",
            ),
            (
                "ifcs set devport 1 rx_ctle_lf [-1]",
                "rx_ctle_lf: bad list element '-1'",
            ),
            (
                "ifcs set devport 1 rx_ctle_lf []",
                "rx_ctle_lf: 1..8 list elements expected",
            ),
            (
                "ifcs set devport 1 rx_ctle_lf [1,2,3,4,5,6,7,8,9]",
                "rx_ctle_lf: 1..8 list elements expected",
            ),
        ];
        for (i, (text, why)) in bad.iter().enumerate() {
            let p = tmp(&format!("bad{i}.cmd"), text);
            let e = parse_init_cmds(&p, &mut InitCmds::default()).unwrap_err();
            assert_eq!(e, format!("{p}:1: {why}"));
        }
    }

    #[test]
    fn post_init_registers() {
        let p = tmp(
            "regs.txt",
            "# LED stream\n0x13514 0x80f 0x808   # LED_CFG\n0x13538 0xffffffff 0x18eec2\n\n79152 2 2\n",
        );
        let r = parse_regs(&p).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].offset, r[0].mask, r[0].value), (0x13514, 0x80f, 0x808));
        assert_eq!(r[2].offset, 79152);
        assert_eq!(r[2].at, format!("{p}:5"));
        let bad = [
            ("0x10 0xff", "expected \"offset mask value\""),
            ("0x10 0xff 0x1 0x2", "expected \"offset mask value\""),
            ("0x10 0xff 0xg", "not a number (0x hex or decimal, 32 bits)"),
            ("0x100000000 0xff 1", "not a number (0x hex or decimal, 32 bits)"),
            ("-4 0xff 1", "not a number (0x hex or decimal, 32 bits)"),
            ("0x11 0xff 1", "offset 0x11 is not 4-byte aligned"),
            (
                "0x10 0x0f 0x10",
                "value 0x10 has bits outside mask 0xf (or the mask is 0)",
            ),
            ("0x10 0 0", "value 0x0 has bits outside mask 0x0 (or the mask is 0)"),
        ];
        for (i, (text, why)) in bad.iter().enumerate() {
            let p = tmp(&format!("badregs{i}.txt"), text);
            assert_eq!(parse_regs(&p).unwrap_err(), format!("{p}:1: {why}"));
        }
        assert_eq!(parse_regs(&tmp("empty-regs.txt", "# none\n")).unwrap(), vec![]);
        assert_eq!(parse_regs("/nonexistent").unwrap_err(), "/nonexistent: cannot open");
    }

    #[test]
    fn register_list_limits() {
        use std::fmt::Write as _;
        // any number of ordered entries, e.g. per-PIC overrides from 0x13700, the same offset again
        let mut many = String::new();
        for k in 0..MAX_POST_INIT_REGS {
            let _ = writeln!(many, "{:#x} 0xff 0x{:x}", 0x13700 + 4 * (k % 64), k % 256);
        }
        let r = parse_regs(&tmp("many-regs.txt", &many)).unwrap();
        assert_eq!(r.len(), MAX_POST_INIT_REGS);
        assert_eq!((r[64].offset, r[64].value), (0x13700, 64));
        let p = tmp("too-many-regs.txt", &format!("{many}0x13700 1 1\n"));
        assert_eq!(
            parse_regs(&p).unwrap_err(),
            format!("{p}:{}: more than 4096 entries", MAX_POST_INIT_REGS + 1)
        );
    }

    #[test]
    fn diag_watch_list() {
        let p = tmp(
            "watch.txt",
            "# LED_CFG block\n0x13500-0x1353c\n0x13700 # SWCTRL IB0\n80128 - 80136\n",
        );
        let w = parse_diag_regs(&p).unwrap();
        assert_eq!(w.len(), 16 + 1 + 3);
        assert_eq!((w[0], w[15], w[16], w[19]), (0x13500, 0x1353c, 0x13700, 80136));
        let bad = [
            ("0x13501", "0x13501-0x13501: offsets 4-byte aligned, first <= last"),
            (
                "0x13540-0x13500",
                "0x13540-0x13500: offsets 4-byte aligned, first <= last",
            ),
            (
                "0x10 0x20",
                "expected \"offset\" or \"first-last\" (0x hex or decimal, 32 bits)",
            ),
            (
                "x-0x20",
                "expected \"offset\" or \"first-last\" (0x hex or decimal, 32 bits)",
            ),
            ("0x0-0x1000", "more than 1024 registers in all"),
        ];
        for (i, (text, why)) in bad.iter().enumerate() {
            let p = tmp(&format!("badwatch{i}.txt"), text);
            assert_eq!(parse_diag_regs(&p).unwrap_err(), format!("{p}:1: {why}"));
        }
        assert_eq!(
            parse_diag_regs(&tmp("full-watch.txt", "0x0-0xffc\n")).unwrap().len(),
            1024
        );
        assert_eq!(
            parse_diag_regs("/nonexistent").unwrap_err(),
            "/nonexistent: cannot open"
        );
    }

    #[test]
    fn config_merge_and_atomic_write() {
        let init = tmp("init.yaml", "a: 1");
        let isg = tmp("isg.yaml", "  isg:\n");
        assert_eq!(merged_config(&init, &isg).unwrap(), b"a: 1\n  isg:\n");
        assert_eq!(merged_config(&init, "").unwrap(), b"a: 1");
        assert_eq!(
            merged_config("/nonexistent", "").unwrap_err(),
            "cannot read /nonexistent"
        );
        assert_eq!(
            merged_config(&init, "/nonexistent").unwrap_err(),
            "SAI_TERALYNX_CONFIG_APPEND /nonexistent unreadable; refusing to start"
        );
        let out = format!("{init}.out");
        write_atomic(&out, b"x").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"x");
        assert!(!Path::new(&format!("{out}.tmp")).exists());
        assert_eq!(
            write_atomic("/nonexistent/dir/config.yaml", b"x").unwrap_err(),
            "cannot write /nonexistent/dir/config.yaml.tmp: No such file or directory"
        );
        assert!(readable(&init) && !readable("/nonexistent"));
    }
}

//! The runner: `sai_script.cpp` step for step (its ops, name rules, outputs, markers, error texts and
//! exit codes), over `sai-client`. Where it deliberately differs (fixed name gaps), the default mode
//! does so and says so on stderr (`sai-script: FIX <tag>: ...`); `--cpp-names` keeps the C++'s resolution.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt as _;
use std::time::Instant;

use sai_client::{AclAction, AclData, AclField, Attr, Entry, EntryOp, Notify, Prefix, QosMapEntry, Sai, Value, sys};
use sai_meta::{AclKind, ValueType};

use crate::json::Json;
use crate::names::{self as n, lookup, lookup_last};
use crate::text;

/// How names are resolved.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// `--cpp-names`: exactly the C++'s name resolution (its gaps included).
    pub cpp_names: bool,
    /// `--tool NAME=CMD`: an exec step's `$ORACLE/bin/NAME` runs as CMD (the frame tools' names).
    pub tools: Vec<(String, String)>,
}

/// A step's failure: `Fail` (the C++'s own exception) or any other (`std::exception`: nlohmann's, `stoul`).
#[derive(Debug)]
pub enum Error {
    /// `struct Fail`
    Fail(String),
    /// another exception
    Other(String),
}

impl Error {
    /// The text after "script error: ".
    pub fn text(&self) -> &str {
        match self {
            Self::Fail(s) | Self::Other(s) => s,
        }
    }
}

type R<T> = Result<T, Error>;

fn js<T>(r: Result<T, String>) -> R<T> {
    r.map_err(Error::Other)
}

fn fail<T>(s: impl Into<String>) -> R<T> {
    Err(Error::Fail(s.into()))
}

/// A line on stdout, flushed (the C++ `printf` + `fflush`).
pub fn say(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.write_all(b"\n");
    let _ = o.flush();
}

fn warn(s: &str) {
    let _ = writeln!(std::io::stderr(), "{s}");
}

/// `%-<w>s` (bytes, as printf).
pub fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.len())))
}

/// An object type's statistics enum (the fix of a counter name the C++ tables lack).
fn stat_enum(t: sys::sai_object_type_t) -> &'static str {
    match t {
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => "sai_router_interface_stat_t",
        sys::SAI_OBJECT_TYPE_QUEUE => "sai_queue_stat_t",
        sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => "sai_ingress_priority_group_stat_t",
        sys::SAI_OBJECT_TYPE_BUFFER_POOL => "sai_buffer_pool_stat_t",
        _ => "sai_port_stat_t",
    }
}

/// The enum an attribute's value (or list element, ACL data) is an enumerator of.
fn enum_of(t: sys::sai_object_type_t, id: u32) -> Option<&'static str> {
    let m = sai_meta::attribute(t, id)?;
    let ty = match m.value_type {
        ValueType::Enum => Some(m.type_name),
        ValueType::I32List | ValueType::AclField(AclKind::Enum) | ValueType::AclAction(AclKind::Enum) => {
            m.type_name.split_whitespace().last()
        }
        _ => None,
    }?;
    sai_driver::names::enum_type(ty)
}

/// The script runner's state (`g_obj`, `g_lists`, `g_switch`, `g_failed`, the marker file).
pub struct Runner<'a> {
    sai: &'a Sai,
    profile: &'a BTreeMap<String, String>,
    opt: Options,
    obj: HashMap<String, sys::sai_object_id_t>,
    lists: HashMap<String, Vec<sys::sai_object_id_t>>,
    /// fixed mode: the type a `create ... as` asked for, by name
    declared: HashMap<String, sys::sai_object_type_t>,
    switch: sys::sai_object_id_t,
    /// failed steps
    pub failed: i32,
    mark: Option<File>,
    clock: Instant,
}

/// A step's `obj` and its type.
struct Target {
    oid: sys::sai_object_id_t,
    t: sys::sai_object_type_t,
    /// the type was the declared one (fixed mode): dispatch by it
    by_declared: bool,
}

impl<'a> Runner<'a> {
    /// A runner; `mark`: `IFCS_TRACE_FILE` opened for appending.
    pub fn new(sai: &'a Sai, profile: &'a BTreeMap<String, String>, opt: Options, mark: Option<File>) -> Self {
        Self {
            sai,
            profile,
            opt,
            obj: HashMap::new(),
            lists: HashMap::new(),
            declared: HashMap::new(),
            switch: 0,
            failed: 0,
            mark,
            clock: Instant::now(),
        }
    }

    /// `mark(s)`: "M <s>" into the trace file.
    pub fn mark(&mut self, s: &str) {
        if let Some(f) = &mut self.mark {
            let _ = f.write_all(format!("M {s}\n").as_bytes());
        }
    }

    fn now_ms(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    fn fixed(&self) -> bool {
        !self.opt.cpp_names
    }

    fn fix(tag: &str, what: &str) {
        warn(&format!("sai-script: FIX {tag}: {what}"));
    }

    // ------------------------------------------------------------ names and values

    /// `num(j)`.
    fn num(j: &Json) -> R<i64> {
        if j.is_integer() {
            return js(j.i64());
        }
        if let Json::Bool(b) = j {
            return Ok(i64::from(*b));
        }
        let s = js(j.str())?;
        if let Some(v) = lookup(n::K_VALUES, s).or_else(|| lookup(n::K_VALUES_QOS, s)) {
            return Ok(v);
        }
        text::stoll(s).map_or_else(|| fail(format!("unknown value {s}")), Ok)
    }

    /// A value that is an enumerator of `ty` (the attribute's): fixed mode resolves a name by that
    /// enum first (the C++'s flat table may give another enum's value, or none).
    fn num_in(&self, j: &Json, ty: Option<&str>, tag: &str, what: &str) -> R<i64> {
        if let (true, Some(ty), Json::Str(s)) = (self.fixed(), ty, j) {
            let full = format!("{}{s}", sai_driver::names::enum_prefix(ty));
            if let Some(v) = sai_driver::names::enum_value(ty, &full) {
                let flat = Self::num(j);
                if !matches!(flat, Ok(f) if f == v) {
                    let was = flat.map_or_else(|e| format!("C++: {}", e.text()), |f| format!("C++ table: {f}"));
                    Self::fix(tag, &format!("{what} value {s} = {full} ({v}; {was})"));
                }
                return Ok(v);
            }
        }
        Self::num(j)
    }

    /// `oid(j)`.
    fn oid(&self, j: &Json) -> R<sys::sai_object_id_t> {
        if j.is_integer() {
            return js(j.u64());
        }
        let s = js(j.str())?;
        if s == "switch" {
            return Ok(self.switch);
        }
        if s == "null" {
            return Ok(0);
        }
        let s = s.strip_prefix('$').unwrap_or(s);
        if let Some(b) = s.find('[') {
            let name = s.get(..b).unwrap_or_default();
            let i = text::stoul(s.get(b + 1..).unwrap_or_default()).ok_or_else(|| Error::Other("stoul".to_owned()))?;
            return usize::try_from(i)
                .ok()
                .and_then(|i| self.lists.get(name).and_then(|l| l.get(i)).copied())
                .map_or_else(|| fail(format!("no list element {s}")), Ok);
        }
        self.obj
            .get(s)
            .copied()
            .map_or_else(|| fail(format!("unknown object {s}")), Ok)
    }

    fn type_of(s: &str) -> R<sys::sai_object_type_t> {
        lookup(n::K_TYPES, s)
            .or_else(|| lookup(n::K_TYPES_QOS, s))
            .and_then(|v| u32::try_from(v).ok())
            .map_or_else(|| fail(format!("unknown object type {s}")), Ok)
    }

    fn mac(s: &str) -> R<[u8; 6]> {
        text::mac(s).map_or_else(|| fail(format!("bad mac {s}")), Ok)
    }

    fn ip(s: &str) -> R<std::net::IpAddr> {
        let a = if s.contains(':') {
            text::ip6(s).map(std::net::IpAddr::from)
        } else {
            text::ip4(s).map(std::net::IpAddr::from)
        };
        a.map_or_else(|| fail(format!("bad ip {s}")), Ok)
    }

    #[allow(
        clippy::many_single_char_names,
        reason = "s: the text, a: its address, p: the prefix (as the C++)"
    )]
    fn prefix(s: &str) -> R<Prefix> {
        let (a, len) = match s.find('/') {
            Some(sl) => (
                s.get(..sl).unwrap_or_default(),
                Some(text::stoi(s.get(sl + 1..).unwrap_or_default()).ok_or_else(|| Error::Other("stoi".to_owned()))?),
            ),
            None => (s, None),
        };
        let ip = Self::ip(a)?;
        let mut p = Prefix {
            v6: ip.is_ipv6(),
            addr: [0; 16],
            mask: [0; 16],
        };
        match ip {
            std::net::IpAddr::V4(v) => {
                let len = len.unwrap_or(32);
                p.addr[..4].copy_from_slice(&v.octets());
                let m = match len {
                    0 => 0,
                    1..=32 => u32::MAX << (32 - len),
                    _ => u32::MAX,
                };
                p.mask[..4].copy_from_slice(&m.to_be_bytes());
            }
            std::net::IpAddr::V6(v) => {
                let len = len.unwrap_or(128);
                p.addr = v.octets();
                for (i, m) in p.mask.iter_mut().enumerate() {
                    let bits = len - i32::try_from(i * 8).unwrap_or(i32::MAX);
                    *m = if bits >= 8 {
                        0xff
                    } else if bits <= 0 {
                        0
                    } else {
                        0xffu8 << (8 - bits)
                    };
                }
            }
        }
        Ok(p)
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "the C++ casts")]
    fn acl_data(&self, v: &Json, field: bool, ty: Option<&str>, tag: &str, what: &str) -> R<(AclData, AclData)> {
        let mask_or = |all: i64| -> R<i64> {
            if v.contains("mask") {
                Self::num(v.get_key("mask"))
            } else {
                Ok(all)
            }
        };
        let str_or = |d: &str| -> R<String> {
            Ok(js(v.member("mask"))?
                .map(|m| js(m.str()).map(str::to_owned))
                .transpose()?
                .unwrap_or_else(|| d.to_owned()))
        };
        Ok(if v.contains("u8") {
            let d = AclData::U8(Self::num(v.get_key("u8"))? as u8);
            (
                d,
                if field {
                    AclData::U8(mask_or(0xff)? as u8)
                } else {
                    AclData::None
                },
            )
        } else if field && v.contains("u16") {
            (
                AclData::U16(Self::num(v.get_key("u16"))? as u16),
                AclData::U16(mask_or(0xffff)? as u16),
            )
        } else if v.contains("u32") {
            let d = AclData::U32(Self::num(v.get_key("u32"))? as u32);
            (
                d,
                if field {
                    AclData::U32(mask_or(0xffff_ffff)? as u32)
                } else {
                    AclData::None
                },
            )
        } else if v.contains("s32") {
            (
                AclData::S32(self.num_in(v.get_key("s32"), ty, tag, what)? as i32),
                AclData::None,
            )
        } else if field && v.contains("ip4") {
            let d = text::ip4(js(v.get_key("ip4").str())?).ok_or_else(|| Error::Fail("bad ip4".to_owned()))?;
            let m = text::ip4(&str_or("255.255.255.255")?).ok_or_else(|| Error::Fail("bad mask".to_owned()))?;
            (AclData::Ip4(d), AclData::Ip4(m))
        } else if field && v.contains("ip6") {
            let d = text::ip6(js(v.get_key("ip6").str())?).ok_or_else(|| Error::Fail("bad ip6".to_owned()))?;
            let m = text::ip6(&str_or("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")?)
                .ok_or_else(|| Error::Fail("bad mask6".to_owned()))?;
            (AclData::Ip6(d), AclData::Ip6(m))
        } else if field && v.contains("mac") {
            let d = Self::mac(js(v.get_key("mac").str())?)?;
            (AclData::Mac(d), AclData::Mac(Self::mac(&str_or("ff:ff:ff:ff:ff:ff")?)?))
        } else if v.contains("oid") {
            (AclData::Oid(self.oid(v.get_key("oid"))?), AclData::None)
        } else if v.contains("oids") {
            let l = v
                .get_key("oids")
                .items()
                .into_iter()
                .map(|o| self.oid(o))
                .collect::<R<Vec<_>>>()?;
            (AclData::Oids(l), AclData::None)
        } else {
            (AclData::None, AclData::None)
        })
    }

    fn enable(v: &Json) -> R<bool> {
        js(v.member("enable"))?.map_or(Ok(true), |e| js(e.boolean()))
    }

    #[allow(clippy::cast_possible_truncation, reason = "the C++ casts")]
    fn qos_field(p: &mut sai_client::QosParams, f: &str, v: u32) -> R<()> {
        match f {
            "tc" => p.tc = v as u8,
            "dscp" => p.dscp = v as u8,
            "dot1p" => p.dot1p = v as u8,
            "prio" => p.prio = v as u8,
            "pg" => p.pg = v as u8,
            "queue" => p.queue_index = v as u8,
            "color" => p.color = v,
            _ => return fail(format!("unknown qos map field {f}")),
        }
        Ok(())
    }

    /// The attribute id of `a[0]` for type `t`.
    fn attr_id(&self, t: sys::sai_object_type_t, a0: &Json, tag: &str) -> R<u32> {
        let an = if let Json::Str(s) = a0 { s.as_str() } else { "" };
        let id = if let Some(v) = lookup_last(n::attr_table(t), an) {
            v
        } else if an.is_empty() {
            Self::num(a0)?
        } else if let Some(v) = lookup(n::K_ATTRS_PORT_QOS, an) {
            v
        } else if let Some(m) = self
            .fixed()
            .then(|| {
                sai_meta::attributes(t)
                    .iter()
                    .find(|m| m.name.split_once("_ATTR_").map(|x| x.1) == Some(an))
            })
            .flatten()
        {
            let was = Self::num(a0).map_or_else(|e| e.text().to_owned(), |v| v.to_string());
            Self::fix(
                tag,
                &format!(
                    "attribute {an} = {} (not in the C++ table of type {t}; C++: {was})",
                    m.name
                ),
            );
            i64::from(m.id)
        } else {
            Self::num(a0)?
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "(sai_attr_id_t)id"
        )]
        Ok(id as u32)
    }

    /// `make_attr(t, a, st)`: one attribute `["NAME", "type", value]`.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "make_attr's cases, in its order; its casts"
    )]
    fn make_attr(&self, t: sys::sai_object_type_t, a: &Json, tag: &str) -> R<Attr> {
        let a0 = js(a.at_index(0))?;
        let id = self.attr_id(t, a0, tag)?;
        let ty = js(js(a.at_index(1))?.str())?;
        let v = if a.len() > 2 {
            js(a.at_index(2))?.clone()
        } else {
            Json::Null
        };
        let what = a0.str().unwrap_or("?").to_owned();
        let enum_ty = enum_of(t, id);
        if v == Json::Null && !matches!(ty, "oidlist" | "u32list" | "qosmap") {
            // a get: nothing written, read back as the type
            return Ok(Attr::new(id, zero(ty)));
        }
        let size = |dflt: usize| -> usize {
            if v.is_number() {
                v.u64().ok().and_then(|x| usize::try_from(x).ok()).unwrap_or(dflt)
            } else {
                dflt
            }
        };
        let value = match ty {
            "bool" => Value::Bool(js(v.boolean())?),
            "u8" => Value::U8(Self::num(&v)? as u8),
            "s8" => Value::S8(Self::num(&v)? as i8),
            "u16" => Value::U16(Self::num(&v)? as u16),
            "u32" => Value::U32(Self::num(&v)? as u32),
            "s32" => Value::S32(self.num_in(&v, enum_ty, tag, &what)? as i32),
            "u64" => Value::U64(Self::num(&v)? as u64),
            "oid" => Value::Oid(self.oid(&v)?),
            "mac" => Value::Mac(Self::mac(js(v.str())?)?),
            "ip" => Value::Ip(Self::ip(js(v.str())?)?),
            "chardata" => Value::Chardata(js(v.str())?.bytes().take_while(|c| *c != 0).collect()),
            "oidlist" => Value::OidList(vec![0; size(256)]),
            "u32list" => {
                if let Json::Arr(l) = &v {
                    Value::U32List(l.iter().map(|e| Self::num(e).map(|x| x as u32)).collect::<R<_>>()?)
                } else {
                    Value::U32List(vec![0; size(64)])
                }
            }
            "oids" => Value::OidList(v.items().into_iter().map(|e| self.oid(e)).collect::<R<_>>()?),
            "qosmap" => {
                if let Json::Arr(l) = &v {
                    let mut out = Vec::new();
                    for e in l {
                        let mut m = QosMapEntry::default();
                        Self::qos_field(
                            &mut m.key,
                            js(js(e.at_index(0))?.str())?,
                            Self::num(js(e.at_index(1))?)? as u32,
                        )?;
                        Self::qos_field(
                            &mut m.value,
                            js(js(e.at_index(2))?.str())?,
                            Self::num(js(e.at_index(3))?)? as u32,
                        )?;
                        out.push(m);
                    }
                    Value::QosMap(out)
                } else {
                    Value::QosMap(vec![QosMapEntry::default(); size(128)])
                }
            }
            "s32list" => {
                if let Json::Arr(l) = &v {
                    Value::S32List(
                        l.iter()
                            .map(|e| self.num_in(e, enum_ty, tag, &what).map(|x| x as i32))
                            .collect::<R<_>>()?,
                    )
                } else {
                    Value::S32List(vec![0; size(64)])
                }
            }
            "aclfield" => {
                let enable = Self::enable(&v)?;
                let (data, mask) = self.acl_data(&v, true, enum_ty, tag, &what)?;
                Value::AclField(AclField { enable, data, mask })
            }
            "aclaction" => {
                let enable = Self::enable(&v)?;
                let (parameter, _) = self.acl_data(&v, false, enum_ty, tag, &what)?;
                Value::AclAction(AclAction { enable, parameter })
            }
            "u32range" => Value::U32Range(
                Self::num(js(v.at_index(0))?)? as u32,
                Self::num(js(v.at_index(1))?)? as u32,
            ),
            "ptr" => Value::Notify(match js(v.str())? {
                "port_state" => Notify::PortState,
                "fdb" => Notify::Fdb,
                "shutdown" => Notify::Shutdown,
                _ => Notify::Null,
            }),
            _ => return fail(format!("unknown attr type {ty}")),
        };
        Ok(Attr::new(id, value))
    }
}

/// The value a get starts from: zero, read back as `ty`.
fn zero(ty: &str) -> Value {
    match ty {
        "bool" => Value::Bool(false),
        "u8" => Value::U8(0),
        "s8" => Value::S8(0),
        "u16" => Value::U16(0),
        "u32" => Value::U32(0),
        "s32" => Value::S32(0),
        "u64" => Value::U64(0),
        "oid" => Value::Oid(0),
        "mac" => Value::Mac([0; 6]),
        "oids" => Value::OidList(Vec::new()),
        _ => Value::None,
    }
}

/// `show(t, x, ty)`.
pub fn show(a: &Attr, ty: &str) -> String {
    use std::fmt::Write as _;
    match (&a.value, ty) {
        (Value::Bool(v), "bool") => v.to_string(),
        (Value::S32(v), "s32") => v.to_string(),
        (Value::U32(v), "u32") => v.to_string(),
        (Value::U64(v), "u64") => v.to_string(),
        (Value::U8(v), "u8") => v.to_string(),
        (Value::S8(v), "s8") => v.to_string(),
        (Value::U16(v), "u16") => v.to_string(),
        (Value::QosMap(l), "qosmap") => {
            let mut s = format!("{} entries", a.count);
            for m in l.iter().take(8) {
                let _ = write!(
                    s,
                    " {}/{}->{}/{}",
                    m.key.dscp, m.key.tc, m.value.tc, m.value.queue_index
                );
            }
            s
        }
        (Value::Oid(v), "oid") => format!("0x{v:x}"),
        (Value::OidList(l), "oidlist" | "oids") => {
            let mut s = format!("{} oids", a.count);
            for o in l.iter().take(16) {
                let _ = write!(s, " 0x{o:x}");
            }
            s
        }
        (Value::U32List(l), "u32list") => {
            format!("[{}]", l.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
        }
        (Value::Mac(m), "mac") => format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            m[0], m[1], m[2], m[3], m[4], m[5]
        ),
        _ => "?".to_owned(),
    }
}

impl Runner<'_> {
    // ------------------------------------------------------------ steps

    /// `run(steps, depth)`.
    pub fn run(&mut self, steps: &Json, depth: usize) -> R<()> {
        match steps {
            Json::Arr(a) => {
                for (i, s) in a.iter().enumerate() {
                    self.step(s, i, depth)?;
                }
                Ok(())
            }
            Json::Null => Ok(()),
            o => Err(Error::Other(format!(
                "[json.exception.type_error.305] cannot use operator[] with a numeric argument with {}",
                o.type_name()
            ))),
        }
    }

    fn may_fail(s: &Json) -> R<bool> {
        js(s.member("may_fail"))?.map_or(Ok(false), |v| js(v.boolean()))
    }

    /// An exec command with the configured tools: `$ORACLE/bin/NAME` (also with the variable in braces)
    /// followed by a blank, `;` or the end becomes the tool's command.
    pub fn rewrite(&self, cmd: &str) -> String {
        let mut out = cmd.to_owned();
        for (name, with) in &self.opt.tools {
            for form in [format!("$ORACLE/bin/{name}"), format!("\u{24}{{ORACLE}}/bin/{name}")] {
                let mut done = String::new();
                let mut rest = out.as_str();
                while let Some(i) = rest.find(&form) {
                    let after = rest.get(i + form.len()..).unwrap_or_default();
                    let at_word = after.is_empty() || after.starts_with(char::is_whitespace) || after.starts_with(';');
                    done.push_str(rest.get(..i).unwrap_or_default());
                    done.push_str(if at_word { with } else { &form });
                    rest = after;
                }
                done.push_str(rest);
                out = done;
            }
        }
        out
    }

    fn exec(&mut self, s: &Json) -> R<()> {
        let cmd = js(js(s.at("cmd"))?.str())?.to_owned();
        self.mark(&format!("exec {cmd}"));
        let run = self.rewrite(&cmd);
        if run != cmd {
            warn(&format!("sai-script: exec as: {run}"));
        }
        let _ = std::io::stdout().flush();
        let rc = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&run)
            .status()
            .map_or(-1, std::process::ExitStatus::into_raw);
        say(&format!("EXEC rc={rc} {cmd}"));
        if rc != 0 && !Self::may_fail(s)? {
            self.failed += 1;
        }
        Ok(())
    }

    fn diag(&self, s: &Json) -> R<()> {
        let Some(path) = self.profile.get("SAI_OPENBCM_DIAG_SOCKET") else {
            return fail("diag: no SAI_OPENBCM_DIAG_SOCKET in the profile");
        };
        let cmd = format!("{}\n", js(js(s.at("cmd"))?.str())?);
        let Ok(mut c) = UnixStream::connect(path) else {
            return fail(format!("diag: cannot connect {path}"));
        };
        let _ = c.write_all(cmd.as_bytes());
        let _ = c.shutdown(std::net::Shutdown::Write);
        let mut o = std::io::stdout().lock();
        let _ = o.write_all(format!("DIAG {cmd}").as_bytes());
        let mut buf = [0u8; 4096];
        while let Ok(k) = c.read(&mut buf) {
            if k == 0 {
                break;
            }
            let _ = o.write_all(buf.get(..k).unwrap_or_default());
        }
        let _ = o.flush();
        Ok(())
    }

    /// The step's `obj` and its type (`None`: skipped, a `may_fail` step's unknown object).
    fn target(&self, s: &Json, tag: &str) -> R<Option<Target>> {
        let o = s.get_key("obj");
        let oid = match self.oid(o) {
            Ok(v) => v,
            Err(Error::Fail(f)) if Self::may_fail(s)? => {
                say(&format!("STEP {} skipped: {f}", pad(tag, 40)));
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let mut t = self.sai.object_type(oid);
        let mut by_declared = false;
        if t == sys::SAI_OBJECT_TYPE_NULL && *o == Json::Str("switch".to_owned()) {
            t = sys::SAI_OBJECT_TYPE_SWITCH;
        } else if t == sys::SAI_OBJECT_TYPE_NULL && self.fixed() {
            let name = o.str().ok().and_then(|x| x.strip_prefix('$')).unwrap_or_default();
            if let Some(d) = self.declared.get(name) {
                Self::fix(
                    tag,
                    &format!("object {name} is 0x{oid:x} (its create failed): type {d} as created, not the null type"),
                );
                t = *d;
                by_declared = true;
            }
        }
        Ok(Some(Target { oid, t, by_declared }))
    }

    fn ids(
        s: &Json,
        t: sys::sai_object_type_t,
        table: &[n::Name],
        fixed: bool,
        tag: &str,
    ) -> R<Vec<sys::sai_stat_id_t>> {
        let mut out = Vec::new();
        for x in js(s.at("ids"))?.items() {
            let v = match x {
                Json::Str(name) => {
                    if let Some(v) = lookup(table, name) {
                        v
                    } else {
                        let ty = stat_enum(t);
                        let full = format!("{}{name}", sai_driver::names::enum_prefix(ty));
                        match (fixed, sai_driver::names::enum_value(ty, &full)) {
                            (true, Some(v)) if Self::num(x).is_err() => {
                                Self::fix(tag, &format!("counter {name} = {full} (not in the C++ table)"));
                                v
                            }
                            _ => Self::num(x)?,
                        }
                    }
                }
                _ => Self::num(x)?,
            };
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "(sai_stat_id_t)v"
            )]
            out.push(v as u32);
        }
        Ok(out)
    }

    fn entry_key(&self, s: &Json, op: &str) -> R<Entry> {
        Ok(if op.ends_with("route") || op == "route_fill" {
            Entry::Route {
                vr: self.oid(js(s.at("vr"))?)?,
                prefix: Self::prefix(js(js(s.at("prefix"))?.str())?)?,
            }
        } else if op.ends_with("neighbor") {
            Entry::Neighbor {
                rif: self.oid(js(s.at("rif"))?)?,
                ip: Self::ip(js(js(s.at("ip"))?.str())?)?,
            }
        } else {
            Entry::Fdb {
                bv: self.oid(js(s.at("vlan"))?)?,
                mac: Self::mac(js(js(s.at("mac"))?.str())?)?,
            }
        })
    }

    /// The control steps (no SAI call); `None` for the others.
    fn control(&mut self, op: &str, s: &Json, depth: usize) -> R<Option<()>> {
        match op {
            "repeat" => {
                let k = js(js(s.at("n"))?.i64())?;
                for _ in 0..k {
                    self.run(js(s.at("steps"))?, depth + 1)?;
                }
            }
            "foreach" => {
                let name = js(js(s.at("list"))?.str())?.to_owned();
                let Some(size) = self.lists.get(&name).map(Vec::len) else {
                    return fail(format!("no list {name}"));
                };
                let max = js(s.member("max"))?.map(|m| js(m.u64())).transpose()?;
                let max = max.and_then(|m| usize::try_from(m).ok()).unwrap_or(size);
                let mut i = 0;
                while i < self.lists.get(&name).map_or(0, Vec::len) && i < max {
                    let v = self.lists.get(&name).and_then(|l| l.get(i)).copied().unwrap_or(0);
                    let as_ = js(js(s.at("as"))?.str())?.to_owned();
                    self.obj.insert(as_, v);
                    self.run(js(s.at("steps"))?, depth + 1)?;
                    i += 1;
                }
            }
            "sleep" => {
                let ms = js(js(s.at("ms"))?.i64())?;
                std::thread::sleep(std::time::Duration::from_millis(u64::try_from(ms).unwrap_or(0)));
            }
            "print" => say(js(js(s.at("text"))?.str())?),
            "exec" => self.exec(s)?,
            "diag" => self.diag(s)?,
            _ => return Ok(None),
        }
        Ok(Some(()))
    }

    #[allow(clippy::too_many_lines, reason = "step(), op by op as the C++")]
    fn step(&mut self, s: &Json, idx: usize, depth: usize) -> R<()> {
        let op = js(js(s.at("op"))?.str())?.to_owned();
        let desc = js(s.member("desc"))?
            .map(|d| js(d.str()).map(str::to_owned))
            .transpose()?
            .unwrap_or_default();
        let tag = format!(
            "{depth}.{idx} {op}{}",
            if desc.is_empty() {
                String::new()
            } else {
                format!(" {desc}")
            }
        );
        if self.control(&op, s, depth)?.is_some() {
            return Ok(());
        }
        let mut t = sys::SAI_OBJECT_TYPE_NULL;
        let mut target = Target {
            oid: 0,
            t,
            by_declared: false,
        };
        if s.contains("type") {
            t = Self::type_of(js(s.get_key("type").str())?)?;
        }
        if s.contains("obj") {
            let Some(x) = self.target(s, &tag)? else {
                return Ok(());
            };
            t = x.t;
            target = x;
        }
        match op.as_str() {
            "create_switch" => t = sys::SAI_OBJECT_TYPE_SWITCH,
            "create_route" | "remove_route" | "set_route" | "route_fill" => t = sys::SAI_OBJECT_TYPE_ROUTE_ENTRY,
            "create_neighbor" | "remove_neighbor" | "set_neighbor" => t = sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY,
            "create_fdb" | "remove_fdb" | "set_fdb" => t = sys::SAI_OBJECT_TYPE_FDB_ENTRY,
            "flush_fdb" => t = sys::SAI_OBJECT_TYPE_FDB_FLUSH,
            _ => {}
        }
        let mut list: Vec<Json> = Vec::new();
        if let Some(a) = js(s.member("attrs"))? {
            list.extend(a.items().into_iter().cloned());
        }
        if s.contains("attr") {
            list.push(s.get_key("attr").clone());
        }
        let mut attrs = list.iter().map(|a| self.make_attr(t, a, &tag)).collect::<R<Vec<_>>>()?;
        // dispatch: the C++ asks sai_object_type_query(o) again; by the declared type after the fix
        let by = if target.by_declared {
            t
        } else {
            self.sai.object_type(target.oid)
        };
        let r;
        let mut out = String::new();
        self.mark(&format!("step {tag}"));
        let t0 = self.now_ms();
        match op.as_str() {
            "create_switch" => {
                let mut sw = self.switch;
                r = self.sai.create_switch(&mut sw, &attrs);
                self.switch = sw;
            }
            "remove_switch" => r = self.sai.remove_switch(self.switch),
            "create" => {
                let (st, o) = self.sai.create(t, self.switch, &attrs);
                r = st;
                if s.contains("as") {
                    let name = js(s.get_key("as").str())?.to_owned();
                    if self.fixed() {
                        self.declared.insert(name.clone(), t);
                    }
                    self.obj.insert(name, o);
                }
                out = format!("0x{o:x}");
            }
            "remove" => r = self.sai.remove(by, target.oid),
            "set" => {
                // the C++ passes attrs.data(): NULL without an attribute
                r = self.sai.set(by, target.oid, attrs.first());
            }
            "get" => {
                r = self.sai.get(by, target.oid, &mut attrs);
                let mut parts = Vec::new();
                for (a, l) in attrs.iter().zip(&list) {
                    if r != sai_client::SUCCESS {
                        break;
                    }
                    let ty = js(js(l.at_index(1))?.str())?;
                    parts.push(show(a, ty));
                    if s.contains("as") {
                        let name = js(s.get_key("as").str())?.to_owned();
                        match (&a.value, ty) {
                            (Value::OidList(v), "oidlist" | "oids") => {
                                self.lists.insert(name, v.clone());
                            }
                            (Value::Oid(v), "oid") => {
                                self.obj.insert(name, *v);
                            }
                            _ => {}
                        }
                    }
                }
                out = parts.join(" ");
            }
            "qstats" | "stats" => {
                let (table, kind) = match (op.as_str(), t) {
                    ("qstats", sys::SAI_OBJECT_TYPE_QUEUE) => (n::K_QUEUE_STATS, t),
                    ("qstats", sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP) => (n::K_IPG_STATS, t),
                    ("qstats", _) => (n::K_POOL_STATS, sys::SAI_OBJECT_TYPE_BUFFER_POOL),
                    (_, sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE) => (n::K_RIF_STATS, t),
                    _ => (n::K_PORT_STATS, sys::SAI_OBJECT_TYPE_PORT),
                };
                let ids = Self::ids(s, kind, table, self.fixed(), &tag)?;
                let (st, vals) = self.sai.stats(kind, target.oid, &ids);
                r = st;
                if r == sai_client::SUCCESS {
                    out = vals.iter().map(u64::to_string).collect::<Vec<_>>().join(" ");
                }
            }
            "stats_cap" => {
                let (st, caps) = self.sai.stats_capability(self.switch, t, 512);
                r = st;
                out = caps
                    .iter()
                    .map(|(e, m)| format!("{e}:{m}"))
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            "create_route" | "remove_route" | "set_route" | "create_neighbor" | "remove_neighbor" | "set_neighbor"
            | "create_fdb" | "remove_fdb" | "set_fdb" => {
                let key = self.entry_key(s, &op)?;
                let eop = if op.starts_with("create") {
                    EntryOp::Create
                } else if op.starts_with("set") {
                    EntryOp::Set
                } else {
                    EntryOp::Remove
                };
                r = self.sai.entry(eop, self.switch, &key, &attrs);
            }
            "route_fill" => {
                let (st, txt) = self.route_fill(s, &attrs)?;
                r = st;
                out = txt;
            }
            "flush_fdb" => r = self.sai.flush_fdb(self.switch, &attrs),
            _ => return fail(format!("unknown op {op}")),
        }
        let dt_ms = self.now_ms() - t0;
        self.mark(&format!("end {tag} status={r} ms={dt_ms:.3}"));
        say(&format!("STEP {} {r} {dt_ms:10.3} ms {out}", pad(&tag, 40)));
        if r != sai_client::SUCCESS && !Self::may_fail(s)? {
            self.failed += 1;
        }
        Ok(())
    }

    /// `route_fill`: `count` routes from the prefix upwards, created (or removed) one by one.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the C++'s arithmetic"
    )]
    fn route_fill(&mut self, s: &Json, attrs: &[Attr]) -> R<(sai_client::Status, String)> {
        let vr = self.oid(js(s.at("vr"))?)?;
        let mut prefix = Self::prefix(js(js(s.at("prefix"))?.str())?)?;
        let count = usize::try_from(js(js(s.at("count"))?.u64())?).unwrap_or(usize::MAX);
        let report = js(s.member("report"))?
            .map(|v| js(v.u64()))
            .transpose()?
            .map_or(1000, |v| usize::try_from(v).unwrap_or(1000));
        let rm = js(s.member("remove"))?
            .map(|v| js(v.boolean()))
            .transpose()?
            .unwrap_or(false);
        let bytes = if prefix.v6 { 16 } else { 4 };
        let len: i32 = prefix
            .mask
            .iter()
            .take(bytes)
            .map(|x| i32::try_from(x.count_ones()).unwrap_or(0))
            .sum();
        let mut tb = self.now_ms();
        let mut done = 0usize;
        let mut r = sai_client::SUCCESS;
        let op = if rm { EntryOp::Remove } else { EntryOp::Create };
        while done < count {
            r = self.sai.entry(op, self.switch, &Entry::Route { vr, prefix }, attrs);
            if r != sai_client::SUCCESS {
                break;
            }
            if (done + 1).is_multiple_of(report) {
                let t = self.now_ms();
                say(&format!(
                    "FILL {} n={} block_ms={:.1} us_per_route={:.1}",
                    if rm { "remove" } else { "create" },
                    done + 1,
                    t - tb,
                    (t - tb) * 1000.0 / report as f64
                ));
                tb = t;
            }
            // next prefix: add 1 at bit (len - 1), counted from the top
            let bit = len - 1;
            let mut carry = true;
            let mut byte = bit / 8;
            while byte >= 0 && carry {
                let sh = if byte == bit / 8 { 7 - bit % 8 } else { 0 };
                let i = usize::try_from(byte).unwrap_or(0);
                let add = if (0..32).contains(&sh) { 1u32 << sh } else { 0 };
                let v = u32::from(prefix.addr.get(i).copied().unwrap_or(0)) + add;
                if let Some(x) = prefix.addr.get_mut(i) {
                    *x = v as u8;
                }
                carry = v > 0xff;
                byte -= 1;
            }
            done += 1;
        }
        let fb = format!(
            "{} {done} of {count} (stopped: status {r})",
            if rm { "removed" } else { "created" }
        );
        say(&format!("FILL done {fb}"));
        Ok((r, fb))
    }
}

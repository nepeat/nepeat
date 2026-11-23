//! The text form of backend calls: one line per call, byte for byte the format of the C++
//! recording backend (`sai-openbcm/src/recording_backend.cpp`, format v1 in the migration
//! scratchpad's `call-log-format.md`), so the two libraries' logs can be compared line by line.
//!
//! `<op>(<arg>=<value>, ...) -> <STATUS>[ <out>=<value>]...`, events as
//! `event link(hw=1, up=true)`. Values: decimal numbers and ids, `true` / `false`, quoted
//! strings, lower-case hex MACs, dotted / uncompressed IP addresses, enumerators by their C++
//! name, `[a,b]` lists, `{f=v,...}` structures (all fields in C++ order).
use super::types::{
    AsicTemps, BufferCfg, ChipLimits, Dest, EgressId, FpCounter, FpGroupExtra, FpMatch, FpRule, LagMember, LinkGate,
    MacAddr, Masked, MdioDev, MirrorCfg, NetifCreated, PhyAddr, PolicerCfg, QosMapEntry, Redirect, RouteTarget,
    SchedCfg, TableUsage, TrapSpec, Vrf, WredCfg,
};
use super::{BackendEvent, HwPort, PortInfo, Profile, Vid};
use crate::error::{AttrError, Error, Result};
use crate::value::IpPrefix;
use core::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A value as the call log prints it.
pub trait LogValue {
    /// Appends the value's text.
    fn log(&self, out: &mut String);
    /// An argument the call log leaves out (a file descriptor: a process-local number).
    fn hidden(&self) -> bool {
        false
    }
}

impl LogValue for std::os::fd::BorrowedFd<'_> {
    fn log(&self, _out: &mut String) {}
    fn hidden(&self) -> bool {
        true
    }
}

/// The outputs of a successful call (` name=value` each, in C++ order).
pub trait LogOut {
    /// Appends ` <name>=<value>` for each output; `names` are the C++ output names.
    fn log_out(&self, names: &[&str], out: &mut String);
}

// `write!` into a String cannot fail.
macro_rules! put {
    ($out:expr, $($fmt:tt)*) => {
        let _ = write!($out, $($fmt)*);
    };
}

macro_rules! decimal {
    ($($t:ty),*) => {
        $(impl LogValue for $t {
            fn log(&self, out: &mut String) {
                put!(out, "{self}");
            }
        })*
    };
}
decimal!(u8, u16, u32, u64, i8, i16, i32, i64, usize);

macro_rules! id_decimal {
    ($($t:ty),*) => {
        $(impl LogValue for $t {
            fn log(&self, out: &mut String) {
                put!(out, "{}", self.0);
            }
        })*
    };
}
id_decimal!(
    HwPort,
    PhyAddr,
    Vrf,
    super::types::TrunkId,
    super::types::NetifId,
    super::types::FilterId,
    super::types::IntfId,
    super::types::StationId,
    EgressId,
    super::types::EcmpId,
    super::types::FpGroupId,
    super::types::FpEntryId,
    super::types::PolicerId,
    super::types::QosMapId,
    super::types::WredId,
    super::types::MirrorId,
    super::types::SampleId,
    super::types::GenlId
);

impl LogValue for Vid {
    fn log(&self, out: &mut String) {
        put!(out, "{}", self.get());
    }
}

impl LogValue for MdioDev {
    fn log(&self, out: &mut String) {
        put!(out, "{}", self.get());
    }
}

impl LogValue for bool {
    fn log(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl LogValue for str {
    /// Double-quoted; `"` and `\\` escaped; bytes below 0x20 or from 0x7f as `\\xHH`.
    fn log(&self, out: &mut String) {
        out.push('"');
        for byte in self.bytes() {
            match byte {
                b'"' => out.push_str("\\\""),
                b'\\' => out.push_str("\\\\"),
                0x20..=0x7e => out.push(char::from(byte)),
                other => {
                    put!(out, "\\x{other:02x}");
                }
            }
        }
        out.push('"');
    }
}

impl LogValue for String {
    fn log(&self, out: &mut String) {
        self.as_str().log(out);
    }
}

impl<T: LogValue + ?Sized> LogValue for &T {
    fn log(&self, out: &mut String) {
        (**self).log(out);
    }
    fn hidden(&self) -> bool {
        (**self).hidden()
    }
}

impl<T: LogValue> LogValue for [T] {
    fn log(&self, out: &mut String) {
        out.push('[');
        for (i, v) in self.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            v.log(out);
        }
        out.push(']');
    }
}

impl<T: LogValue> LogValue for Vec<T> {
    fn log(&self, out: &mut String) {
        self.as_slice().log(out);
    }
}

impl<A: LogValue, B: LogValue> LogValue for (A, B) {
    fn log(&self, out: &mut String) {
        out.push('(');
        self.0.log(out);
        out.push(',');
        self.1.log(out);
        out.push(')');
    }
}

impl<T: LogValue, const N: usize> LogValue for [T; N] {
    fn log(&self, out: &mut String) {
        self.as_slice().log(out);
    }
}

impl LogValue for MacAddr {
    fn log(&self, out: &mut String) {
        put!(out, "{self}");
    }
}

impl LogValue for Ipv4Addr {
    fn log(&self, out: &mut String) {
        put!(out, "{self}");
    }
}

impl LogValue for Ipv6Addr {
    /// Eight groups of lower-case hex without leading zeros, no `::` compression.
    fn log(&self, out: &mut String) {
        for (i, group) in self.segments().iter().enumerate() {
            if i > 0 {
                out.push(':');
            }
            put!(out, "{group:x}");
        }
    }
}

impl LogValue for IpAddr {
    fn log(&self, out: &mut String) {
        match self {
            Self::V4(a) => a.log(out),
            Self::V6(a) => a.log(out),
        }
    }
}

/// The prefix length of a contiguous mask.
fn prefix_len(mask: IpAddr) -> Option<u32> {
    let (bits, width) = match mask {
        IpAddr::V4(m) => (u128::from(m.to_bits()) << 96, 32),
        IpAddr::V6(m) => (m.to_bits(), 128),
    };
    let len = bits.leading_ones();
    (bits.checked_shl(len).unwrap_or(0) == 0 && len <= width).then_some(len)
}

impl LogValue for IpPrefix {
    /// `<addr>/<len>` for a contiguous mask, else `<addr>/<mask>`.
    fn log(&self, out: &mut String) {
        self.addr().log(out);
        out.push('/');
        match prefix_len(self.mask()) {
            Some(len) => {
                put!(out, "{len}");
            }
            None => self.mask().log(out),
        }
    }
}

impl LogValue for Dest {
    /// The C++ integer (a LAG: `0x40000000 | tid` in decimal).
    fn log(&self, out: &mut String) {
        put!(out, "{}", self.raw());
    }
}

impl LogValue for RouteTarget {
    /// The two C++ arguments of `l3_route_add`: `<target>, ecmp=<bool>` (the `target=` before it
    /// is the argument's name).
    fn log(&self, out: &mut String) {
        match self {
            Self::Egress(id) => {
                put!(out, "{}, ecmp=false", id.0);
            }
            Self::Ecmp(id) => {
                put!(out, "{}, ecmp=true", id.0);
            }
        }
    }
}

/// Optional ids and numbers: `-1` for none (the C++ convention).
macro_rules! minus_one {
    ($($t:ty),*) => {
        $(impl LogValue for Option<$t> {
            fn log(&self, out: &mut String) {
                match self {
                    Some(v) => v.log(out),
                    None => out.push_str("-1"),
                }
            }
        })*
    };
}
minus_one!(
    u8,
    u32,
    Vid,
    Dest,
    super::types::PolicerId,
    super::types::QosMapId,
    super::types::WredId,
    super::types::SampleId,
    super::types::FilterId
);

/// A link gate argument: `set` / `empty`.
impl LogValue for Option<LinkGate> {
    fn log(&self, out: &mut String) {
        out.push_str(if self.is_some() { "set" } else { "empty" });
    }
}

/// Optional structures passed by pointer in the C++ (`SchedCfg*`, `BufferCfg*`): `none` for null.
macro_rules! none_for_null {
    ($($t:ty),*) => {
        $(impl LogValue for Option<&$t> {
            fn log(&self, out: &mut String) {
                match self {
                    Some(v) => v.log(out),
                    None => out.push_str("none"),
                }
            }
        })*
    };
}
none_for_null!(SchedCfg, BufferCfg);

/// Enumerators print as their C++ names, which are the Rust variant names.
macro_rules! by_name {
    ($($t:ty),*) => {
        $(impl LogValue for $t {
            fn log(&self, out: &mut String) {
                put!(out, "{self:?}");
            }
        })*
    };
}
by_name!(
    super::types::Fec,
    super::types::Loopback,
    super::types::Learn,
    super::types::PortStat,
    super::types::TrapKind,
    super::types::FpQual,
    super::types::FpCap,
    super::types::QosMapKind,
    super::types::StormKind,
    super::types::FloodKind
);

impl LogValue for super::types::PortInterface {
    /// The C++ passes the interface type as its name (a string).
    fn log(&self, out: &mut String) {
        self.name().log(out);
    }
}

/// Writes `{f=v,f=v}`.
macro_rules! fields {
    ($out:expr, $self:expr; $($name:literal = $value:expr),* $(,)?) => {{
        $out.push('{');
        let mut first = true;
        $(
            if !first {
                $out.push(',');
            }
            first = false;
            $out.push_str($name);
            $out.push('=');
            LogValue::log(&$value, $out);
        )*
        let _ = first;
        $out.push('}');
    }};
}

impl LogValue for PortInfo {
    /// `{hw=..,lanes=..,speed=..,name="..",autoneg=..}`.
    fn log(&self, out: &mut String) {
        fields!(out, self; "hw" = self.hw, "lanes" = self.lanes, "speed" = self.speed, "name" = self.name, "autoneg" = self.autoneg);
    }
}

impl LogValue for LagMember {
    fn log(&self, out: &mut String) {
        fields!(out, self; "hw" = self.hw, "egress_disable" = self.egress_disable, "ingress_disable" = self.ingress_disable);
    }
}

impl LogValue for TableUsage {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "v4_routes_free" = self.v4_routes_free,
            "v6_routes_free" = self.v6_routes_free,
            "hosts_free" = self.hosts_free,
            "egress_free" = self.egress_free,
            "fdb_free" = self.fdb_free,
            "ecmp_groups_free" = self.ecmp_groups_free,
            "ecmp_members_free" = self.ecmp_members_free,
        );
    }
}

impl LogValue for ChipLimits {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "buffer_bytes" = self.buffer_bytes,
            "acl_prio_min" = self.acl_prio_min,
            "acl_prio_max" = self.acl_prio_max,
            "vrf_max" = self.vrf_max,
            "ecmp_groups" = self.ecmp_groups,
            "ecmp_paths" = self.ecmp_paths,
            "ecmp_members" = self.ecmp_members,
        );
    }
}

impl LogValue for PolicerCfg {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "tr_tcm" = self.tr_tcm,
            "packets" = self.packets,
            "color_blind" = self.color_blind,
            "cir" = self.cir,
            "cbs" = self.cbs,
            "pir" = self.pir,
            "pbs" = self.pbs,
        );
    }
}

impl LogValue for TrapSpec {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "type" = self.trap_type,
            "action" = self.action,
            "priority" = self.priority,
            "queue" = self.queue,
            "group_up" = self.group_up,
            "policer" = self.policer,
            "yellow_drop" = self.yellow_drop,
            "red_drop" = self.red_drop,
        );
    }
}

impl LogValue for SchedCfg {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "strict" = self.strict,
            "weight" = self.weight,
            "packets" = self.packets,
            "min_rate" = self.min_rate,
            "min_burst" = self.min_burst,
            "max_rate" = self.max_rate,
            "max_burst" = self.max_burst,
        );
    }
}

impl LogValue for WredCfg {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "enable" = self.enable,
            "min" = self.min,
            "max" = self.max,
            "prob" = self.prob,
            "weight" = self.weight,
            "ecn" = self.ecn,
        );
    }
}

impl LogValue for BufferCfg {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "ingress" = self.ingress,
            "dynamic" = self.dynamic,
            "dynamic_th" = self.dynamic_th,
            "static_th" = self.static_th,
            "reserved" = self.reserved,
            "xoff" = self.xoff,
            "xon" = self.xon,
            "xon_offset" = self.xon_offset,
            "pool_size" = self.pool_size,
            "pool_xoff" = self.pool_xoff,
        );
    }
}

impl LogValue for MirrorCfg {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "erspan" = self.erspan,
            "monitor" = self.monitor,
            "truncate" = self.truncate,
            "sample_rate" = self.sample_rate,
            "tos" = self.tos,
            "ttl" = self.ttl,
            "src" = self.src,
            "dst" = self.dst,
            "smac" = self.smac,
            "dmac" = self.dmac,
            "gre_type" = self.gre_type,
            "vlan" = self.vlan,
            "vlan_tpid" = self.vlan_tpid,
            "vlan_id" = self.vlan_id,
            "vlan_pri" = self.vlan_pri,
            "vlan_cfi" = self.vlan_cfi,
        );
    }
}

impl LogValue for FpGroupExtra {
    fn log(&self, out: &mut String) {
        fields!(out, self;
            "egress" = self.egress,
            "src_range" = self.src_range,
            "dst_range" = self.dst_range,
            "out_ports" = self.out_ports,
            "redirect" = self.redirect,
            "set_tc" = self.set_tc,
            "set_dscp" = self.set_dscp,
            "mirror" = self.mirror,
        );
    }
}

impl LogValue for QosMapEntry {
    /// `<key>.<key_color>=<value>.<value_color>`.
    fn log(&self, out: &mut String) {
        put!(
            out,
            "{}.{}={}.{}",
            self.key,
            self.key_color,
            self.value,
            self.value_color
        );
    }
}

/// `0x<hex>/0x<hex>` (no padding).
struct Hex<T>(Masked<T>);

impl<T: core::fmt::LowerHex> LogValue for Hex<T> {
    fn log(&self, out: &mut String) {
        put!(out, "{:#x}/{:#x}", self.0.value, self.0.mask);
    }
}

/// `<value>/<mask>` with both in their own form (addresses, MACs).
struct Pair<T>(Masked<T>);

impl<T: LogValue> LogValue for Pair<T> {
    fn log(&self, out: &mut String) {
        self.0.value.log(out);
        out.push('/');
        self.0.mask.log(out);
    }
}

impl LogValue for FpMatch {
    /// Only the qualifiers present, in `FpQual` order, `<FpQual>=<data>`; then the ranges and
    /// `OutPorts` when set.
    fn log(&self, out: &mut String) {
        let mut items: Vec<String> = Vec::new();
        let mut item = |name: &str, value: &dyn LogValue| {
            let mut s = String::from(name);
            s.push('=');
            value.log(&mut s);
            items.push(s);
        };
        if let Some(ports) = &self.in_ports {
            item("InPorts", ports);
        }
        if let Some(v) = self.ether_type {
            item("EtherType", &Hex(v));
        }
        if let Some(v) = self.ip_type {
            item("IpType", &v);
        }
        if let Some(v) = self.ip_protocol {
            item("IpProtocol", &Hex(v));
        }
        if let Some(v) = self.src_ip {
            item("SrcIp", &Pair(v));
        }
        if let Some(v) = self.dst_ip {
            item("DstIp", &Pair(v));
        }
        if let Some(v) = self.src_ip6 {
            item("SrcIp6", &Pair(v));
        }
        if let Some(v) = self.dst_ip6 {
            item("DstIp6", &Pair(v));
        }
        if let Some(v) = self.l4_src_port {
            item("L4SrcPort", &Hex(v));
        }
        if let Some(v) = self.l4_dst_port {
            item("L4DstPort", &Hex(v));
        }
        if let Some(v) = self.tcp_flags {
            item("TcpFlags", &Hex(v));
        }
        if let Some(v) = self.icmp_type_code {
            item("IcmpTypeCode", &Hex(v));
        }
        if let Some(v) = self.tos {
            item("Tos", &Hex(v));
        }
        if let Some(v) = self.dst_mac {
            item("DstMac", &Pair(v));
        }
        if let Some(v) = self.src_mac {
            item("SrcMac", &Pair(v));
        }
        if let Some(v) = self.outer_vlan {
            item("OuterVlan", &Hex(v));
        }
        if let Some(v) = self.ttl {
            item("Ttl", &Hex(v));
        }
        if self.dst_cpu {
            item("DstCpu", &true);
        }
        let mut range = |name: &str, r: Option<(u16, u16)>| {
            if let Some((min, max)) = r {
                items.push(format!("{name}={min}-{max}"));
            }
        };
        range("L4SrcRange", self.l4_src_range);
        range("L4DstRange", self.l4_dst_range);
        if !self.out_ports.is_empty() {
            let mut s = String::from("OutPorts=");
            self.out_ports.log(&mut s);
            items.push(s);
        }
        out.push('{');
        out.push_str(&items.join(","));
        out.push('}');
    }
}

impl LogValue for FpRule {
    fn log(&self, out: &mut String) {
        let (redirect, redirect_egress) = match self.redirect {
            None => (None, false),
            Some(Redirect::Dest(d)) => (Some(d.raw()), false),
            Some(Redirect::Egress(e)) => (Some(e.0), true),
        };
        fields!(out, self;
            "priority" = self.priority,
            "m" = self.m,
            "drop" = self.drop,
            "drop_cancel" = self.drop_cancel,
            "copy_cpu" = self.copy_cpu,
            "counter" = self.counter,
            "policer" = self.policer,
            "green_drop" = self.green_drop,
            "yellow_drop" = self.yellow_drop,
            "red_drop" = self.red_drop,
            "cpu_copy_only" = self.cpu_copy_only,
            "mirror_ingress" = self.mirror_ingress,
            "mirror_egress" = self.mirror_egress,
            "redirect" = redirect,
            "redirect_egress" = redirect_egress,
            "set_tc" = self.set_tc,
            "set_dscp" = self.set_dscp,
        );
    }
}

impl LogValue for Profile {
    /// `{KEY="value",...}`, keys in byte order.
    fn log(&self, out: &mut String) {
        out.push('{');
        for (i, (key, value)) in self.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(key);
            out.push('=');
            value.log(out);
        }
        out.push('}');
    }
}

// ---- outputs

impl<T: LogOut + ?Sized> LogOut for &T {
    fn log_out(&self, names: &[&str], out: &mut String) {
        (**self).log_out(names, out);
    }
}

impl LogOut for () {
    fn log_out(&self, _names: &[&str], _out: &mut String) {}
}

/// One output value under the first name.
pub struct One<'a, T: ?Sized>(pub &'a T);

impl<T: LogValue + ?Sized> LogOut for One<'_, T> {
    fn log_out(&self, names: &[&str], out: &mut String) {
        out.push(' ');
        out.push_str(names.first().copied().unwrap_or("out"));
        out.push('=');
        self.0.log(out);
    }
}

fn named(names: &[&str], i: usize, out: &mut String, value: &dyn LogValue) {
    out.push(' ');
    out.push_str(names.get(i).copied().unwrap_or("out"));
    out.push('=');
    value.log(out);
}

impl LogOut for NetifCreated {
    fn log_out(&self, names: &[&str], out: &mut String) {
        named(names, 0, out, &self.netif_id);
        named(names, 1, out, &self.filter_id);
    }
}

impl LogOut for FpCounter {
    fn log_out(&self, names: &[&str], out: &mut String) {
        named(names, 0, out, &self.packets);
        named(names, 1, out, &self.bytes);
    }
}

impl LogOut for AsicTemps {
    fn log_out(&self, names: &[&str], out: &mut String) {
        named(names, 0, out, &self.cur);
        named(names, 1, out, &self.peak);
    }
}

/// The status name of a result (`SUCCESS`, `NOT_SUPPORTED`, `INVALID_ATTR_VALUE_2`, ...).
pub fn status_name<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => "SUCCESS".to_owned(),
        Err(e) => error_name(*e),
    }
}

/// The SAI status name of an error, without `SAI_STATUS_`.
pub fn error_name(error: Error) -> String {
    let fixed = match error {
        Error::Failure => "FAILURE",
        Error::NotSupported => "NOT_SUPPORTED",
        Error::NotImplemented => "NOT_IMPLEMENTED",
        Error::InsufficientResources => "INSUFFICIENT_RESOURCES",
        Error::InvalidParameter => "INVALID_PARAMETER",
        Error::ItemAlreadyExists => "ITEM_ALREADY_EXISTS",
        Error::ItemNotFound => "ITEM_NOT_FOUND",
        Error::InvalidVlanId => "INVALID_VLAN_ID",
        Error::Uninitialized => "UNINITIALIZED",
        Error::TableFull => "TABLE_FULL",
        Error::MandatoryAttributeMissing => "MANDATORY_ATTRIBUTE_MISSING",
        Error::ObjectInUse => "OBJECT_IN_USE",
        Error::InvalidObjectType => "INVALID_OBJECT_TYPE",
        Error::InvalidObjectId => "INVALID_OBJECT_ID",
        Error::InvalidPortNumber => "INVALID_PORT_NUMBER",
        Error::InvalidPortMember => "INVALID_PORT_MEMBER",
        Error::NoMemory => "NO_MEMORY",
        Error::BufferOverflow => "BUFFER_OVERFLOW",
        Error::AddrNotFound => "ADDR_NOT_FOUND",
        Error::Attr(kind, index) => {
            let base = match kind {
                AttrError::Invalid => "INVALID_ATTRIBUTE",
                AttrError::InvalidValue => "INVALID_ATTR_VALUE",
                AttrError::NotImplemented => "ATTR_NOT_IMPLEMENTED",
                AttrError::Unknown => "UNKNOWN_ATTRIBUTE",
                AttrError::NotSupported => "ATTR_NOT_SUPPORTED",
            };
            return format!("{base}_{index}");
        }
    };
    fixed.to_owned()
}

/// One call line: `op(args) -> STATUS outs` (outputs only on success).
/// `outs` appends the outputs of a successful result.
pub fn call_line<T>(op: &str, args: &str, result: &Result<T>, outs: impl FnOnce(&T, &mut String)) -> String {
    let mut line = format!("{op}({args}) -> {}", status_name(result));
    if let Ok(value) = result {
        outs(value, &mut line);
    }
    line
}

/// `outs` for [`call_line`]: no output.
pub fn no_outs<T>(_value: &T, _out: &mut String) {}

/// One line of a call that returns a value instead of a status: `op(args) -> value`.
pub fn value_line(op: &str, args: &str, value: &dyn LogValue) -> String {
    let mut line = format!("{op}({args}) -> ");
    value.log(&mut line);
    line
}

/// Appends `name=value` to an argument list (`, ` between arguments).
pub fn arg(args: &mut String, name: &str, value: &dyn LogValue) {
    if value.hidden() {
        return;
    }
    if !args.is_empty() {
        args.push_str(", ");
    }
    args.push_str(name);
    args.push('=');
    value.log(args);
}

/// The line of an event the backend raised.
pub fn event_line(event: &BackendEvent) -> String {
    let mut args = String::new();
    match event {
        BackendEvent::Link { hw, up } => {
            arg(&mut args, "hw", hw);
            arg(&mut args, "up", up);
            format!("event link({args})")
        }
        BackendEvent::Fdb(e) => {
            arg(
                &mut args,
                "learned",
                &matches!(e.change, super::types::FdbChange::Learned),
            );
            arg(&mut args, "mac", &e.mac);
            arg(&mut args, "vid", &e.vid);
            arg(&mut args, "hw_port", &e.hw_port);
            format!("event fdb({args})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::types::{FpMatch, Masked};

    fn text(v: &dyn LogValue) -> String {
        let mut s = String::new();
        v.log(&mut s);
        s
    }

    #[test]
    fn values_print_as_the_format_says() {
        assert_eq!(text(&MacAddr([0, 0x11, 0x22, 0x33, 0x44, 0x5f])), "00:11:22:33:44:5f");
        assert_eq!(text(&"a\"b\\c\n\u{7f}"), "\"a\\\"b\\\\c\\x0a\\x7f\"");
        let v6: Ipv6Addr = "2001:db8::1".parse().unwrap();
        assert_eq!(text(&v6), "2001:db8:0:0:0:0:0:1");
        let p = IpPrefix::new(
            IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)),
            IpAddr::V4(Ipv4Addr::new(255, 255, 0, 0)),
        )
        .unwrap();
        assert_eq!(text(&p), "10.1.0.0/16");
        let odd = IpPrefix::new(
            IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)),
            IpAddr::V4(Ipv4Addr::new(255, 0, 255, 0)),
        )
        .unwrap();
        assert_eq!(text(&odd), "10.0.2.0/255.0.255.0");
        let all = IpPrefix::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), IpAddr::V6(Ipv6Addr::UNSPECIFIED)).unwrap();
        assert_eq!(text(&all), "0:0:0:0:0:0:0:0/0");
        let host = IpPrefix::new(IpAddr::V6(v6), IpAddr::V6(Ipv6Addr::from_bits(u128::MAX))).unwrap();
        assert_eq!(text(&host), "2001:db8:0:0:0:0:0:1/128");
        assert_eq!(text(&Dest::Lag(super::super::types::TrunkId(1))), "1073741825");
        assert_eq!(text(&vec![(HwPort(1), true)]), "[(1,true)]");
        assert_eq!(text(&Vec::<u32>::new()), "[]");
        assert_eq!(text(&Option::<super::super::types::PolicerId>::None), "-1");
        assert_eq!(text(&super::super::types::Fec::Rs), "Rs");
        let m = FpMatch {
            in_ports: Some(vec![HwPort(1)]),
            ether_type: Some(Masked {
                value: 0x806,
                mask: 0xffff,
            }),
            ..FpMatch::default()
        };
        assert_eq!(text(&m), "{InPorts=[1],EtherType=0x806/0xffff}");
        assert_eq!(text(&FpMatch::default()), "{}");
        let e = QosMapEntry {
            key: 1,
            key_color: 0,
            value: 2,
            value_color: 0,
        };
        assert_eq!(text(&vec![e]), "[1.0=2.0]");
    }

    #[test]
    fn lines() {
        let mut args = String::new();
        arg(&mut args, "hw", &HwPort(7));
        let ok: Result<bool> = Ok(false);
        assert_eq!(
            call_line("port_link_get", &args, &ok, |v, s| One(v).log_out(&["up"], s)),
            "port_link_get(hw=7) -> SUCCESS up=false"
        );
        let bad: Result<bool> = Err(Error::attr(AttrError::InvalidValue, 2));
        assert_eq!(
            call_line("port_link_get", &args, &bad, |v, s| One(v).log_out(&["up"], s)),
            "port_link_get(hw=7) -> INVALID_ATTR_VALUE_2"
        );
        assert_eq!(
            call_line::<()>("lag_create", "", &Ok(()), no_outs),
            "lag_create() -> SUCCESS"
        );
        assert_eq!(
            event_line(&BackendEvent::Link {
                hw: HwPort(1),
                up: true
            }),
            "event link(hw=1, up=true)"
        );
    }
}

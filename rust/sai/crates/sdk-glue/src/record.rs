//! The call log of a Rust fake chip ([`crate::rust_fake::Chip::record`]): one canonical text line
//! per glue call, as the C caller of `sdk_glue.h` makes it, so a recording C stub of the header can
//! write the same line for the same call (sai-backend-openbcm's glue differential,
//! `tests/cpp/glue_stub.c`, which also replays these lines as its answers).
//!
//! # Line format
//!
//! ```text
//! call      = name "(" arg *(", " arg) ")" " -> " rv *(" " out)      e.g. port_link_get(unit=0, port=1) -> 0 up=1
//! callback  = "cb link(port=" int ", up=" int ")"
//!           | "cb l2(mac=" mac|null ", vid=" int ", port=" int ", learned=" int ")"
//!           | "cb gate(port=" int ") -> " answer
//! injection = "inject " what "(" args ")"                               a test hook of the fake
//! ```
//!
//! * The arguments are the C ones (`sdk_glue.h` order and names), `ob_sdk_` left out of the name:
//!   ints decimal; MDIO addresses, registers and values `0x` hex; MACs `aa:bb:..`; IP address and
//!   mask bytes as the glue reads them (4 bytes, or 16 with `v6`) as hex digits; strings quoted
//!   (`\"`, `\\`, `\xHH` below 0x20 and from 0x7f); buffer capacities as their `*len` / `max`
//!   argument; a file descriptor as `fd`; callback pointers and `ctx` as `set` / `null`; the
//!   structs `ob_fp_rule_t` and `ob_policer_cfg_t` field by field (`{name=value, ...}`, the rule's
//!   `ports` array as `[..]`). A pure output pointer is no argument; an in/out one is
//!   (`netif_filter_set`'s `filter_id`, `l3_ecmp_set`'s `ecmp_id` with `replace`).
//! * `rv`: the return value (`void` for the two void functions). Then every output as
//!   `name=value`, also after a failure (what the out-parameters hold then); arrays as `[..]`
//!   with the entries the count says were filled (`rv` for `ports`, `port_speeds`,
//!   `vlan_members`; `count` for `temperatures`), message buffers as the text up to the NUL.
//! * `ob_sdk_errmsg` is not logged: a text lookup, no state.
//! * A line is written when its call returns, so the lines of what happened during it (callbacks
//!   the fake fired, glue calls made from those callbacks) come before it, indented two blanks
//!   per level ([`Recorder`]).
#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the functions have the glue interface's signatures (crate::glue)"
)]

use std::ffi::{CStr, c_int, c_uint};
use std::fmt::Write as _;
use std::os::fd::BorrowedFd;

use sdk_glue_sys as sys;

use crate::rust_fake::Chip;

/// The lines of one chip and the nesting depth of the call being made.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    lines: Vec<String>,
    depth: usize,
}

/// A call in progress: its depth.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scope(usize);

impl Recorder {
    /// A call starts: its depth.
    pub(crate) fn enter(&mut self) -> Scope {
        let d = self.depth;
        self.depth = self.depth.saturating_add(1);
        Scope(d)
    }

    /// The call of `scope` returned: its line, indented by its depth.
    pub(crate) fn exit(&mut self, scope: Scope, line: &str) {
        self.depth = scope.0;
        let mut l = "  ".repeat(scope.0);
        l.push_str(line);
        self.lines.push(l);
    }

    /// The lines so far, taken.
    pub(crate) fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }
}

/// A string as the line quotes it.
pub(crate) fn quoted(bytes: &[u8]) -> String {
    let mut o = String::from("\"");
    for &c in bytes {
        match c {
            b'"' | b'\\' => {
                o.push('\\');
                o.push(char::from(c));
            }
            0x20..=0x7e => o.push(char::from(c)),
            _ => {
                let _ = write!(o, "\\x{c:02x}");
            }
        }
    }
    o.push('"');
    o
}

fn q(s: &CStr) -> String {
    quoted(s.to_bytes())
}

/// The text of a message buffer: up to the first NUL (all of it without one), quoted.
fn text(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    quoted(buf.get(..end).unwrap_or_default())
}

/// A MAC address.
pub(crate) fn mac(m: &[u8; 6]) -> String {
    m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

/// Bytes as hex digits.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// A list.
fn list<T: std::fmt::Display>(items: impl IntoIterator<Item = T>) -> String {
    let v: Vec<String> = items.into_iter().map(|x| x.to_string()).collect();
    format!("[{}]", v.join(","))
}

fn set(on: bool) -> &'static str {
    if on { "set" } else { "null" }
}

/// A buffer length as the glue's `int` (as `real.rs` passes it).
fn len(n: usize) -> c_int {
    c_int::try_from(n).unwrap_or(c_int::MAX)
}

/// The first `n` entries the count `n` says were filled (a negative count: none).
fn filled<T>(items: &[T], n: c_int) -> &[T] {
    let n = usize::try_from(n).unwrap_or(0).min(items.len());
    items.get(..n).unwrap_or_default()
}

/// `ob_policer_cfg_t`, field by field.
fn policer(c: &sys::ob_policer_cfg_t) -> String {
    format!(
        "{{tr_tcm={}, packets={}, color_blind={}, cir={}, cbs={}, pir={}, pbs={}}}",
        c.tr_tcm, c.packets, c.color_blind, c.cir, c.cbs, c.pir, c.pbs
    )
}

/// `ob_fp_rule_t`, field by field (`ports`: the `nports` entries it points to).
fn rule(r: &crate::field::RawRule<'_>) -> String {
    let c = r.raw();
    let ports = filled(r.ports(), c.nports);
    format!(
        "{{present=0x{:x}, ether={}, ether_mask={}, ip_type={}, proto={}, proto_mask={}, src={}, src_mask={}, \
         dst={}, dst_mask={}, l4_src={}, l4_src_mask={}, l4_dst={}, l4_dst_mask={}, tcp={}, tcp_mask={}, \
         icmp={}, icmp_mask={}, tos={}, tos_mask={}, dmac={}, dmac_mask={}, smac={}, smac_mask={}, vlan={}, \
         vlan_mask={}, ttl={}, ttl_mask={}, dst_egress={}, ports={}, nports={}, priority={}, drop={}, \
         drop_cancel={}, copy_cpu={}, counter={}, policer={}, green_drop={}, yellow_drop={}, red_drop={}, \
         cpu_copy_only={}}}",
        c.present,
        c.ether,
        c.ether_mask,
        c.ip_type,
        c.proto,
        c.proto_mask,
        hex(&c.src),
        hex(&c.src_mask),
        hex(&c.dst),
        hex(&c.dst_mask),
        c.l4_src,
        c.l4_src_mask,
        c.l4_dst,
        c.l4_dst_mask,
        c.tcp,
        c.tcp_mask,
        c.icmp,
        c.icmp_mask,
        c.tos,
        c.tos_mask,
        mac(&c.dmac),
        mac(&c.dmac_mask),
        mac(&c.smac),
        mac(&c.smac_mask),
        c.vlan,
        c.vlan_mask,
        c.ttl,
        c.ttl_mask,
        c.dst_egress,
        list(ports),
        c.nports,
        c.priority,
        c.drop,
        c.drop_cancel,
        c.copy_cpu,
        c.counter,
        c.policer,
        c.green_drop,
        c.yellow_drop,
        c.red_drop,
        c.cpu_copy_only
    )
}

fn port_info(p: &sys::ob_port_info_t) -> String {
    let name: Vec<u8> = p
        .name
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
        .collect();
    format!(
        "{{port={}, phys={}, lanes={}, speed={}, name={}}}",
        p.port,
        p.phys,
        p.lanes,
        p.speed,
        quoted(&name)
    )
}

/// One recorded glue call: `$input` formatted before it, `$out` after (may name `$rv`).
macro_rules! rec {
    ($chip:ident, $call:expr, [$($input:tt)*], |$rv:ident| [$($out:tt)*]) => {{
        let Some(scope) = $chip.rec_enter() else {
            return $call;
        };
        let input = format!($($input)*);
        let $rv = $call;
        let out = format!($($out)*);
        $chip.rec_exit(scope, &format!("{input} -> {}{out}", $rv));
        $rv
    }};
}

/// The same for a void function.
macro_rules! rec_void {
    ($chip:ident, $call:expr, [$($input:tt)*]) => {{
        let Some(scope) = $chip.rec_enter() else {
            return $call;
        };
        let input = format!($($input)*);
        $call;
        $chip.rec_exit(scope, &format!("{input} -> void"));
    }};
}

pub(crate) fn init(chip: &Chip, unit: c_int, config: &CStr, rc: &CStr, err: &mut [u8]) -> c_int {
    let errlen = len(err.len());
    rec!(
        chip,
        chip.init(unit, config, rc, err),
        ["init(unit={unit}, config={}, rc={}, errlen={errlen})", q(config), q(rc)],
        |rv| [" err={}", text(err)]
    )
}

pub(crate) fn attach(chip: &Chip, unit: c_int, err: &mut [u8]) -> c_int {
    let errlen = len(err.len());
    rec!(
        chip,
        chip.attach(unit, err),
        ["attach(unit={unit}, errlen={errlen})"],
        |rv| [" err={}", text(err)]
    )
}

pub(crate) fn init_mdio(chip: &Chip, unit: c_int, config: &CStr, err: &mut [u8]) -> c_int {
    let errlen = len(err.len());
    rec!(
        chip,
        chip.init_mdio(unit, config, err),
        ["init_mdio(unit={unit}, config={}, errlen={errlen})", q(config)],
        |rv| [" err={}", text(err)]
    )
}

pub(crate) fn temperatures(
    chip: &Chip,
    unit: c_int,
    cur: &mut [c_int],
    peak: &mut [c_int],
    count: &mut c_int,
) -> c_int {
    let max = len(cur.len().min(peak.len()));
    rec!(
        chip,
        chip.temperatures(unit, cur, peak, count),
        ["temperatures(unit={unit}, max={max})"],
        |rv| [
            " count={count} cur={} peak={}",
            list(filled(cur, (*count).min(max))),
            list(filled(peak, (*count).min(max)))
        ]
    )
}

pub(crate) fn th_overtemp_shutdown(chip: &Chip, unit: c_int) -> c_int {
    rec!(
        chip,
        chip.th_overtemp_shutdown(unit),
        ["th_overtemp_shutdown(unit={unit})"],
        |rv| [""]
    )
}

pub(crate) fn mdio_read(chip: &Chip, unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: &mut c_uint) -> c_int {
    rec!(
        chip,
        chip.mdio_read(unit, phy, dev, reg, val),
        ["mdio_c45_read(unit={unit}, phy=0x{phy:x}, dev={dev}, reg=0x{reg:x})"],
        |rv| [" val=0x{:x}", *val]
    )
}

pub(crate) fn mdio_write(chip: &Chip, unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: c_uint) -> c_int {
    rec!(
        chip,
        chip.mdio_write(unit, phy, dev, reg, val),
        ["mdio_c45_write(unit={unit}, phy=0x{phy:x}, dev={dev}, reg=0x{reg:x}, val=0x{val:x})"],
        |rv| [""]
    )
}

pub(crate) fn linkscan_port_gate(chip: &Chip, unit: c_int, port: c_int, on: bool, key: usize) -> c_int {
    rec!(
        chip,
        chip.linkscan_port_gate(unit, port, on, key),
        [
            "linkscan_port_gate(unit={unit}, port={port}, gate={}, ctx={})",
            set(on),
            set(key != 0)
        ],
        |rv| [""]
    )
}

pub(crate) fn shutdown(chip: &Chip, unit: c_int) {
    rec_void!(chip, chip.shutdown(unit), ["shutdown(unit={unit})"]);
}

pub(crate) fn shell(chip: &Chip, unit: c_int, cmd: &CStr) -> c_int {
    rec!(
        chip,
        chip.shell(unit, cmd),
        ["shell(unit={unit}, cmd={})", q(cmd)],
        |rv| [""]
    )
}

pub(crate) fn shell_to(chip: &Chip, unit: c_int, cmd: &CStr, fd: BorrowedFd<'_>) -> c_int {
    rec!(
        chip,
        chip.shell_to(unit, cmd, fd),
        ["shell_to(unit={unit}, cmd={}, fd)", q(cmd)],
        |rv| [""]
    )
}

pub(crate) fn register_callbacks(chip: &Chip, unit: c_int, link: bool, l2: bool, key: usize) -> c_int {
    rec!(
        chip,
        chip.register_callbacks(unit, link, l2, key),
        [
            "register_callbacks(unit={unit}, link={}, l2={}, ctx={})",
            set(link),
            set(l2),
            set(key != 0)
        ],
        |rv| [""]
    )
}

pub(crate) fn unregister_callbacks(chip: &Chip, unit: c_int) {
    rec_void!(
        chip,
        chip.unregister_callbacks(unit),
        ["unregister_callbacks(unit={unit})"]
    );
}

pub(crate) fn hw_info(chip: &Chip, unit: c_int, buf: &mut [u8]) -> c_int {
    let n = len(buf.len());
    rec!(chip, chip.hw_info(unit, buf), ["hw_info(unit={unit}, len={n})"], |rv| [
        " buf={}",
        text(buf)
    ])
}

pub(crate) fn knet_cleanup(chip: &Chip, unit: c_int, netifs: &mut c_int, filters: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.knet_cleanup(unit, netifs, filters),
        ["knet_cleanup(unit={unit})"],
        |rv| [" netifs={netifs} filters={filters}"]
    )
}

pub(crate) fn table_usage(chip: &Chip, unit: c_int, u: &mut sys::ob_table_usage_t) -> c_int {
    rec!(chip, chip.table_usage(unit, u), ["table_usage(unit={unit})"], |rv| [
        " u={{v4_routes_free={}, v6_routes_free={}, hosts_free={}, egress_free={}, fdb_free={}, \
             ecmp_groups_free={}, ecmp_members_free={}}}",
        u.v4_routes_free,
        u.v6_routes_free,
        u.hosts_free,
        u.egress_free,
        u.fdb_free,
        u.ecmp_groups_free,
        u.ecmp_members_free
    ])
}

pub(crate) fn chip_limits(chip: &Chip, unit: c_int, l: &mut sys::ob_chip_limits_t) -> c_int {
    rec!(chip, chip.chip_limits(unit, l), ["chip_limits(unit={unit})"], |rv| [
        " l={{buffer_bytes={}, acl_prio_min={}, acl_prio_max={}, max_host={}, max_route={}, max_nexthop={}, \
             max_l2={}, ecmp_groups={}, ecmp_paths={}, ecmp_members={}, vrf_max={}}}",
        l.buffer_bytes,
        l.acl_prio_min,
        l.acl_prio_max,
        l.max_host,
        l.max_route,
        l.max_nexthop,
        l.max_l2,
        l.ecmp_groups,
        l.ecmp_paths,
        l.ecmp_members,
        l.vrf_max
    ])
}

pub(crate) fn cpu_port(chip: &Chip, unit: c_int) -> c_int {
    rec!(chip, chip.cpu_port(unit), ["cpu_port(unit={unit})"], |rv| [""])
}

pub(crate) fn ports(chip: &Chip, unit: c_int, out: &mut [sys::ob_port_info_t]) -> c_int {
    let max = len(out.len());
    rec!(chip, chip.ports(unit, out), ["ports(unit={unit}, max={max})"], |rv| [
        " out={}",
        list(filled(out, rv).iter().map(port_info))
    ])
}

pub(crate) fn port_enable_set(chip: &Chip, unit: c_int, port: c_int, enable: c_int) -> c_int {
    rec!(
        chip,
        chip.port_enable_set(unit, port, enable),
        ["port_enable_set(unit={unit}, port={port}, enable={enable})"],
        |rv| [""]
    )
}

pub(crate) fn port_link_get(chip: &Chip, unit: c_int, port: c_int, up: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.port_link_get(unit, port, up),
        ["port_link_get(unit={unit}, port={port})"],
        |rv| [" up={up}"]
    )
}

pub(crate) fn port_speed_set(chip: &Chip, unit: c_int, port: c_int, mbps: c_int) -> c_int {
    rec!(
        chip,
        chip.port_speed_set(unit, port, mbps),
        ["port_speed_set(unit={unit}, port={port}, mbps={mbps})"],
        |rv| [""]
    )
}

pub(crate) fn port_speed_get(chip: &Chip, unit: c_int, port: c_int, mbps: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.port_speed_get(unit, port, mbps),
        ["port_speed_get(unit={unit}, port={port})"],
        |rv| [" mbps={mbps}"]
    )
}

pub(crate) fn port_speeds(chip: &Chip, unit: c_int, port: c_int, out: &mut [u32]) -> c_int {
    let max = len(out.len());
    rec!(
        chip,
        chip.port_speeds(unit, port, out),
        ["port_speeds(unit={unit}, port={port}, max={max})"],
        |rv| [" out={}", list(filled(out, rv))]
    )
}

/// A setter of one int of a port.
macro_rules! port_int {
    ($($name:ident($arg:ident);)*) => {$(
        pub(crate) fn $name(chip: &Chip, unit: c_int, port: c_int, $arg: c_int) -> c_int {
            rec!(
                chip,
                chip.$name(unit, port, $arg),
                [concat!(stringify!($name), "(unit={}, port={}, ", stringify!($arg), "={})"), unit, port, $arg],
                |rv| [""]
            )
        }
    )*};
}

port_int! {
    port_frame_max_set(bytes);
    port_fec_set(mode);
    port_autoneg_set(on);
    port_ifg_set(bits);
    port_loopback_set(mode);
    port_pvid_set(vid);
    port_learn_set(mode);
}

pub(crate) fn port_autoneg_get(chip: &Chip, unit: c_int, port: c_int, on: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.port_autoneg_get(unit, port, on),
        ["port_autoneg_get(unit={unit}, port={port})"],
        |rv| [" on={on}"]
    )
}

pub(crate) fn port_interface_set(chip: &Chip, unit: c_int, port: c_int, name: &CStr) -> c_int {
    rec!(
        chip,
        chip.port_interface_set(unit, port, name),
        ["port_interface_set(unit={unit}, port={port}, name={})", q(name)],
        |rv| [""]
    )
}

pub(crate) fn port_stat_get(chip: &Chip, unit: c_int, port: c_int, which: c_int, v: &mut u64) -> c_int {
    rec!(
        chip,
        chip.port_stat_get(unit, port, which, v),
        ["port_stat_get(unit={unit}, port={port}, which={which})"],
        |rv| [" v={v}"]
    )
}

pub(crate) fn port_stat_clear(chip: &Chip, unit: c_int, port: c_int) -> c_int {
    rec!(
        chip,
        chip.port_stat_clear(unit, port),
        ["port_stat_clear(unit={unit}, port={port})"],
        |rv| [""]
    )
}

pub(crate) fn vlan_create(chip: &Chip, unit: c_int, vid: c_int) -> c_int {
    rec!(
        chip,
        chip.vlan_create(unit, vid),
        ["vlan_create(unit={unit}, vid={vid})"],
        |rv| [""]
    )
}

pub(crate) fn vlan_destroy(chip: &Chip, unit: c_int, vid: c_int) -> c_int {
    rec!(
        chip,
        chip.vlan_destroy(unit, vid),
        ["vlan_destroy(unit={unit}, vid={vid})"],
        |rv| [""]
    )
}

pub(crate) fn vlan_port_add(chip: &Chip, unit: c_int, vid: c_int, port: c_int, untagged: c_int) -> c_int {
    rec!(
        chip,
        chip.vlan_port_add(unit, vid, port, untagged),
        ["vlan_port_add(unit={unit}, vid={vid}, port={port}, untagged={untagged})"],
        |rv| [""]
    )
}

pub(crate) fn vlan_port_remove(chip: &Chip, unit: c_int, vid: c_int, port: c_int) -> c_int {
    rec!(
        chip,
        chip.vlan_port_remove(unit, vid, port),
        ["vlan_port_remove(unit={unit}, vid={vid}, port={port})"],
        |rv| [""]
    )
}

pub(crate) fn vlan_members(chip: &Chip, unit: c_int, vid: c_int, ports: &mut [c_int], untagged: &mut [c_int]) -> c_int {
    let max = len(ports.len().min(untagged.len()));
    rec!(
        chip,
        chip.vlan_members(unit, vid, ports, untagged),
        ["vlan_members(unit={unit}, vid={vid}, max={max})"],
        |rv| [
            " ports={} untagged={}",
            list(filled(ports, rv.min(max))),
            list(filled(untagged, rv.min(max)))
        ]
    )
}

pub(crate) fn l2_add(
    chip: &Chip,
    unit: c_int,
    m: &[u8; 6],
    vid: c_int,
    port: c_int,
    is_static: c_int,
    discard: c_int,
) -> c_int {
    rec!(
        chip,
        chip.l2_add(unit, m, vid, port, is_static, discard),
        [
            "l2_add(unit={unit}, mac={}, vid={vid}, port={port}, is_static={is_static}, discard={discard})",
            mac(m)
        ],
        |rv| [""]
    )
}

pub(crate) fn l2_delete(chip: &Chip, unit: c_int, m: &[u8; 6], vid: c_int) -> c_int {
    rec!(
        chip,
        chip.l2_delete(unit, m, vid),
        ["l2_delete(unit={unit}, mac={}, vid={vid})", mac(m)],
        |rv| [""]
    )
}

pub(crate) fn l2_flush(chip: &Chip, unit: c_int, port: c_int, vid: c_int, statics: c_int, dynamics: c_int) -> c_int {
    rec!(
        chip,
        chip.l2_flush(unit, port, vid, statics, dynamics),
        ["l2_flush(unit={unit}, port={port}, vid={vid}, statics={statics}, dynamics={dynamics})"],
        |rv| [""]
    )
}

pub(crate) fn l2_age_set(chip: &Chip, unit: c_int, seconds: c_int) -> c_int {
    rec!(
        chip,
        chip.l2_age_set(unit, seconds),
        ["l2_age_set(unit={unit}, seconds={seconds})"],
        |rv| [""]
    )
}

#[allow(clippy::too_many_arguments, reason = "the glue's signature")]
pub(crate) fn netif_create(
    chip: &Chip,
    unit: c_int,
    name: &CStr,
    port: c_int,
    vid: c_int,
    m: &[u8; 6],
    keep_tag: c_int,
    netif_id: &mut c_int,
    filter_id: &mut c_int,
) -> c_int {
    rec!(
        chip,
        chip.netif_create(unit, name, port, vid, m, keep_tag, netif_id, filter_id),
        [
            "netif_create(unit={unit}, name={}, port={port}, vid={vid}, mac={}, keep_tag={keep_tag})",
            q(name),
            mac(m)
        ],
        |rv| [" netif_id={netif_id} filter_id={filter_id}"]
    )
}

pub(crate) fn netif_destroy(chip: &Chip, unit: c_int, netif_id: c_int, filter_id: c_int) -> c_int {
    rec!(
        chip,
        chip.netif_destroy(unit, netif_id, filter_id),
        ["netif_destroy(unit={unit}, netif_id={netif_id}, filter_id={filter_id})"],
        |rv| [""]
    )
}

pub(crate) fn netif_filter_set(
    chip: &Chip,
    unit: c_int,
    netif_id: c_int,
    port: c_int,
    keep_tag: c_int,
    filter_id: &mut c_int,
) -> c_int {
    rec!(
        chip,
        chip.netif_filter_set(unit, netif_id, port, keep_tag, filter_id),
        [
            "netif_filter_set(unit={unit}, netif_id={netif_id}, port={port}, keep_tag={keep_tag}, filter_id={})",
            *filter_id
        ],
        |rv| [" filter_id={filter_id}"]
    )
}

pub(crate) fn trap_set(chip: &Chip, unit: c_int, kind: c_int, to_cpu: c_int) -> c_int {
    rec!(
        chip,
        chip.trap_set(unit, kind, to_cpu),
        ["trap_set(unit={unit}, kind={kind}, to_cpu={to_cpu})"],
        |rv| [""]
    )
}

pub(crate) fn trunk_create(chip: &Chip, unit: c_int, tid: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.trunk_create(unit, tid),
        ["trunk_create(unit={unit})"],
        |rv| [" tid={tid}"]
    )
}

pub(crate) fn trunk_destroy(chip: &Chip, unit: c_int, tid: c_int) -> c_int {
    rec!(
        chip,
        chip.trunk_destroy(unit, tid),
        ["trunk_destroy(unit={unit}, tid={tid})"],
        |rv| [""]
    )
}

pub(crate) fn trunk_set(chip: &Chip, unit: c_int, tid: c_int, ports: &[c_int], flags: &[c_int]) -> c_int {
    let n = len(ports.len().min(flags.len()));
    rec!(
        chip,
        chip.trunk_set(unit, tid, ports, flags),
        [
            "trunk_set(unit={unit}, tid={tid}, ports={}, flags={}, n={n})",
            list(filled(ports, n)),
            list(filled(flags, n))
        ],
        |rv| [""]
    )
}

pub(crate) fn l3_intf_create(
    chip: &Chip,
    unit: c_int,
    m: &[u8; 6],
    vid: c_int,
    mtu: c_int,
    vrf: c_int,
    id: &mut c_int,
) -> c_int {
    rec!(
        chip,
        chip.l3_intf_create(unit, m, vid, mtu, vrf, id),
        [
            "l3_intf_create(unit={unit}, mac={}, vid={vid}, mtu={mtu}, vrf={vrf})",
            mac(m)
        ],
        |rv| [" intf_id={id}"]
    )
}

pub(crate) fn l3_intf_update(
    chip: &Chip,
    unit: c_int,
    id: c_int,
    m: &[u8; 6],
    vid: c_int,
    mtu: c_int,
    vrf: c_int,
) -> c_int {
    rec!(
        chip,
        chip.l3_intf_update(unit, id, m, vid, mtu, vrf),
        [
            "l3_intf_update(unit={unit}, intf_id={id}, mac={}, vid={vid}, mtu={mtu}, vrf={vrf})",
            mac(m)
        ],
        |rv| [""]
    )
}

pub(crate) fn l3_intf_destroy(chip: &Chip, unit: c_int, id: c_int) -> c_int {
    rec!(
        chip,
        chip.l3_intf_destroy(unit, id),
        ["l3_intf_destroy(unit={unit}, intf_id={id})"],
        |rv| [""]
    )
}

pub(crate) fn l3_station_add(chip: &Chip, unit: c_int, m: &[u8; 6], id: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.l3_station_add(unit, m, id),
        ["l3_station_add(unit={unit}, mac={})", mac(m)],
        |rv| [" station_id={id}"]
    )
}

pub(crate) fn l3_station_delete(chip: &Chip, unit: c_int, id: c_int) -> c_int {
    rec!(
        chip,
        chip.l3_station_delete(unit, id),
        ["l3_station_delete(unit={unit}, station_id={id})"],
        |rv| [""]
    )
}

pub(crate) fn l3_egress_create(
    chip: &Chip,
    unit: c_int,
    intf: c_int,
    m: &[u8; 6],
    port: c_int,
    vid: c_int,
    id: &mut c_int,
) -> c_int {
    rec!(
        chip,
        chip.l3_egress_create(unit, intf, m, port, vid, id),
        [
            "l3_egress_create(unit={unit}, intf={intf}, mac={}, port={port}, vid={vid})",
            mac(m)
        ],
        |rv| [" egress_id={id}"]
    )
}

pub(crate) fn l3_egress_update(
    chip: &Chip,
    unit: c_int,
    id: c_int,
    intf: c_int,
    m: &[u8; 6],
    port: c_int,
    vid: c_int,
) -> c_int {
    rec!(
        chip,
        chip.l3_egress_update(unit, id, intf, m, port, vid),
        [
            "l3_egress_update(unit={unit}, egress_id={id}, intf={intf}, mac={}, port={port}, vid={vid})",
            mac(m)
        ],
        |rv| [""]
    )
}

pub(crate) fn l3_egress_special(chip: &Chip, unit: c_int, drop: c_int, id: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.l3_egress_special(unit, drop, id),
        ["l3_egress_special(unit={unit}, drop={drop})"],
        |rv| [" egress_id={id}"]
    )
}

pub(crate) fn l3_egress_destroy(chip: &Chip, unit: c_int, id: c_int) -> c_int {
    rec!(
        chip,
        chip.l3_egress_destroy(unit, id),
        ["l3_egress_destroy(unit={unit}, egress_id={id})"],
        |rv| [""]
    )
}

fn v6(addr: &[u8]) -> c_int {
    c_int::from(addr.len() == 16)
}

pub(crate) fn l3_host_add(chip: &Chip, unit: c_int, vrf: c_int, addr: &[u8], egress: c_int) -> c_int {
    rec!(
        chip,
        chip.l3_host_add(unit, vrf, addr, egress),
        [
            "l3_host_add(unit={unit}, vrf={vrf}, v6={}, addr={}, egress_id={egress})",
            v6(addr),
            hex(addr)
        ],
        |rv| [""]
    )
}

pub(crate) fn l3_host_delete(chip: &Chip, unit: c_int, vrf: c_int, addr: &[u8]) -> c_int {
    rec!(
        chip,
        chip.l3_host_delete(unit, vrf, addr),
        [
            "l3_host_delete(unit={unit}, vrf={vrf}, v6={}, addr={})",
            v6(addr),
            hex(addr)
        ],
        |rv| [""]
    )
}

#[allow(clippy::too_many_arguments, reason = "the glue's signature")]
pub(crate) fn l3_route_add(
    chip: &Chip,
    unit: c_int,
    vrf: c_int,
    addr: &[u8],
    mask: &[u8],
    egress: c_int,
    ecmp: c_int,
    drop: c_int,
    replace: c_int,
) -> c_int {
    rec!(
        chip,
        chip.l3_route_add(unit, vrf, addr, mask, egress, ecmp, drop, replace),
        [
            "l3_route_add(unit={unit}, vrf={vrf}, v6={}, addr={}, mask={}, egress_id={egress}, ecmp={ecmp}, \
             drop={drop}, replace={replace})",
            v6(addr),
            hex(addr),
            hex(mask)
        ],
        |rv| [""]
    )
}

pub(crate) fn l3_route_delete(chip: &Chip, unit: c_int, vrf: c_int, addr: &[u8], mask: &[u8]) -> c_int {
    rec!(
        chip,
        chip.l3_route_delete(unit, vrf, addr, mask),
        [
            "l3_route_delete(unit={unit}, vrf={vrf}, v6={}, addr={}, mask={})",
            v6(addr),
            hex(addr),
            hex(mask)
        ],
        |rv| [""]
    )
}

pub(crate) fn l3_ecmp_set(chip: &Chip, unit: c_int, id: &mut c_int, replace: c_int, egress: &[c_int]) -> c_int {
    let n = len(egress.len());
    // The id is read only with replace (an output otherwise).
    let current = if replace == 0 {
        String::new()
    } else {
        format!("ecmp_id={}, ", *id)
    };
    rec!(
        chip,
        chip.l3_ecmp_set(unit, id, replace, egress),
        [
            "l3_ecmp_set(unit={unit}, {current}replace={replace}, egress={}, n={n})",
            list(egress)
        ],
        |rv| [" ecmp_id={id}"]
    )
}

pub(crate) fn l3_ecmp_destroy(chip: &Chip, unit: c_int, id: c_int) -> c_int {
    rec!(
        chip,
        chip.l3_ecmp_destroy(unit, id),
        ["l3_ecmp_destroy(unit={unit}, ecmp_id={id})"],
        |rv| [""]
    )
}

pub(crate) fn fp_group_create(chip: &Chip, unit: c_int, qset: u32, priority: c_int, gid: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.fp_group_create(unit, qset, priority, gid),
        ["fp_group_create(unit={unit}, qset=0x{qset:x}, priority={priority})"],
        |rv| [" gid={gid}"]
    )
}

pub(crate) fn fp_group_destroy(chip: &Chip, unit: c_int, gid: c_int) -> c_int {
    rec!(
        chip,
        chip.fp_group_destroy(unit, gid),
        ["fp_group_destroy(unit={unit}, gid={gid})"],
        |rv| [""]
    )
}

pub(crate) fn fp_entry_create(
    chip: &Chip,
    unit: c_int,
    gid: c_int,
    r: &crate::field::RawRule<'_>,
    eid: &mut c_int,
    stat: &mut c_int,
) -> c_int {
    rec!(
        chip,
        chip.fp_entry_create(unit, gid, r, eid, stat),
        ["fp_entry_create(unit={unit}, gid={gid}, r={})", rule(r)],
        |rv| [" eid={eid} stat_id={stat}"]
    )
}

pub(crate) fn fp_entry_destroy(chip: &Chip, unit: c_int, eid: c_int, stat: c_int) -> c_int {
    rec!(
        chip,
        chip.fp_entry_destroy(unit, eid, stat),
        ["fp_entry_destroy(unit={unit}, eid={eid}, stat_id={stat})"],
        |rv| [""]
    )
}

pub(crate) fn fp_stat_get(chip: &Chip, unit: c_int, stat: c_int, packets: &mut u64, bytes: &mut u64) -> c_int {
    rec!(
        chip,
        chip.fp_stat_get(unit, stat, packets, bytes),
        ["fp_stat_get(unit={unit}, stat_id={stat})"],
        |rv| [" packets={packets} bytes={bytes}"]
    )
}

pub(crate) fn policer_create(chip: &Chip, unit: c_int, cfg: &sys::ob_policer_cfg_t, pid: &mut c_int) -> c_int {
    rec!(
        chip,
        chip.policer_create(unit, cfg, pid),
        ["policer_create(unit={unit}, c={})", policer(cfg)],
        |rv| [" pid={pid}"]
    )
}

pub(crate) fn policer_set(chip: &Chip, unit: c_int, pid: c_int, cfg: &sys::ob_policer_cfg_t) -> c_int {
    rec!(
        chip,
        chip.policer_set(unit, pid, cfg),
        ["policer_set(unit={unit}, pid={pid}, c={})", policer(cfg)],
        |rv| [""]
    )
}

pub(crate) fn policer_destroy(chip: &Chip, unit: c_int, pid: c_int) -> c_int {
    rec!(
        chip,
        chip.policer_destroy(unit, pid),
        ["policer_destroy(unit={unit}, pid={pid})"],
        |rv| [""]
    )
}

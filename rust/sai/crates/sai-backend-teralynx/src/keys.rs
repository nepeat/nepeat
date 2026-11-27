//! The backend's profile keys (this crate's README "Profile keys"), read as the C++ read them:
//! `Core::key(k, default)` is the profile's value when the key is set (even empty), else the
//! default; numbers go through the C library's `strtoul` / `strtoull` (base 0: `0x` hex, a leading
//! `0` octal, stopping at the first character that does not fit, no digits = 0) or `atoi`, as the
//! C++ does, so a value parses here exactly as it did there. Every key of the table is read and
//! stored at init ([`Settings`]), also those of areas not ported yet, so nothing is silently
//! ignored when they are.
#![forbid(unsafe_code)]

use sai_core::backend::Profile;

/// `SAI_INIT_CONFIG_FILE`: the generated IFCS init yaml (required).
pub const INIT_CONFIG_FILE: &str = "SAI_INIT_CONFIG_FILE";
/// `SAI_TERALYNX_CONFIG_APPEND`: appended at run time (the vendor isg yaml).
pub const CONFIG_APPEND: &str = "SAI_TERALYNX_CONFIG_APPEND";
/// `SAI_TERALYNX_RUN_DIR`: where the merged `config.yaml` (and the guard's `.prom`) go.
pub const RUN_DIR: &str = "SAI_TERALYNX_RUN_DIR";
/// `SAI_TERALYNX_LIBIFCS`: absolute path of the vendor `libifcs.so` (required).
pub const LIBIFCS: &str = "SAI_TERALYNX_LIBIFCS";
/// `SAI_TERALYNX_PORT_MAP`: `devport lanes speed_mbps fec` per front-panel devport (required).
pub const PORT_MAP: &str = "SAI_TERALYNX_PORT_MAP";
/// `SAI_TERALYNX_INIT_CMDS`: `:`-separated init command files.
pub const INIT_CMDS: &str = "SAI_TERALYNX_INIT_CMDS";
/// `SAI_TERALYNX_NETDEV`: `adopt` (default) / `create`.
pub const NETDEV: &str = "SAI_TERALYNX_NETDEV";
/// `SAI_TERALYNX_L3INTF_STATS`: `off` skips the RIF counter node setup.
pub const L3INTF_STATS: &str = "SAI_TERALYNX_L3INTF_STATS";
/// `SAI_TERALYNX_L3INTF_STATS_MODES`: modes tried in order (`4,1,2,3`).
pub const L3INTF_STATS_MODES: &str = "SAI_TERALYNX_L3INTF_STATS_MODES";
/// `SAI_TERALYNX_L3INTF_STATS_COUNTERS`: flex counters per direction (4096).
pub const L3INTF_STATS_COUNTERS: &str = "SAI_TERALYNX_L3INTF_STATS_COUNTERS";
/// `SAI_TERALYNX_SYSTEM_TRAPS`: `isai` (default) / `sdk`.
pub const SYSTEM_TRAPS: &str = "SAI_TERALYNX_SYSTEM_TRAPS";
/// `SAI_TERALYNX_SFLOW_CPU_QUEUE`: the samples' CPU queue when the node does not say.
pub const SFLOW_CPU_QUEUE: &str = "SAI_TERALYNX_SFLOW_CPU_QUEUE";
/// `SAI_TERALYNX_SFLOW_METER`: `on` meters the samples' queue (default `off`).
pub const SFLOW_METER: &str = "SAI_TERALYNX_SFLOW_METER";
/// `SAI_TERALYNX_EGRESS_SFLOW`: `on` sets node `EGRESS_SFLOW_ENABLE` before enable.
pub const EGRESS_SFLOW: &str = "SAI_TERALYNX_EGRESS_SFLOW";
/// `SAI_TERALYNX_IPD_DEV`: the ipd device libifcs opened (default `/dev/ipd` or `/dev/ipd0`).
pub const IPD_DEV: &str = "SAI_TERALYNX_IPD_DEV";
/// `SAI_TERALYNX_SHUTDOWN`: `deinit` (default) / `ports-down`.
pub const SHUTDOWN: &str = "SAI_TERALYNX_SHUTDOWN";
/// `SAI_TERALYNX_CPU_FLOOD`: `on` puts the CPU in every VLAN (default `off`).
pub const CPU_FLOOD: &str = "SAI_TERALYNX_CPU_FLOOD";
/// `SAI_TERALYNX_LAG_FLOOD`: `off` skips `MULTICAST_LAG_RESOLUTION_ENABLE` (default `on`).
pub const LAG_FLOOD: &str = "SAI_TERALYNX_LAG_FLOOD";
/// `SAI_TERALYNX_HASH_FIELDS`: the LAG / ECMP hash fields at init.
pub const HASH_FIELDS: &str = "SAI_TERALYNX_HASH_FIELDS";
/// `SAI_TERALYNX_HASH_SEED`: node `ENTROPY_SEED` at init.
pub const HASH_SEED: &str = "SAI_TERALYNX_HASH_SEED";
/// `SAI_TERALYNX_IFCS_LOG_NAME`: `ifcs_init`'s log name (`sai-teralynx`).
pub const IFCS_LOG_NAME: &str = "SAI_TERALYNX_IFCS_LOG_NAME";
/// `SAI_TERALYNX_LINK_STATUS`: `read` (default) / `cached`.
pub const LINK_STATUS: &str = "SAI_TERALYNX_LINK_STATUS";
/// `SAI_TERALYNX_STATS`: `ids` (default) / `all`.
pub const STATS: &str = "SAI_TERALYNX_STATS";
/// `SAI_TERALYNX_TEMP_PERIOD`: seconds between temperature reads (15; 0 = every call).
pub const TEMP_PERIOD: &str = "SAI_TERALYNX_TEMP_PERIOD";
/// `SAI_TERALYNX_TEMP_MONITORS`: `all` (default) / `master`.
pub const TEMP_MONITORS: &str = "SAI_TERALYNX_TEMP_MONITORS";
/// `SAI_TERALYNX_IFCS_GUARD`: `on` (default) / `off`.
pub const IFCS_GUARD: &str = "SAI_TERALYNX_IFCS_GUARD";
/// `SAI_TERALYNX_LEARN_RING_SIZE`: the guard's ring size (2048).
pub const LEARN_RING_SIZE: &str = "SAI_TERALYNX_LEARN_RING_SIZE";
/// `SAI_TERALYNX_IM_MALLOC_LIMIT_MIB`: the guard's `im_malloc` limit (1024; 0 = none).
pub const IM_MALLOC_LIMIT_MIB: &str = "SAI_TERALYNX_IM_MALLOC_LIMIT_MIB";
/// `SAI_TERALYNX_POST_INIT_REGS`: a file of BAR0 register writes applied after node enable on every
/// init (board data: the CS8210's LED stream setup), `offset mask value` per line ([`crate::config::parse_regs`]).
pub const POST_INIT_REGS: &str = "SAI_TERALYNX_POST_INIT_REGS";
/// `SAI_TERALYNX_DIAG_REGS`: a file of BAR0 registers (offsets, `first-last` ranges) the diag shell's
/// `regs` reads, read only ([`crate::config::parse_diag_regs`]).
pub const DIAG_REGS: &str = "SAI_TERALYNX_DIAG_REGS";
/// `SAI_TERALYNX_IFCS_TRACE`: the call tracer library (oracle).
pub const IFCS_TRACE: &str = "SAI_TERALYNX_IFCS_TRACE";

/// Every key this backend reads (the rim asks the SAI user for each, besides the frontend's:
/// `SAI_OPENBCM_BACKEND`, `_TEMP_FILE`, `_TEMP_PROM`, `_DIAG_SOCKET` are `sai_core`'s).
/// `SAI_TERALYNX_IFCS_GUARD_INJECT` is the shim's (an environment variable, not a key).
pub const PROFILE_KEYS: [&str; 31] = [
    INIT_CONFIG_FILE,
    CONFIG_APPEND,
    RUN_DIR,
    LIBIFCS,
    PORT_MAP,
    INIT_CMDS,
    NETDEV,
    L3INTF_STATS,
    L3INTF_STATS_MODES,
    L3INTF_STATS_COUNTERS,
    SYSTEM_TRAPS,
    SFLOW_CPU_QUEUE,
    SFLOW_METER,
    EGRESS_SFLOW,
    IPD_DEV,
    SHUTDOWN,
    CPU_FLOOD,
    LAG_FLOOD,
    HASH_FIELDS,
    HASH_SEED,
    IFCS_LOG_NAME,
    LINK_STATUS,
    STATS,
    TEMP_PERIOD,
    TEMP_MONITORS,
    IFCS_GUARD,
    LEARN_RING_SIZE,
    IM_MALLOC_LIMIT_MIB,
    IFCS_TRACE,
    POST_INIT_REGS,
    DIAG_REGS,
];

/// `Core::key(k, def)`: the value when set (also when empty), else `def`.
pub fn key<'a>(profile: &'a Profile, k: &str, def: &'a str) -> &'a str {
    profile.get(k).unwrap_or(def)
}

/// C `strtoull(text, NULL, 0)` on LP64: leading white space, an optional sign (a `-` negates in
/// unsigned arithmetic), `0x` / `0X` hex, a leading `0` octal, else decimal; stops at the first
/// character that is not a digit of the base; no digits gives 0; overflow gives `u64::MAX`.
#[allow(clippy::many_single_char_names, reason = "the C library's names")]
pub fn strtoull(text: &str) -> u64 {
    let s = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, s) = match s.as_bytes().first() {
        Some(b'-') => (true, s.get(1..).unwrap_or_default()),
        Some(b'+') => (false, s.get(1..).unwrap_or_default()),
        _ => (false, s),
    };
    let b = s.as_bytes();
    let (base, digits) = if (b.first() == Some(&b'0'))
        && matches!(b.get(1), Some(b'x' | b'X'))
        && b.get(2).is_some_and(u8::is_ascii_hexdigit)
    {
        (16, s.get(2..).unwrap_or_default())
    } else if b.first() == Some(&b'0') {
        (8, s)
    } else {
        (10, s)
    };
    let mut v: u64 = 0;
    let mut overflow = false;
    for c in digits.chars() {
        let Some(d) = c.to_digit(base) else { break };
        match v.checked_mul(u64::from(base)).and_then(|x| x.checked_add(u64::from(d))) {
            Some(n) => v = n,
            None => overflow = true,
        }
    }
    if overflow {
        return u64::MAX;
    }
    if neg { v.wrapping_neg() } else { v }
}

/// C `strtoul(text, NULL, 0)` (LP64: the same as [`strtoull`]).
pub fn strtoul(text: &str) -> u64 {
    strtoull(text)
}

/// `static_cast<uint32_t>(strtoul(text, nullptr, 0))`: the low 32 bits.
pub fn strtoul_u32(text: &str) -> u32 {
    u32::try_from(strtoul(text) & u64::from(u32::MAX)).unwrap_or(u32::MAX)
}

/// C `atoi` (`sai_core::keys::atoi_text`, clamped to `i32` where C would be undefined).
pub fn atoi(text: &str) -> i32 {
    let v = sai_core::keys::atoi_text(text);
    i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX })
}

/// How a netdev is made for a host interface (`SAI_TERALYNX_NETDEV`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetdevMode {
    /// Rename the SDK's `inno_0_<sysport>` (the default).
    Adopt,
    /// `ifcs_netdev_create` per host interface.
    Create,
}

/// Every key's value as the C++ backend reads it, parsed once at init. Fields of areas not ported
/// yet are read now so their keys are not silently ignored later (G2 uses them).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "one flag per on/off key")]
pub struct Settings {
    /// `SAI_INIT_CONFIG_FILE`.
    pub config: String,
    /// `SAI_TERALYNX_CONFIG_APPEND`.
    pub append: String,
    /// `SAI_TERALYNX_RUN_DIR`.
    pub run_dir: String,
    /// `SAI_TERALYNX_LIBIFCS`.
    pub libifcs: String,
    /// `SAI_TERALYNX_PORT_MAP`.
    pub port_map: String,
    /// `SAI_TERALYNX_INIT_CMDS`.
    pub init_cmds: String,
    /// `SAI_TERALYNX_NETDEV`.
    pub netdev: NetdevMode,
    /// `SAI_TERALYNX_SHUTDOWN` != `ports-down`.
    pub deinit_on_shutdown: bool,
    /// `SAI_TERALYNX_IFCS_GUARD` != `off`.
    pub guard: bool,
    /// `SAI_TERALYNX_LEARN_RING_SIZE` (`strtoul`, truncated to 32 bits).
    pub ring_size: u32,
    /// `SAI_TERALYNX_IM_MALLOC_LIMIT_MIB` (`strtoull` << 20).
    pub malloc_limit: u64,
    /// `SAI_TERALYNX_IFCS_TRACE`.
    pub trace: String,
    /// `SAI_TERALYNX_IFCS_LOG_NAME`.
    pub log_name: String,
    /// `SAI_TERALYNX_LINK_STATUS` == `cached`.
    pub link_cached: bool,
    /// `SAI_TERALYNX_STATS` != `all`.
    pub stats_by_id: bool,
    /// `SAI_TERALYNX_TEMP_PERIOD` (`atoi`; outside 0..=3600: 15).
    pub temp_period: u32,
    /// `SAI_TERALYNX_TEMP_MONITORS` == `master`: 1, else 5.
    pub temp_monitors: usize,
    /// `SAI_TERALYNX_L3INTF_STATS` != `off`.
    pub l3intf_stats: bool,
    /// `SAI_TERALYNX_L3INTF_STATS_MODES` (`strtoul` each, empty parts skipped).
    pub l3intf_modes: Vec<u32>,
    /// `SAI_TERALYNX_L3INTF_STATS_COUNTERS` (`strtoul`, 32 bits).
    pub l3intf_counters: u32,
    /// `SAI_TERALYNX_SYSTEM_TRAPS` == `sdk`: leave them as the SDK has them.
    pub system_traps_sdk: bool,
    /// `SAI_TERALYNX_SFLOW_CPU_QUEUE`: `None` when empty (`atoi` otherwise).
    pub sflow_cpu_queue: Option<i32>,
    /// `SAI_TERALYNX_SFLOW_METER` == `on`.
    pub sflow_meter: bool,
    /// `SAI_TERALYNX_EGRESS_SFLOW` == `on`.
    pub egress_sflow: bool,
    /// `SAI_TERALYNX_IPD_DEV` (empty: `/dev/ipd` or `/dev/ipd0`).
    pub ipd_dev: String,
    /// `SAI_TERALYNX_CPU_FLOOD` == `on`.
    pub cpu_flood: bool,
    /// `SAI_TERALYNX_LAG_FLOOD` != `off`.
    pub lag_flood: bool,
    /// `SAI_TERALYNX_HASH_FIELDS` (parsed at init by the LAG area).
    pub hash_fields: String,
    /// `SAI_TERALYNX_HASH_SEED` (empty: not set).
    pub hash_seed: String,
    /// `SAI_TERALYNX_POST_INIT_REGS` (empty: none; Rust only, no C++ counterpart).
    pub post_init_regs: String,
    /// `SAI_TERALYNX_DIAG_REGS` (empty: none; Rust only).
    pub diag_regs: String,
}

impl Settings {
    /// The profile's values. The one refusal the C++ makes while reading keys: a
    /// `SAI_TERALYNX_NETDEV` other than `adopt` / `create` (`None`).
    pub fn from_profile(p: &Profile) -> Option<Self> {
        let netdev = match key(p, NETDEV, "adopt") {
            "adopt" => NetdevMode::Adopt,
            "create" => NetdevMode::Create,
            _ => return None,
        };
        let period = atoi(key(p, TEMP_PERIOD, "15"));
        Some(Self {
            config: key(p, INIT_CONFIG_FILE, "").to_owned(),
            append: key(p, CONFIG_APPEND, "").to_owned(),
            run_dir: key(p, RUN_DIR, "/run/sai-teralynx").to_owned(),
            libifcs: key(p, LIBIFCS, "").to_owned(),
            port_map: key(p, PORT_MAP, "").to_owned(),
            init_cmds: key(p, INIT_CMDS, "").to_owned(),
            netdev,
            deinit_on_shutdown: key(p, SHUTDOWN, "deinit") != "ports-down",
            guard: key(p, IFCS_GUARD, "on") != "off",
            ring_size: strtoul_u32(key(p, LEARN_RING_SIZE, "2048")),
            malloc_limit: strtoull(key(p, IM_MALLOC_LIMIT_MIB, "1024")) << 20,
            trace: key(p, IFCS_TRACE, "").to_owned(),
            log_name: key(p, IFCS_LOG_NAME, "sai-teralynx").to_owned(),
            link_cached: key(p, LINK_STATUS, "read") == "cached",
            stats_by_id: key(p, STATS, "ids") != "all",
            temp_period: u32::try_from(period).ok().filter(|&s| s <= 3600).unwrap_or(15),
            temp_monitors: if key(p, TEMP_MONITORS, "all") == "master" { 1 } else { 5 },
            l3intf_stats: key(p, L3INTF_STATS, "on") != "off",
            l3intf_modes: key(p, L3INTF_STATS_MODES, "4,1,2,3")
                .split(',')
                .filter(|m| !m.is_empty())
                .map(strtoul_u32)
                .collect(),
            l3intf_counters: strtoul_u32(key(p, L3INTF_STATS_COUNTERS, "4096")),
            system_traps_sdk: key(p, SYSTEM_TRAPS, "isai") == "sdk",
            sflow_cpu_queue: Some(key(p, SFLOW_CPU_QUEUE, "")).filter(|k| !k.is_empty()).map(atoi),
            sflow_meter: key(p, SFLOW_METER, "off") == "on",
            egress_sflow: key(p, EGRESS_SFLOW, "off") == "on",
            ipd_dev: key(p, IPD_DEV, "").to_owned(),
            cpu_flood: key(p, CPU_FLOOD, "off") == "on",
            lag_flood: key(p, LAG_FLOOD, "on") != "off",
            hash_fields: key(p, HASH_FIELDS, "").to_owned(),
            hash_seed: key(p, HASH_SEED, "").to_owned(),
            post_init_regs: key(p, POST_INIT_REGS, "").to_owned(),
            diag_regs: key(p, DIAG_REGS, "").to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_parse_as_the_c_library() {
        assert_eq!(strtoull("2048"), 2048);
        assert_eq!(strtoull(" 0x800"), 0x800);
        assert_eq!(strtoull("010"), 8); // octal
        assert_eq!(strtoull("09"), 0); // octal stops at 9
        assert_eq!(strtoull("12abc"), 12);
        assert_eq!(strtoull("abc"), 0);
        assert_eq!(strtoull(""), 0);
        assert_eq!(strtoull("0x"), 0); // "0" then "x": no hex digits
        assert_eq!(strtoull("-1"), u64::MAX);
        assert_eq!(strtoull("99999999999999999999999"), u64::MAX);
        assert_eq!(strtoul_u32("0x100000005"), 5); // static_cast<uint32_t>
        assert_eq!(atoi(" -3x"), -3);
        assert_eq!(atoi("x"), 0);
    }

    #[test]
    fn settings_have_the_cpp_defaults() {
        let s = Settings::from_profile(&Profile::default()).unwrap();
        assert_eq!(s.run_dir, "/run/sai-teralynx");
        assert_eq!(s.netdev, NetdevMode::Adopt);
        assert!(s.deinit_on_shutdown && s.guard && s.stats_by_id && s.l3intf_stats && s.lag_flood);
        assert!(!s.link_cached && !s.system_traps_sdk && !s.sflow_meter && !s.egress_sflow && !s.cpu_flood);
        assert_eq!((s.ring_size, s.malloc_limit), (2048, 1024 << 20));
        assert_eq!((s.temp_period, s.temp_monitors), (15, 5));
        assert_eq!(s.l3intf_modes, vec![4, 1, 2, 3]);
        assert_eq!(s.l3intf_counters, 4096);
        assert_eq!(s.log_name, "sai-teralynx");
        assert_eq!(s.sflow_cpu_queue, None);
    }

    #[test]
    fn values_parse_and_refuse_as_the_cpp() {
        let p = |pairs: &[(&str, &str)]| Profile::from_pairs(pairs.iter().copied());
        assert!(Settings::from_profile(&p(&[(NETDEV, "bogus")])).is_none());
        assert!(Settings::from_profile(&p(&[(NETDEV, "")])).is_none()); // set but empty: neither
        let s = Settings::from_profile(&p(&[
            (NETDEV, "create"),
            (SHUTDOWN, "ports-down"),
            (IFCS_GUARD, "off"),
            (LINK_STATUS, "cached"),
            (STATS, "all"),
            (TEMP_PERIOD, "4000"),
            (TEMP_MONITORS, "master"),
            (L3INTF_STATS_MODES, ",3,,0x2,"),
            (IM_MALLOC_LIMIT_MIB, "0"),
            (SFLOW_CPU_QUEUE, "2"),
            (RUN_DIR, ""),
        ]))
        .unwrap();
        assert_eq!(s.netdev, NetdevMode::Create);
        assert!(!s.deinit_on_shutdown && !s.guard && s.link_cached && !s.stats_by_id);
        assert_eq!((s.temp_period, s.temp_monitors), (15, 1));
        assert_eq!(s.l3intf_modes, vec![3, 2]);
        assert_eq!(s.malloc_limit, 0);
        assert_eq!(s.sflow_cpu_queue, Some(2));
        assert_eq!(s.run_dir, ""); // set: the value, even empty
        let s = Settings::from_profile(&p(&[(TEMP_PERIOD, "0"), (STATS, "ids2")])).unwrap();
        assert_eq!(s.temp_period, 0);
        assert!(s.stats_by_id); // anything but "all"
        assert_eq!(PROFILE_KEYS.len(), 31);
    }
}

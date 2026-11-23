//! Requests: `<name> <arg> <arg> ...`, words separated by single blanks (header
//! `sai_rs_test.h`). Parsing checks the shape of every argument; whether the thing named exists
//! is the answer's business.
use crate::encode;
use sai_core::backend::types::MacAddr;
use std::net::IpAddr;

/// An optional programming group of the virtual backend (`tp::Feature`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// `qos`: `QoS` maps, schedulers, WRED, buffers, queue / PG / PFC counters.
    Qos,
    /// `mirror`: mirror sessions, sampling, generic netlink.
    Mirror,
    /// `l2`: LAG / ECMP hash, storm control, VLAN flood control.
    L2Extras,
}

/// A table of the virtual backend whose size the tests check (`tp::Table`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// `T_STATIONS`
    Stations,
    /// `T_QOS_MAPS`
    QosMaps,
    /// `T_PORT_QOS_MAPS`
    PortQosMaps,
    /// `T_QUEUE_SCHEDULERS`
    QueueSchedulers,
    /// `T_PORT_SCHEDULERS`
    PortSchedulers,
    /// `T_QUEUE_BUFFERS`
    QueueBuffers,
    /// `T_PG_BUFFERS`
    PgBuffers,
    /// `T_MIRRORS`
    Mirrors,
    /// `T_PORT_MIRRORS`
    PortMirrors,
    /// `T_SAMPLES`
    Samples,
    /// `T_PORT_SAMPLES`
    PortSamples,
    /// `T_STORM`
    Storm,
    /// `T_FLOOD`
    Flood,
    /// `T_FP_GROUPS`
    FpGroups,
    /// `T_FP_ENTRIES`
    FpEntries,
    /// `T_POLICERS`
    Policers,
}

impl Table {
    const NAMES: [(&'static str, Self); 16] = [
        ("T_STATIONS", Self::Stations),
        ("T_QOS_MAPS", Self::QosMaps),
        ("T_PORT_QOS_MAPS", Self::PortQosMaps),
        ("T_QUEUE_SCHEDULERS", Self::QueueSchedulers),
        ("T_PORT_SCHEDULERS", Self::PortSchedulers),
        ("T_QUEUE_BUFFERS", Self::QueueBuffers),
        ("T_PG_BUFFERS", Self::PgBuffers),
        ("T_MIRRORS", Self::Mirrors),
        ("T_PORT_MIRRORS", Self::PortMirrors),
        ("T_SAMPLES", Self::Samples),
        ("T_PORT_SAMPLES", Self::PortSamples),
        ("T_STORM", Self::Storm),
        ("T_FLOOD", Self::Flood),
        ("T_FP_GROUPS", Self::FpGroups),
        ("T_FP_ENTRIES", Self::FpEntries),
        ("T_POLICERS", Self::Policers),
    ];

    fn from_word(word: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(name, _)| *name == word).map(|(_, t)| *t)
    }
}

/// A request, its arguments decoded. Numbers that name a backend object stay as written
/// (`i64`): one that no object can have is answered as absent, as by the C++ test port.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    missing_docs,
    reason = "one variant per request of the header; the header documents them"
)]
pub enum Request {
    HasHardwareBackend,
    SwitchCreated,
    MakeOid {
        object_type: u32,
        index: u64,
        switch: u32,
    },
    OidType(u64),
    OidIndex(u64),
    OidSwitch(u64),
    OidIndexMask,
    AttrCaps,
    BackendId(u64),
    Rif(u64),
    PortLag(u64),
    TrapProgrammed(u64),
    ObjectCount(u32),
    AclBindingCount,
    CoppGroup,
    CoppRuleCount,
    EventsDelivered,
    NeighborEgress(u64, IpAddr),
    DiagStart(String),
    DiagStop,
    LogToStderr(bool),
    ExtPhyActive,
    ExtPhyStatus,
    ExtMacsecActive,
    ExtMacsecStatus,
    SetFeature(Feature, bool),
    InjectLink(i64, bool),
    Linkscan,
    InjectFdb {
        learned: bool,
        mac: MacAddr,
        vid: i64,
        dest: i64,
    },
    SetPortStat(i64, u32, u64),
    SetQueueStat(i64, i64, u32, u64),
    SetPgStat(i64, i64, u32, u64),
    FpHit(i64, u64, u64),
    SetTemperatures(Vec<i32>),
    SetTemperatureFail(bool),
    MdioSet {
        phy: u32,
        dev: u32,
        reg: u32,
        value: u32,
    },
    MdioSetDead(u32, bool),
    MdioLog {
        clear: bool,
    },
    ShellLog,
    BackendHostInsert(i64, IpAddr, i64),
    BackendHostErase(i64, IpAddr),
    BackendL2Insert(MacAddr, i64, i64),
    BackendL2Erase(MacAddr, i64),
    DropEgress,
    BackendEgressDestroy(i64),
    LagDest(i64),
    Count(Table),
    Ports,
    Port(i64),
    PortForgetQuirks(i64),
    L2AgeSeconds,
    VlanExists(i64),
    VlanMembers(i64),
    L2Entries,
    Netifs,
    TrapToCpu(i32),
    L3Intfs,
    EgressObjects,
    Egress(i64),
    Hosts,
    HostEgress(i64, IpAddr),
    Routes,
    RouteTarget(i64, IpAddr, IpAddr),
    EcmpGroups,
    LagMembers(i64),
    FpGroup(i64),
    FpEntries,
    FpEntry(i64),
    Policer(i64),
    QosMap(i64),
    PortQosMap(i64, i32),
    PortDefaultTc(i64),
    PortPfc(i64),
    QueueScheduler(i64, i64),
    PortScheduler(i64),
    Wred(i64),
    QueueWred(i64, i64),
    QueueBuffer(i64, i64),
    PgBuffer(i64, i64),
    Mirror(i64),
    PortMirrors(i64, bool),
    SampleRate(i64),
    PortSample(i64, bool),
    Genetlink(i64),
    Hash(bool),
    PortStorm(i64, u32),
    VlanFlood(i64, u32),
}

/// Why a request was refused (the one-line reason of `BAD_REQUEST`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadRequest(pub String);

/// The words after the name.
struct Args<'a> {
    name: &'a str,
    words: core::str::Split<'a, char>,
}

impl<'a> Args<'a> {
    fn bad(&self, what: &str) -> BadRequest {
        BadRequest(format!("{}: {what}", self.name))
    }

    fn word(&mut self, what: &str) -> Result<&'a str, BadRequest> {
        match self.words.next() {
            Some(word) if !word.is_empty() => Ok(word),
            _ => Err(self.bad(&format!("missing {what}"))),
        }
    }

    fn parse<T: core::str::FromStr>(&mut self, what: &str) -> Result<T, BadRequest> {
        let word = self.word(what)?;
        word.parse().map_err(|_| self.bad(&format!("bad {what} {word:?}")))
    }

    fn int(&mut self, what: &str) -> Result<i64, BadRequest> {
        self.parse(what)
    }

    fn bool(&mut self, what: &str) -> Result<bool, BadRequest> {
        match self.word(what)? {
            "0" => Ok(false),
            "1" => Ok(true),
            other => Err(self.bad(&format!("bad {what} {other:?} (0 or 1)"))),
        }
    }

    fn mac(&mut self) -> Result<MacAddr, BadRequest> {
        let word = self.word("MAC")?;
        encode::mac(word).ok_or_else(|| self.bad(&format!("bad MAC {word:?}")))
    }

    fn ip(&mut self, what: &str) -> Result<IpAddr, BadRequest> {
        let word = self.word(what)?;
        encode::ip(word).ok_or_else(|| self.bad(&format!("bad {what} {word:?} (4/<8 hex> or 6/<32 hex>)")))
    }

    /// The last argument: a string, empty when the word is missing or empty.
    fn string(&mut self) -> Result<String, BadRequest> {
        let word = self.words.next().unwrap_or_default();
        encode::unpercent(word).ok_or_else(|| self.bad(&format!("bad string {word:?}")))
    }

    /// The last argument: a list, empty when the word is missing or empty.
    fn list<T: core::str::FromStr>(&mut self, what: &str) -> Result<Vec<T>, BadRequest> {
        let word = self.words.next().unwrap_or_default();
        if word.is_empty() {
            return Ok(Vec::new());
        }
        word.split(',')
            .map(|item| item.parse().map_err(|_| self.bad(&format!("bad {what} {item:?}"))))
            .collect()
    }

    fn end(mut self) -> Result<(), BadRequest> {
        match self.words.next() {
            None => Ok(()),
            Some(extra) => Err(self.bad(&format!("unexpected argument {extra:?}"))),
        }
    }
}

impl Request {
    /// Parses a request.
    pub fn parse(text: &str) -> Result<Self, BadRequest> {
        let mut words = text.split(' ');
        let name = words.next().unwrap_or_default();
        let mut a = Args { name, words };
        let request = Self::parse_args(name, &mut a)?;
        a.end()?;
        Ok(request)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per request of the protocol, in the header's order"
    )]
    fn parse_args(name: &str, a: &mut Args<'_>) -> Result<Self, BadRequest> {
        Ok(match name {
            "has_hardware_backend" => Self::HasHardwareBackend,
            "switch_created" => Self::SwitchCreated,
            "make_oid" => Self::MakeOid {
                object_type: a.parse("object type")?,
                index: a.parse("index")?,
                switch: a.parse("switch index")?,
            },
            "oid_type" => Self::OidType(a.parse("object id")?),
            "oid_index" => Self::OidIndex(a.parse("object id")?),
            "oid_switch" => Self::OidSwitch(a.parse("object id")?),
            "oid_index_mask" => Self::OidIndexMask,
            "attr_caps" => Self::AttrCaps,
            "backend_id" => Self::BackendId(a.parse("object id")?),
            "rif" => Self::Rif(a.parse("object id")?),
            "port_lag" => Self::PortLag(a.parse("object id")?),
            "trap_programmed" => Self::TrapProgrammed(a.parse("object id")?),
            "object_count" => Self::ObjectCount(a.parse("object type")?),
            "acl_binding_count" => Self::AclBindingCount,
            "copp_group" => Self::CoppGroup,
            "copp_rule_count" => Self::CoppRuleCount,
            "events_delivered" => Self::EventsDelivered,
            "neighbor_egress" => Self::NeighborEgress(a.parse("object id")?, a.ip("IP")?),
            "diag_start" => Self::DiagStart(a.string()?),
            "diag_stop" => Self::DiagStop,
            "log_to_stderr" => Self::LogToStderr(a.bool("on")?),
            "ext_phy_active" => Self::ExtPhyActive,
            "ext_phy_status" => Self::ExtPhyStatus,
            "ext_macsec_active" => Self::ExtMacsecActive,
            "ext_macsec_status" => Self::ExtMacsecStatus,
            "set_feature" => {
                let feature = match a.word("feature")? {
                    "qos" => Feature::Qos,
                    "mirror" => Feature::Mirror,
                    "l2" => Feature::L2Extras,
                    other => return Err(a.bad(&format!("unknown feature {other:?} (qos, mirror, l2)"))),
                };
                Self::SetFeature(feature, a.bool("on")?)
            }
            "inject_link" => Self::InjectLink(a.int("hw")?, a.bool("up")?),
            "linkscan" => Self::Linkscan,
            "inject_fdb" => Self::InjectFdb {
                learned: a.bool("learned")?,
                mac: a.mac()?,
                vid: a.int("vid")?,
                dest: a.int("dest")?,
            },
            "set_port_stat" => Self::SetPortStat(a.int("hw")?, a.parse("stat id")?, a.parse("value")?),
            "set_queue_stat" => {
                Self::SetQueueStat(a.int("hw")?, a.int("queue")?, a.parse("stat id")?, a.parse("value")?)
            }
            "set_pg_stat" => Self::SetPgStat(a.int("hw")?, a.int("pg")?, a.parse("stat id")?, a.parse("value")?),
            "fp_hit" => Self::FpHit(a.int("eid")?, a.parse("packets")?, a.parse("bytes")?),
            "set_temperatures" => Self::SetTemperatures(a.list("temperature")?),
            "set_temperature_fail" => Self::SetTemperatureFail(a.bool("fail")?),
            "mdio_set" => Self::MdioSet {
                phy: a.parse("phy")?,
                dev: a.parse("dev")?,
                reg: a.parse("reg")?,
                value: a.parse("value")?,
            },
            "mdio_set_dead" => Self::MdioSetDead(a.parse("phy")?, a.bool("dead")?),
            "mdio_log" => Self::MdioLog {
                clear: a.bool("clear")?,
            },
            "shell_log" => Self::ShellLog,
            "backend_host_insert" => Self::BackendHostInsert(a.int("vrf")?, a.ip("IP")?, a.int("egress")?),
            "backend_host_erase" => Self::BackendHostErase(a.int("vrf")?, a.ip("IP")?),
            "backend_l2_insert" => Self::BackendL2Insert(a.mac()?, a.int("vid")?, a.int("dest")?),
            "backend_l2_erase" => Self::BackendL2Erase(a.mac()?, a.int("vid")?),
            "drop_egress" => Self::DropEgress,
            "backend_egress_destroy" => Self::BackendEgressDestroy(a.int("egress")?),
            "lag_dest" => Self::LagDest(a.int("tid")?),
            "count" => {
                let word = a.word("table")?;
                Self::Count(Table::from_word(word).ok_or_else(|| a.bad(&format!("unknown table {word:?}")))?)
            }
            "ports" => Self::Ports,
            "port" => Self::Port(a.int("hw")?),
            "port_forget_quirks" => Self::PortForgetQuirks(a.int("hw")?),
            "l2_age_seconds" => Self::L2AgeSeconds,
            "vlan_exists" => Self::VlanExists(a.int("vid")?),
            "vlan_members" => Self::VlanMembers(a.int("vid")?),
            "l2_entries" => Self::L2Entries,
            "netifs" => Self::Netifs,
            "trap_to_cpu" => Self::TrapToCpu(a.parse("trap type")?),
            "l3_intfs" => Self::L3Intfs,
            "egress_objects" => Self::EgressObjects,
            "egress" => Self::Egress(a.int("egress")?),
            "hosts" => Self::Hosts,
            "host_egress" => Self::HostEgress(a.int("vrf")?, a.ip("IP")?),
            "routes" => Self::Routes,
            "route_target" => Self::RouteTarget(a.int("vrf")?, a.ip("address")?, a.ip("mask")?),
            "ecmp_groups" => Self::EcmpGroups,
            "lag_members" => Self::LagMembers(a.int("tid")?),
            "fp_group" => Self::FpGroup(a.int("gid")?),
            "fp_entries" => Self::FpEntries,
            "fp_entry" => Self::FpEntry(a.int("eid")?),
            "policer" => Self::Policer(a.int("pid")?),
            "qos_map" => Self::QosMap(a.int("id")?),
            "port_qos_map" => Self::PortQosMap(a.int("hw")?, a.parse("QoS map type")?),
            "port_default_tc" => Self::PortDefaultTc(a.int("hw")?),
            "port_pfc" => Self::PortPfc(a.int("hw")?),
            "queue_scheduler" => Self::QueueScheduler(a.int("hw")?, a.int("queue")?),
            "port_scheduler" => Self::PortScheduler(a.int("hw")?),
            "wred" => Self::Wred(a.int("id")?),
            "queue_wred" => Self::QueueWred(a.int("hw")?, a.int("queue")?),
            "queue_buffer" => Self::QueueBuffer(a.int("hw")?, a.int("queue")?),
            "pg_buffer" => Self::PgBuffer(a.int("hw")?, a.int("pg")?),
            "mirror" => Self::Mirror(a.int("id")?),
            "port_mirrors" => Self::PortMirrors(a.int("hw")?, a.bool("ingress")?),
            "sample_rate" => Self::SampleRate(a.int("id")?),
            "port_sample" => Self::PortSample(a.int("hw")?, a.bool("ingress")?),
            "genetlink" => Self::Genetlink(a.int("id")?),
            "hash" => Self::Hash(a.bool("lag")?),
            "port_storm" => Self::PortStorm(a.int("hw")?, a.parse("port attribute id")?),
            "vlan_flood" => Self::VlanFlood(a.int("vid")?, a.parse("VLAN attribute id")?),
            "" => return Err(BadRequest("empty request".to_owned())),
            other => return Err(BadRequest(format!("{other}: unknown request"))),
        })
    }
}

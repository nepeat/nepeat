//! The Rust libsai's test port: the answering side of `sai_rs_test_query`, the one test-only
//! function `libsai.so` exports when `sai-ffi` is built with the feature `test-port` (C
//! declaration and return codes: `include/sai_rs_test.h`). The C++ suite of sai-openbcm
//! (`tests/test_sai.cpp`, 3014 checks) reaches a library only through the SAI C ABI and
//! `tests/test_port.h`; `tests/test_port_rust.cpp` implements that header over this function, so
//! the same suite runs against the Rust library and its virtual backend. Each answer means what
//! `test_port.h` says; `tests/test_port_cpp.cpp` (over the C++ internals) is the reference.
//!
//! No unsafe code here: the rim (`sai-ffi/src/test_port.rs`) checks the pointers, validates the
//! UTF-8 and calls [`answer`] under the API lock; this crate parses ([`request`]), asks
//! [`sai_core::testing`] and the virtual backend, and encodes the reply.
//!
//! # Protocol
//!
//! A request is words separated by single blanks: `<name> <arg> <arg> ...`. A reply is zero or
//! more records, one per line (`\n` after each, the last too); a record is blank-separated
//! `key=value` fields in a fixed order. Return codes: [`Code`] (`OK` 0, `ABSENT` 1 = the
//! `tp` function's "false" with an empty reply, `TOO_SMALL` 2, `UNSUPPORTED` 3 and `BAD_REQUEST`
//! 4 with a one-line reason, `FAILURE` -1).
//!
//! Values: integers in decimal (object ids as `u64`), booleans `0` / `1`, MACs
//! `aa:bb:cc:dd:ee:ff`, IP addresses `4/<8 hex>` or `6/<32 hex>` (network order, lower case:
//! 10.0.0.1 is `4/0a000001`), lists `a,b,c` (empty: nothing after `=`), strings and free text
//! percent-encoded (every byte outside `[A-Za-z0-9_.:/-]` as `%HH`, upper case).
//!
//! Requests and their records (`->`; "empty": no records):
//!
//! - library and OIDs, answered also before `sai_api_initialize`: `has_hardware_backend`,
//!   `switch_created`, `make_oid <ot> <index> <switch>`, `oid_type <oid>`, `oid_index <oid>`,
//!   `oid_switch <oid>`, `oid_index_mask` -> `value=`; `attr_caps` -> `ot= id= create= set= get=`
//!   per row of the capability table; `events_delivered` -> `value=`; `log_to_stderr <0|1>`.
//! - the library's view: `backend_id <oid>` -> `value=` (-1 none); `rif <oid>` -> `intf= vrf= vid=`
//!   (unknown: -1 -1 0); `port_lag <oid>` -> `value=<oid>` (0 none); `trap_programmed <oid>`;
//!   `object_count <ot>` (`VIRTUAL_ROUTER`, `ACL_TABLE`, `ACL_ENTRY`); `acl_binding_count`,
//!   `copp_group`, `copp_rule_count`; `neighbor_egress <rif oid> <ip>` -> `value=`.
//! - services: `diag_start <path>`, `diag_stop`; `ext_phy_active`, `ext_macsec_active` ->
//!   `value=`; `ext_phy_status`, `ext_macsec_status` -> `text=`.
//! - virtual backend hooks (empty on success): `set_feature <qos|mirror|l2> <0|1>`,
//!   `inject_link <hw> <0|1>`, `linkscan`, `inject_fdb <learned> <mac> <vid> <dest>`,
//!   `set_port_stat <hw> <id> <v>`, `set_queue_stat <hw> <q> <id> <v>`, `set_pg_stat <hw> <pg> <id> <v>`,
//!   `fp_hit <eid> <packets> <bytes>`, `set_temperatures <list>`, `set_temperature_fail <0|1>`,
//!   `mdio_set <phy> <dev> <reg> <value>`, `mdio_set_dead <phy> <0|1>`, `mdio_log <clear>` ->
//!   `op=R|W phy= dev= reg= val=`, `shell_log` -> `cmd=`, `backend_host_insert <vrf> <ip> <egress>`,
//!   `backend_host_erase <vrf> <ip>`, `backend_l2_insert <mac> <vid> <dest>`,
//!   `backend_l2_erase <mac> <vid>`, `drop_egress` -> `value=`, `backend_egress_destroy <egress>` ->
//!   `status=`, `lag_dest <tid>` -> `value=`, `port_forget_quirks <hw>`.
//! - virtual backend state: `count <T_...>`, `ports` / `port <hw>` (`hw= admin= link= fec= autoneg=
//!   iface= ifg= quirk_calls= loopback= pvid= frame_max= learn=`), `l2_age_seconds`, `vlan_exists`,
//!   `vlan_members`, `l2_entries`, `netifs`, `trap_to_cpu`, `l3_intfs`, `egress_objects` / `egress`,
//!   `hosts` / `host_egress`, `routes` / `route_target`, `ecmp_groups`, `lag_members`, `fp_group`,
//!   `fp_entries` / `fp_entry`, `policer`, `qos_map`, `port_qos_map`, `port_default_tc`,
//!   `port_pfc`, `queue_scheduler`, `port_scheduler`, `wred`, `queue_wred`, `queue_buffer`,
//!   `pg_buffer`, `mirror`, `port_mirrors`, `sample_rate`, `port_sample`, `genetlink`, `hash`,
//!   `port_storm`, `vlan_flood`: the fields and the absent answers of `test_port.h`, in its
//!   order ([`request::Request`] lists the arguments; `answer.rs` the fields).
//!
//! A question an area module of `sai-core` cannot answer yet ([`sai_core::testing::NotPorted`]),
//! a virtual-backend request without a switch or with another backend, and anything before
//! `sai_api_initialize` that needs the library are `UNSUPPORTED`. Hooks queue the events they
//! raise; the library's notifier delivers them after the call.
#![forbid(unsafe_code)]

mod answer;
mod encode;
pub mod request;

use sai_core::backend::Backend;
use sai_core::sai::Sai;

/// A return code of `sai_rs_test_query` (the macros of `sai_rs_test.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    /// `SAI_RS_TEST_OK`: the reply holds the answer.
    Ok,
    /// `SAI_RS_TEST_ABSENT`: the thing asked about does not exist; empty reply.
    Absent,
    /// `SAI_RS_TEST_TOO_SMALL`: the reply buffer is too small (the rim's answer).
    TooSmall,
    /// `SAI_RS_TEST_UNSUPPORTED`: the library cannot answer this yet; the reply says why.
    Unsupported,
    /// `SAI_RS_TEST_BAD_REQUEST`: unknown request or malformed arguments; the reply says why.
    BadRequest,
    /// `SAI_RS_TEST_FAILURE`: bad pointers, invalid UTF-8, a panic, a poisoned lock (the rim's).
    Failure,
}

impl Code {
    /// The C value.
    pub const fn raw(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Absent => 1,
            Self::TooSmall => 2,
            Self::Unsupported => 3,
            Self::BadRequest => 4,
            Self::Failure => -1,
        }
    }
}

/// An answer: its code and the reply text (records, a reason, or nothing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The code.
    pub code: Code,
    /// The text.
    pub text: String,
}

/// What the rim knows and the core does not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// The library was built with a hardware backend (`sai-ffi` feature `openbcm`).
    pub has_hardware_backend: bool,
    /// Backend events the notifier has finished (delivered or dropped). Monotonic.
    pub events_delivered: u64,
}

/// A parsed request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    name: String,
    request: request::Request,
}

impl Query {
    /// Parses a request; a malformed one is the `BAD_REQUEST` reply.
    pub fn parse(text: &str) -> Result<Self, Reply> {
        let request = request::Request::parse(text).map_err(|reason| Reply {
            code: Code::BadRequest,
            text: reason.0,
        })?;
        Ok(Self {
            name: text.split(' ').next().unwrap_or_default().to_owned(),
            request,
        })
    }

    /// The request.
    pub fn request(&self) -> &request::Request {
        &self.request
    }

    /// Whether the answer needs the library's state: the rim then takes the API lock and passes
    /// the core; otherwise `None` will do (OID layout, build facts, the log switch).
    pub fn needs_library(&self) -> bool {
        !answer::is_pure(&self.request)
    }

    /// Answers it. `sai`: the core under the API lock, `None` before `sai_api_initialize` (or
    /// when [`Query::needs_library`] is false).
    pub fn answer<B: Backend>(&self, sai: Option<&mut Sai<B>>, context: &Context) -> Reply {
        answer::answer(&self.name, &self.request, sai, context)
    }
}

/// Parses and answers one request ([`Query`]).
pub fn answer<B: Backend>(request: &str, sai: Option<&mut Sai<B>>, context: &Context) -> Reply {
    match Query::parse(request) {
        Ok(query) => query.answer(sai, context),
        Err(reply) => reply,
    }
}

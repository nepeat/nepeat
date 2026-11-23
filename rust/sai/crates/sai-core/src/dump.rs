//! `sai_dbg_generate_dump`: a text summary of the model (backend, switch, ports, VLANs, counts per
//! type, LAGs, host interfaces). Ports the dump of `sai_api.cpp` line for line: the same words,
//! the same order, object ids in lower-case hex without `0x` (`std::hex`), numbers in decimal,
//! booleans as `0` / `1`, a port's oper status as the SAI enumerator's number.
//!
//! What the C++ prints from state this library does not keep (stated in the output's place):
//! - `lag ... trunk`: the SDK trunk id is the LAG area's, not the store's: `-1` (the C++ value of
//!   a LAG without a trunk). `pvid`: the LAG's `PORT_VLAN_ID` attribute, else 1 (the C++ default).
//! - `hostif ... netif` / `filter`: the KNET ids are the host interface area's: `-1` (the C++
//!   value of none).
//! - `acl_bindings`: the ports and LAGs whose `INGRESS_ACL` attribute names an object (the C++
//!   `acl_bind` map: port or LAG to its ingress table or group). `copp_entries`: 0 (the `CoPP`
//!   field entries are not in the store).
//! - No switch: `backend none`, `switch 0 mac 02:00:00:00:00:01` (the C++ initial MAC) and zero
//!   counts, as the C++ prints before the first switch (after a removal it prints the last MAC).
//!
//! Backend: [`crate::backend::Base::is_fake`] (the C++ name of the in-memory backend is `fake`)
//! and [`crate::backend::Backend::name`]; everything else comes from the store.
//!
//! Registration: none in the dispatch table; the rim's `sai_dbg_generate_dump` calls [`text`]
//! under the API lock and writes the file outside it.
use crate::backend::Backend;
use crate::model::{EntryKey, Port, PortKind, Switch, Vlan, VlanMember};
use crate::oid::{ObjectType, SwitchId, TypedId};
use crate::store::Store;
use crate::value::Value;
use core::fmt::Write as _;
use sai_sys as sys;

/// The C++ `Ctx::src_mac` before a switch set it.
const INITIAL_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
/// What the C++ prints for an SDK id (trunk, netif, filter) that does not exist.
const NO_ID: i32 = -1;

/// The dump text (the file's whole content). `store` and `switch` are the switch's, `None`
/// when there is none.
pub fn text<B: Backend + ?Sized>(backend: &B, store: Option<&Store>, switch: Option<SwitchId>) -> String {
    let mut out = String::new();
    let switch = store.zip(switch);
    let name = match switch {
        None => "none",
        Some(_) if backend.is_fake() => "fake",
        Some(_) => backend.name(),
    };
    // Writing into a String cannot fail.
    let _ = writeln!(out, "sai-openbcm dump\nbackend {name}");
    match switch {
        None => {
            let _ = writeln!(out, "switch 0 mac {}", mac(INITIAL_MAC));
            write_counts(&mut out, &Store::new());
        }
        Some((store, id)) => {
            let src_mac = store.get::<Switch>(id).map_or(INITIAL_MAC, |s| s.src_mac);
            let _ = writeln!(out, "switch {:x} mac {}", id.raw().0, mac(src_mac));
            write_ports(&mut out, store);
            for (vlan, v) in store.iter::<Vlan>() {
                let members = store.iter::<VlanMember>().filter(|(_, m)| m.vlan == vlan).count();
                let _ = writeln!(out, "vlan {} members {members}", v.vid.get());
            }
            write_counts(&mut out, store);
            write_lags(&mut out, store);
            write_hostifs(&mut out, store);
        }
    }
    out
}

/// `%02x:%02x:%02x:%02x:%02x:%02x` (C++ `mac_str`).
fn mac(bytes: [u8; 6]) -> String {
    bytes.map(|byte| format!("{byte:02x}")).join(":")
}

/// One line per front-panel port, in chip order (the C++ `port_list`).
fn write_ports(out: &mut String, store: &Store) {
    for (id, port) in store.iter::<Port>() {
        let PortKind::Front { lanes } = &port.kind else {
            continue;
        };
        let _ = write!(out, "port {:x} hw {} {} lanes", id.raw().0, port.hw.0, port.name);
        for lane in lanes {
            let _ = write!(out, " {lane}");
        }
        let oper = if port.oper_up {
            sys::SAI_PORT_OPER_STATUS_UP
        } else {
            sys::SAI_PORT_OPER_STATUS_DOWN
        };
        let _ = writeln!(
            out,
            " admin {} oper {oper} pvid {} mtu {}",
            u8::from(port.admin),
            port.pvid.get(),
            port.mtu
        );
    }
}

/// The number of stored objects of a SAI type; 0 for a type this library does not implement.
fn count(store: &Store, object_type: sys::sai_object_type_t) -> usize {
    ObjectType::from_sai(object_type).map_or(0, |t| store.ids(t).count())
}

/// The number of entries of one kind.
fn entries(store: &Store, kind: fn(&EntryKey) -> bool) -> usize {
    store.entries().filter(|(key, _)| kind(key)).count()
}

/// Whether an attribute names an object.
fn names_object(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Oid(oid)) if !oid.is_null())
}

/// Ports and LAGs bound to an ingress ACL (the C++ `acl_bind`).
fn acl_bindings(store: &Store) -> usize {
    let ports = store
        .iter::<Port>()
        .filter(|(_, p)| names_object(p.kept.get(&sys::SAI_PORT_ATTR_INGRESS_ACL)))
        .count();
    let lags = store
        .bags(ObjectType::Lag)
        .filter(|(_, b)| names_object(b.attrs.get(&sys::SAI_LAG_ATTR_INGRESS_ACL)))
        .count();
    ports + lags
}

/// The two lines of object counts.
fn write_counts(out: &mut String, store: &Store) {
    let n = |t| count(store, t);
    let _ = writeln!(
        out,
        "bridge_ports {} fdb {} hostifs {} traps {} rifs {} neighbors {} next_hops {} routes {} lags {} \
         lag_members {} virtual_routers {} next_hop_groups {} next_hop_group_members {}",
        n(sys::SAI_OBJECT_TYPE_BRIDGE_PORT),
        entries(store, |k| matches!(k, EntryKey::Fdb { .. })),
        n(sys::SAI_OBJECT_TYPE_HOSTIF),
        n(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP),
        n(sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE),
        entries(store, |k| matches!(k, EntryKey::Neighbor { .. })),
        n(sys::SAI_OBJECT_TYPE_NEXT_HOP),
        entries(store, |k| matches!(k, EntryKey::Route { .. })),
        n(sys::SAI_OBJECT_TYPE_LAG),
        n(sys::SAI_OBJECT_TYPE_LAG_MEMBER),
        n(sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER),
        n(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP),
        n(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER),
    );
    let _ = writeln!(
        out,
        "policers {} acl_tables {} acl_entries {} acl_counters {} acl_table_groups {} acl_bindings {} \
         copp_entries 0",
        n(sys::SAI_OBJECT_TYPE_POLICER),
        n(sys::SAI_OBJECT_TYPE_ACL_TABLE),
        n(sys::SAI_OBJECT_TYPE_ACL_ENTRY),
        n(sys::SAI_OBJECT_TYPE_ACL_COUNTER),
        n(sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP),
        acl_bindings(store),
    );
}

/// One line per LAG, in id order.
fn write_lags(out: &mut String, store: &Store) {
    for lag in store.ids(ObjectType::Lag) {
        let members = store
            .bags(ObjectType::LagMember)
            .filter(|(_, m)| m.attrs.get(&sys::SAI_LAG_MEMBER_ATTR_LAG_ID) == Some(&Value::Oid(lag)))
            .count();
        let pvid = match store
            .bag(lag)
            .ok()
            .and_then(|b| b.attrs.get(&sys::SAI_LAG_ATTR_PORT_VLAN_ID))
        {
            Some(Value::U16(vid)) => *vid,
            _ => 1,
        };
        let _ = writeln!(out, "lag {:x} trunk {NO_ID} members {members} pvid {pvid}", lag.0);
    }
}

/// One line per host interface, in id order.
fn write_hostifs(out: &mut String, store: &Store) {
    for hostif in store.ids(ObjectType::Hostif) {
        let name = match store
            .bag(hostif)
            .ok()
            .and_then(|b| b.attrs.get(&sys::SAI_HOSTIF_ATTR_NAME))
        {
            Some(Value::CharData(name)) => name.as_str(),
            _ => "",
        };
        let _ = writeln!(out, "hostif {name} netif {NO_ID} filter {NO_ID}");
    }
}

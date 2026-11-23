//! L3 on the virtual backend: the C++ test_sai checks of test_l3, test_vrf, test_ecmp and the
//! router interface on a LAG (tests/test_sai.cpp), failure injection (a failed backend call
//! leaves model and chip as they were), and the backend call log of the differential's
//! driver-scripts/06-l3.ops against the C++ library's.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::doc_markdown
)]
mod common;

use common::{RIF, SWITCH, VLAN, VR, enumerator, get1, init_attrs, oid, oid_list};
use sai_core::backend::types::{Dest, EgressId, MacAddr, RouteTarget, Vrf};
use sai_core::backend::{CallLog, HwPort, Mode, Profile, Recording, Vid};
use sai_core::error::{AttrError, Error};
use sai_core::l3;
use sai_core::model::EntryKey;
use sai_core::oid::{RawOid, RouterInterfaceId, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{Attr, IpPrefix, Value};
use sai_core::virt::{EgressKind, VirtualBackend};
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};

const NH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_NEXT_HOP;
const NHG: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP;
const NHGM: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER;
const LAG: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_LAG;
const LAG_MEMBER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_LAG_MEMBER;
const BRIDGE_PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_BRIDGE_PORT;
const VLAN_MEMBER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_VLAN_MEMBER;

/// The C++ fake's numbering (port i is hw 1 + 4i), as the C++ tests and the driver have it.
fn cpp_profile(ports: u32) -> Profile {
    Profile::from_pairs([
        ("SAI_RS_VIRTUAL_PORTS", ports.to_string()),
        ("SAI_RS_VIRTUAL_COMPAT", "cpp".to_owned()),
    ])
}

fn cpp_switch<B: sai_core::backend::Backend>(backend: B, ports: u32) -> (Sai<B>, RawOid) {
    let mut sai = Sai::new(backend);
    let id = sai.create_switch(&init_attrs(), &cpp_profile(ports)).unwrap();
    (sai, id.raw())
}

struct T {
    sai: Sai<VirtualBackend>,
    sw: RawOid,
    ports: Vec<RawOid>,
    vr0: RawOid,
}

impl T {
    fn new() -> Self {
        let (sai, sw) = cpp_switch(VirtualBackend::new(), 8);
        let ports = oid_list(get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
        let vr0 = oid(get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID).unwrap());
        Self { sai, sw, ports, vr0 }
    }

    fn chip(&self) -> &sai_core::virt::L3Model {
        self.sai.backend().l3()
    }

    fn egress(&self, id: EgressId) -> EgressKind {
        *self.chip().egress.get(&id).unwrap()
    }

    fn route_target(&self, vrf: u32, p: IpPrefix) -> Option<RouteTarget> {
        self.chip().routes.get(&(Vrf(vrf), p)).copied()
    }

    fn host_egress(&self, vrf: u32, ip: IpAddr) -> Option<EgressId> {
        self.chip().hosts.get(&(Vrf(vrf), ip)).copied()
    }

    fn drop_egress(&mut self) -> EgressId {
        use sai_core::backend::L3 as _;
        self.sai.backend_mut().l3_egress_drop().unwrap()
    }

    fn rif(&mut self, vr: RawOid, kind: u32, on: Option<RawOid>) -> sai_core::Result<RawOid> {
        let mut attrs = vec![
            Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(vr)),
            Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_TYPE, enumerator(kind)),
        ];
        if let Some(on) = on {
            let id = if kind == sys::SAI_ROUTER_INTERFACE_TYPE_VLAN {
                sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID
            } else {
                sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID
            };
            attrs.push(Attr::new(id, Value::Oid(on)));
        }
        self.sai.create(RIF, self.sw, &attrs)
    }

    fn port_rif(&mut self, vr: RawOid, port: RawOid) -> RawOid {
        self.rif(vr, sys::SAI_ROUTER_INTERFACE_TYPE_PORT, Some(port)).unwrap()
    }

    fn nbr(&mut self, rif: RawOid, ip: IpAddr, last: u8) -> sai_core::Result<()> {
        let mac = Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac(nmac(last)));
        self.sai.create_entry(self.sw, nbr_key(rif, ip), &[mac])
    }

    fn nh(&mut self, rif: RawOid, ip: IpAddr) -> sai_core::Result<RawOid> {
        self.sai.create(
            NH,
            self.sw,
            &[
                Attr::new(sys::SAI_NEXT_HOP_ATTR_TYPE, enumerator(sys::SAI_NEXT_HOP_TYPE_IP)),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(ip)),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, Value::Oid(rif)),
            ],
        )
    }

    fn route(&mut self, vr: RawOid, p: IpPrefix, attrs: &[Attr]) -> sai_core::Result<()> {
        self.sai.create_entry(self.sw, route_key(vr, p), attrs)
    }

    fn route_nh(&mut self, vr: RawOid, p: IpPrefix, nh: RawOid) -> sai_core::Result<()> {
        self.route(vr, p, &[next_hop_attr(nh)])
    }

    fn group(&mut self) -> RawOid {
        self.sai
            .create(
                NHG,
                self.sw,
                &[Attr::new(
                    sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE,
                    enumerator(sys::SAI_NEXT_HOP_GROUP_TYPE_ECMP),
                )],
            )
            .unwrap()
    }

    fn member(&mut self, group: RawOid, nh: RawOid, weight: u32) -> sai_core::Result<RawOid> {
        let mut attrs = vec![
            Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID, Value::Oid(group)),
            Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID, Value::Oid(nh)),
        ];
        if weight > 0 {
            attrs.push(Attr::new(
                sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT,
                Value::U32(weight),
            ));
        }
        self.sai.create(NHGM, self.sw, &attrs)
    }

    fn vr(&mut self, attrs: &[Attr]) -> sai_core::Result<RawOid> {
        self.sai.create(VR, self.sw, attrs)
    }

    fn vlan(&mut self, vid: u16) -> sai_core::Result<RawOid> {
        self.sai
            .create(VLAN, self.sw, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(vid))])
    }

    fn u32_of(&self, attr: sys::sai_attr_id_t) -> u32 {
        match get1(&self.sai, SWITCH, self.sw, attr).unwrap() {
            Value::U32(n) => n,
            other => panic!("{other:?}"),
        }
    }

    fn neighbor_egress(&self, rif: RawOid, ip: IpAddr) -> EgressId {
        l3::neighbor_egress(self.sai.store().unwrap(), RouterInterfaceId::try_from(rif).unwrap(), ip).unwrap()
    }

    fn rif_obj(&self, rif: RawOid) -> l3::RouterInterface {
        let store = self.sai.store().unwrap();
        store
            .get::<l3::RouterInterface>(RouterInterfaceId::try_from(rif).unwrap())
            .unwrap()
            .clone()
    }

    fn rif_vid(&self, rif: RawOid) -> Vid {
        self.rif_obj(rif).vid().unwrap()
    }

    fn vrf(&self, vr: RawOid) -> u32 {
        let store = self.sai.store().unwrap();
        let id = sai_core::oid::VirtualRouterId::try_from(vr).unwrap();
        store.get::<l3::VirtualRouter>(id).unwrap().vrf.get()
    }

    fn ecmp_members(&self, id: sai_core::backend::types::EcmpId) -> Vec<EgressId> {
        self.chip().ecmps.get(&id).cloned().unwrap_or_default()
    }

    fn vlan_member(&self, vid: u16, hw: u16) -> Option<bool> {
        self.sai
            .backend()
            .vlans()
            .get(&Vid::new(vid).unwrap())
            .and_then(|m| m.get(&HwPort(hw)).copied())
    }

    fn pvid(&self, hw: u16) -> u16 {
        self.sai.backend().port_state(HwPort(hw)).unwrap().pvid.get()
    }

    fn vlan_exists(&self, vid: u16) -> bool {
        self.sai.backend().vlans().contains_key(&Vid::new(vid).unwrap())
    }
}

fn ip4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(a, b, c, d))
}

fn prefix(a: u8, b: u8, c: u8, d: u8, len: u32) -> IpPrefix {
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    IpPrefix::new(ip4(a, b, c, d), IpAddr::V4(Ipv4Addr::from_bits(mask))).unwrap()
}

fn nmac(last: u8) -> [u8; 6] {
    [0x00, 0x00, 0x5e, 0x00, 0x53, last]
}

fn nbr_key(rif: RawOid, ip: IpAddr) -> EntryKey {
    EntryKey::Neighbor { rif, ip }
}

fn route_key(vr: RawOid, prefix: IpPrefix) -> EntryKey {
    EntryKey::Route { vr, prefix }
}

fn next_hop_attr(nh: RawOid) -> Attr {
    Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID, Value::Oid(nh))
}

fn action(a: u32) -> Attr {
    Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION, enumerator(a))
}

fn egress_of(target: Option<RouteTarget>) -> EgressId {
    match target {
        Some(RouteTarget::Egress(e)) => e,
        other => panic!("not an egress target: {other:?}"),
    }
}

/// C++ test_l3.
#[test]
fn port_and_vlan_router_interfaces_neighbors_next_hops_routes() {
    let mut t = T::new();
    let (vr, p3) = (t.vr0, t.ports[3]);
    let rif = t.port_rif(vr, p3);
    assert_eq!(rif.sai_object_type(), sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE);
    assert_eq!(t.vlan_member(4094, 13), Some(true));
    assert_eq!(t.pvid(13), 4094);
    assert_eq!(t.chip().intfs.len(), 1);
    assert_eq!(t.chip().stations.len(), 1);
    assert_eq!(
        t.rif(vr, sys::SAI_ROUTER_INTERFACE_TYPE_PORT, Some(p3)),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(t.vlan(4094), Err(Error::ItemAlreadyExists)); // internal VLAN

    // next hop first (unresolved: CPU), then the neighbor resolves it
    let ip = ip4(10, 0, 0, 2);
    let nh = t.nh(rif, ip).unwrap();
    let r1 = prefix(192, 168, 0, 0, 24);
    t.route_nh(vr, r1, nh).unwrap();
    assert_eq!(t.route_nh(vr, r1, nh), Err(Error::ItemAlreadyExists));
    assert_eq!(t.chip().routes.len(), 1);
    assert_eq!(t.egress(egress_of(t.route_target(0, r1))), EgressKind::Cpu);

    t.nbr(rif, ip, 0x01).unwrap();
    assert_eq!(t.chip().hosts.len(), 1);
    let nbr = t.host_egress(0, ip).unwrap();
    assert!(matches!(
        t.egress(nbr),
        EgressKind::Forward { dest: Dest::Port(HwPort(13)), dmac: MacAddr(m), .. } if m == nmac(0x01)
    ));
    assert_eq!(t.route_target(0, r1), Some(RouteTarget::Egress(nbr))); // route re-pointed

    // IP2ME and default-drop routes, as orchagent installs them
    let cpu = oid(get1(&t.sai, SWITCH, t.sw, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    let me = prefix(10, 0, 0, 1, 32);
    t.route_nh(vr, me, cpu).unwrap();
    let def = prefix(0, 0, 0, 0, 0);
    t.route(vr, def, &[action(sys::SAI_PACKET_ACTION_DROP)]).unwrap();
    assert_eq!(t.egress(egress_of(t.route_target(0, def))), EgressKind::Drop);
    // prefix canonicalisation: host bits are ignored in the key
    assert_eq!(
        t.sai.get_entry(
            t.sw,
            &route_key(vr, prefix(192, 168, 0, 77, 24)),
            &[sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID]
        ),
        Ok(vec![Value::Oid(nh)])
    );

    // in-use checks, then teardown in order
    assert_eq!(t.sai.remove_entry(t.sw, &nbr_key(rif, ip)), Err(Error::ObjectInUse));
    assert_eq!(t.sai.remove(NH, nh), Err(Error::ObjectInUse));
    assert_eq!(t.sai.remove(RIF, rif), Err(Error::ObjectInUse));
    t.sai.remove_entry(t.sw, &route_key(vr, r1)).unwrap();
    t.sai.remove(NH, nh).unwrap();
    t.sai.remove_entry(t.sw, &nbr_key(rif, ip)).unwrap();
    t.sai.remove_entry(t.sw, &route_key(vr, me)).unwrap();
    t.sai.remove_entry(t.sw, &route_key(vr, def)).unwrap();
    t.sai.remove(RIF, rif).unwrap();
    assert!(!t.vlan_exists(4094));
    assert_eq!(t.pvid(13), 1);
    assert!(t.chip().intfs.is_empty() && t.chip().hosts.is_empty() && t.chip().routes.is_empty());

    // VLAN RIF; a route on an unknown next-hop group
    let v200 = t.vlan(200).unwrap();
    let vrif = t.rif(vr, sys::SAI_ROUTER_INTERFACE_TYPE_VLAN, Some(v200)).unwrap();
    assert_eq!(t.chip().intfs.values().next().unwrap().vid.get(), 200);
    assert_eq!(t.sai.remove(VLAN, v200), Err(Error::ObjectInUse));
    let unknown_group = RawOid((u64::from(sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP) << 56) | 1);
    assert_eq!(
        t.route_nh(vr, prefix(172, 16, 0, 0, 12), unknown_group),
        Err(Error::InvalidParameter)
    );
    t.sai.remove(RIF, vrif).unwrap();
    t.sai.remove(VLAN, v200).unwrap();
    assert_eq!(
        t.rif(vr, sys::SAI_ROUTER_INTERFACE_TYPE_SUB_PORT, Some(p3)),
        Err(Error::NotSupported)
    );
    common::model_and_chip_agree(&t.sai).unwrap();
}

/// C++ test_vrf.
#[test]
fn virtual_routers_are_vrfs_and_routes_may_leak_between_them() {
    let mut t = T::new();
    let dvr = t.vr0;
    assert_eq!(t.sai.remove(VR, dvr), Err(Error::ObjectInUse));
    let vmac = [0x02, 0x00, 0x00, 0x00, 0x0a, 0x01];
    let vr1 = t
        .vr(&[
            Attr::new(sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS, Value::Mac(vmac)),
            Attr::new(
                sys::SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_TTL1_PACKET_ACTION,
                enumerator(sys::SAI_PACKET_ACTION_DROP),
            ),
        ])
        .unwrap();
    let vr2 = t.vr(&[]).unwrap();
    assert!(vr1 != vr2 && vr1 != dvr && vr2 != dvr);
    assert_eq!(t.vrf(vr1), 1);
    assert_eq!(t.vrf(vr2), 2);
    assert_eq!(t.vrf(dvr), 0);
    // routing cannot be switched off per VRF
    assert_eq!(
        t.vr(&[Attr::new(
            sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE,
            Value::Bool(false)
        )]),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    assert_eq!(
        t.sai.get(
            VR,
            vr1,
            &[
                sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS,
                sys::SAI_VIRTUAL_ROUTER_ATTR_VIOLATION_TTL1_PACKET_ACTION
            ]
        ),
        Ok(vec![Value::Mac(vmac), enumerator(sys::SAI_PACKET_ACTION_DROP)])
    );
    let smac = get1(&t.sai, SWITCH, t.sw, sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS).unwrap();
    assert_eq!(
        get1(&t.sai, VR, vr2, sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS),
        Ok(smac)
    );
    // unset stubs answer the C++ defaults
    assert_eq!(
        get1(
            &t.sai,
            VR,
            vr2,
            sys::SAI_VIRTUAL_ROUTER_ATTR_UNKNOWN_L3_MULTICAST_PACKET_ACTION
        ),
        Ok(enumerator(sys::SAI_PACKET_ACTION_DROP))
    );

    // RIF on port 6 in vr1 (takes vr1's MAC), RIF on port 7 in the default VR
    let (p6, p7) = (t.ports[6], t.ports[7]);
    let rif1 = t.port_rif(vr1, p6);
    let rif0 = t.port_rif(dvr, p7);
    let intf_of = |t: &T, rif: RawOid| *t.chip().intfs.get(&t.rif_obj(rif).intf.unwrap()).unwrap();
    assert_eq!(intf_of(&t, rif1).vrf, Vrf(1));
    assert_eq!(intf_of(&t, rif1).mac, MacAddr(vmac));
    assert_eq!(intf_of(&t, rif0).vrf, Vrf(0));
    assert_eq!(
        get1(&t.sai, RIF, rif1, sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID),
        Ok(Value::Oid(vr1))
    );
    // the MTU update keeps the VRF binding
    t.sai
        .set(
            RIF,
            rif1,
            &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_MTU, Value::U32(9100)),
        )
        .unwrap();
    assert_eq!(intf_of(&t, rif1).vrf, Vrf(1));
    assert_eq!(intf_of(&t, rif1).mtu, 9100);
    // a RIF in an unknown VR; a loopback RIF in a VRF (no SDK object)
    let unknown_vr = RawOid((u64::from(sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER) << 56) | 0x63);
    assert_eq!(
        t.rif(unknown_vr, sys::SAI_ROUTER_INTERFACE_TYPE_LOOPBACK, None),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    let intfs = t.chip().intfs.len();
    let lo1 = t.rif(vr1, sys::SAI_ROUTER_INTERFACE_TYPE_LOOPBACK, None).unwrap();
    assert_eq!(t.chip().intfs.len(), intfs);

    // the same neighbor address in both VRFs: one host entry each
    let ip = ip4(10, 9, 0, 1);
    t.nbr(rif1, ip, 0x11).unwrap();
    t.nbr(rif0, ip, 0x12).unwrap();
    assert!(t.host_egress(1, ip).is_some() && t.host_egress(0, ip).is_some());
    assert_ne!(t.host_egress(1, ip), t.host_egress(0, ip));
    // a loopback RIF has no neighbors
    assert_eq!(t.nbr(lo1, ip, 0x13), Err(Error::NotSupported));

    // the same prefix in two VRs; a route in vr1 leaking to a next hop in the default VR
    let nh1 = t.nh(rif1, ip).unwrap();
    let nh0 = t.nh(rif0, ip).unwrap();
    let p = prefix(198, 51, 100, 0, 24);
    let leak = prefix(203, 0, 113, 0, 24);
    t.route_nh(vr1, p, nh1).unwrap();
    t.route(dvr, p, &[action(sys::SAI_PACKET_ACTION_DROP)]).unwrap();
    t.route_nh(vr1, leak, nh0).unwrap();
    let me = prefix(10, 9, 0, 0, 32);
    let cpu = oid(get1(&t.sai, SWITCH, t.sw, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    t.route_nh(vr1, me, cpu).unwrap();
    assert_eq!(
        t.route_target(1, p),
        Some(RouteTarget::Egress(t.neighbor_egress(rif1, ip)))
    );
    let drop = t.drop_egress();
    assert_eq!(t.route_target(0, p), Some(RouteTarget::Egress(drop)));
    assert_eq!(
        t.route_target(1, leak),
        Some(RouteTarget::Egress(t.neighbor_egress(rif0, ip)))
    );
    assert_eq!(t.egress(egress_of(t.route_target(1, me))), EgressKind::Cpu);
    assert_eq!(
        t.sai
            .get_entry(t.sw, &route_key(vr1, p), &[sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID]),
        Ok(vec![Value::Oid(nh1)])
    );
    assert_eq!(t.route_nh(vr1, p, nh1), Err(Error::ItemAlreadyExists));
    assert_eq!(
        t.route(
            unknown_vr,
            prefix(10, 0, 0, 0, 8),
            &[action(sys::SAI_PACKET_ACTION_DROP)]
        ),
        Err(Error::InvalidParameter)
    );
    assert_eq!(t.sai.remove(VR, vr1), Err(Error::ObjectInUse));

    // teardown
    for (v, q) in [(vr1, p), (dvr, p), (vr1, leak), (vr1, me)] {
        t.sai.remove_entry(t.sw, &route_key(v, q)).unwrap();
    }
    t.sai.remove(NH, nh1).unwrap();
    t.sai.remove(NH, nh0).unwrap();
    t.sai.remove_entry(t.sw, &nbr_key(rif1, ip)).unwrap();
    assert!(t.host_egress(1, ip).is_none() && t.host_egress(0, ip).is_some());
    t.sai.remove_entry(t.sw, &nbr_key(rif0, ip)).unwrap();
    assert_eq!(t.sai.remove(VR, vr1), Err(Error::ObjectInUse)); // RIFs still in it
    t.sai.remove(RIF, rif1).unwrap();
    t.sai.remove(RIF, lo1).unwrap();
    t.sai.remove(RIF, rif0).unwrap();
    t.sai.remove(VR, vr1).unwrap();
    t.sai.remove(VR, vr2).unwrap();
    assert_eq!(t.sai.remove(VR, vr2), Err(Error::InvalidObjectId));
    assert!(t.chip().routes.is_empty() && t.chip().hosts.is_empty());
    let store = t.sai.store().unwrap();
    assert_eq!(store.iter::<l3::VirtualRouter>().count(), 1);

    // VRF ids run out at the chip's maximum (virtual: 1023)
    let mut many = Vec::new();
    loop {
        match t.vr(&[]) {
            Ok(v) => many.push(v),
            Err(e) => {
                assert_eq!(e, Error::InsufficientResources);
                break;
            }
        }
        assert!(many.len() <= 5000);
    }
    assert_eq!(many.len(), 1023);
    for v in many {
        t.sai.remove(VR, v).unwrap();
    }
    common::model_and_chip_agree(&t.sai).unwrap();
}

/// C++ test_ecmp.
#[test]
fn next_hop_groups_are_ecmp_groups_of_resolved_members() {
    let mut t = T::new();
    let dvr = t.vr0;
    let (p6, p7) = (t.ports[6], t.ports[7]);
    let rif6 = t.port_rif(dvr, p6);
    let rif7 = t.port_rif(dvr, p7);
    let (ipa, ipb, ipc) = (ip4(10, 8, 0, 1), ip4(10, 8, 1, 1), ip4(10, 8, 2, 1));
    t.nbr(rif6, ipa, 0x21).unwrap();
    let nha = t.nh(rif6, ipa).unwrap();
    let nhb = t.nh(rif7, ipb).unwrap(); // b, c unresolved
    let nhc = t.nh(rif7, ipc).unwrap();
    let ea = t.neighbor_egress(rif6, ipa);

    // group create: TYPE is mandatory, only (unordered) ECMP
    assert_eq!(t.sai.create(NHG, t.sw, &[]), Err(Error::MandatoryAttributeMissing));
    assert_eq!(
        t.sai.create(
            NHG,
            t.sw,
            &[Attr::new(
                sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE,
                enumerator(sys::SAI_NEXT_HOP_GROUP_TYPE_DYNAMIC_ORDERED_ECMP)
            )]
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    let grp = t.group();
    assert_eq!(grp.sai_object_type(), sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP);

    // a route on the empty group drops; no SDK group yet
    let rt = prefix(192, 0, 2, 0, 24);
    t.route_nh(dvr, rt, grp).unwrap();
    let drop = t.drop_egress();
    assert_eq!(t.route_target(0, rt), Some(RouteTarget::Egress(drop)));
    assert!(t.chip().ecmps.is_empty());

    // first resolved member: the SDK group appears and the route follows
    let ma = t.member(grp, nha, 0).unwrap();
    assert_eq!(t.chip().ecmps.len(), 1);
    let ecmp = *t.chip().ecmps.keys().next().unwrap();
    assert_eq!(t.route_target(0, rt), Some(RouteTarget::Ecmp(ecmp)));
    assert_eq!(t.ecmp_members(ecmp), [ea]);
    assert_eq!(t.u32_of(sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY), 4095);
    // unresolved members are kept, not programmed; the neighbor brings them in
    let mb = t.member(grp, nhb, 0).unwrap();
    let mc = t.member(grp, nhc, 2).unwrap();
    assert_eq!(t.ecmp_members(ecmp), [ea]);
    assert_eq!(
        t.sai.get(
            NHG,
            grp,
            &[
                sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT,
                sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST
            ]
        ),
        Ok(vec![Value::U32(3), Value::OidList(vec![ma, mb, mc])])
    );
    t.nbr(rif7, ipb, 0x22).unwrap();
    let eb = t.neighbor_egress(rif7, ipb);
    assert_eq!(t.ecmp_members(ecmp), [ea, eb]);
    assert_eq!(t.route_target(0, rt), Some(RouteTarget::Ecmp(ecmp))); // same SDK group, updated in place
    t.nbr(rif7, ipc, 0x23).unwrap();
    let ec = t.neighbor_egress(rif7, ipc);
    assert_eq!(t.ecmp_members(ecmp), [ea, eb, ec, ec]); // weight 2
    assert_eq!(
        t.u32_of(sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY),
        16384 - 4
    );

    // member attributes
    let weight = |w| Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT, Value::U32(w));
    assert_eq!(
        t.sai.set(NHGM, ma, &weight(0)),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    t.sai.set(NHGM, ma, &weight(3)).unwrap();
    assert_eq!(t.ecmp_members(ecmp), [ea, ea, ea, eb, ec, ec]);
    t.sai.set(NHGM, ma, &weight(100)).unwrap(); // more paths than the chip's 64: the rest is left out
    assert_eq!(t.ecmp_members(ecmp).len(), 64);
    t.sai.set(NHGM, ma, &weight(1)).unwrap();
    assert_eq!(
        t.sai.get(
            NHGM,
            mc,
            &[
                sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID,
                sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID,
                sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT
            ]
        ),
        Ok(vec![Value::Oid(grp), Value::Oid(nhc), Value::U32(2)])
    );
    let to = |nh| Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID, Value::Oid(nh));
    t.sai.set(NHGM, mc, &to(nha)).unwrap(); // a member moves to another next hop
    assert_eq!(t.ecmp_members(ecmp), [ea, eb, ea, ea]);
    t.sai.set(NHGM, mc, &to(nhc)).unwrap();

    // in use
    assert_eq!(t.sai.remove(NH, nha), Err(Error::ObjectInUse));
    assert_eq!(t.sai.remove(NHG, grp), Err(Error::ObjectInUse));
    assert_eq!(t.sai.remove_entry(t.sw, &nbr_key(rif6, ipa)), Err(Error::ObjectInUse));
    {
        use sai_core::backend::L3 as _;
        assert_eq!(t.sai.backend_mut().l3_egress_destroy(ea), Err(Error::ObjectInUse));
    }

    // ECMP in a VRF: a route in vr1 on the same group (egress objects have no VRF)
    let vr1 = t.vr(&[]).unwrap();
    t.route_nh(vr1, rt, grp).unwrap();
    assert_eq!(t.route_target(t.vrf(vr1), rt), Some(RouteTarget::Ecmp(ecmp)));
    t.sai.remove_entry(t.sw, &route_key(vr1, rt)).unwrap();
    t.sai.remove(VR, vr1).unwrap();

    // members leave; the last one takes the SDK group with it, the route drops
    t.sai.remove(NHGM, ma).unwrap();
    assert_eq!(t.ecmp_members(ecmp), [eb, ec, ec]);
    t.sai.remove(NHGM, mc).unwrap();
    t.sai.remove(NHGM, mb).unwrap();
    assert!(t.chip().ecmps.is_empty());
    assert_eq!(t.route_target(0, rt), Some(RouteTarget::Egress(drop)));
    assert_eq!(t.sai.remove(NHGM, mb), Err(Error::InvalidObjectId));
    // a route moved from a next hop to a group and back
    t.sai.set_entry(t.sw, &route_key(dvr, rt), &next_hop_attr(nhb)).unwrap();
    assert_eq!(t.route_target(0, rt), Some(RouteTarget::Egress(eb)));
    let m2 = t.member(grp, nhb, 0).unwrap();
    t.sai.set_entry(t.sw, &route_key(dvr, rt), &next_hop_attr(grp)).unwrap();
    let ecmp2 = *t.chip().ecmps.keys().next().unwrap();
    assert_eq!(t.route_target(0, rt), Some(RouteTarget::Ecmp(ecmp2)));
    t.sai.remove_entry(t.sw, &route_key(dvr, rt)).unwrap();
    t.sai.remove(NHGM, m2).unwrap();
    t.sai.remove(NHG, grp).unwrap();
    assert_eq!(t.sai.remove(NHG, grp), Err(Error::InvalidObjectId));
    assert_eq!(t.u32_of(sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY), 4096);
    assert_eq!(
        t.u32_of(sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY),
        16384
    );
    // a member on an unknown group
    assert_eq!(t.member(grp, nhb, 0), Err(Error::attr(AttrError::InvalidValue, 0)));

    // teardown
    for nh in [nha, nhb, nhc] {
        t.sai.remove(NH, nh).unwrap();
    }
    for (rif, ip) in [(rif6, ipa), (rif7, ipb), (rif7, ipc)] {
        t.sai.remove_entry(t.sw, &nbr_key(rif, ip)).unwrap();
    }
    t.sai.remove(RIF, rif6).unwrap();
    t.sai.remove(RIF, rif7).unwrap();
    assert!(t.chip().routes.is_empty() && t.chip().hosts.is_empty());
    common::model_and_chip_agree(&t.sai).unwrap();
}

/// The router interface on a LAG of C++ test_lag: the internal VLAN on every member, egress to
/// the trunk; members that join and leave follow.
#[test]
fn a_router_interface_on_a_lag() {
    let mut t = T::new();
    let (p4, p5, p6) = (t.ports[4], t.ports[5], t.ports[6]);
    // orchagent removes the default bridge ports first
    for p in [p4, p5, p6] {
        let bports = oid_list(
            get1(
                &t.sai,
                sys::SAI_OBJECT_TYPE_BRIDGE,
                oid(get1(&t.sai, SWITCH, t.sw, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID).unwrap()),
                sys::SAI_BRIDGE_ATTR_PORT_LIST,
            )
            .unwrap(),
        );
        for bp in bports {
            if get1(&t.sai, BRIDGE_PORT, bp, sys::SAI_BRIDGE_PORT_ATTR_PORT_ID) == Ok(Value::Oid(p)) {
                let members = oid_list(
                    get1(
                        &t.sai,
                        VLAN,
                        oid(get1(&t.sai, SWITCH, t.sw, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID).unwrap()),
                        sys::SAI_VLAN_ATTR_MEMBER_LIST,
                    )
                    .unwrap(),
                );
                for m in members {
                    if get1(&t.sai, VLAN_MEMBER, m, sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID) == Ok(Value::Oid(bp)) {
                        t.sai.remove(VLAN_MEMBER, m).unwrap();
                    }
                }
                t.sai.remove(BRIDGE_PORT, bp).unwrap();
            }
        }
    }
    let lag = t.sai.create(LAG, t.sw, &[]).unwrap();
    let member = |t: &mut T, port| {
        t.sai.create(
            LAG_MEMBER,
            t.sw,
            &[
                Attr::new(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, Value::Oid(lag)),
                Attr::new(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(port)),
            ],
        )
    };
    let m4 = member(&mut t, p4).unwrap();
    let m5 = member(&mut t, p5).unwrap();
    let rif = t.port_rif(t.vr0, lag);
    let ivid = t.rif_vid(rif).get();
    assert_eq!(t.vlan_member(ivid, 17), Some(true));
    assert_eq!(t.vlan_member(ivid, 21), Some(true));
    assert!(t.pvid(17) == ivid && t.pvid(21) == ivid);
    assert_eq!(
        get1(&t.sai, LAG, lag, sys::SAI_LAG_ATTR_PORT_VLAN_ID),
        Ok(Value::U16(ivid))
    );
    // not on a member
    assert_eq!(
        t.rif(t.vr0, sys::SAI_ROUTER_INTERFACE_TYPE_PORT, Some(p4)),
        Err(Error::InvalidParameter)
    );
    let m6 = member(&mut t, p6).unwrap(); // joins the internal VLAN
    assert!(t.vlan_member(ivid, 25).is_some() && t.pvid(25) == ivid);
    t.sai.remove(LAG_MEMBER, m6).unwrap();
    assert!(t.vlan_member(ivid, 25).is_none());
    let ip = ip4(10, 1, 0, 2);
    t.nbr(rif, ip, 0x07).unwrap();
    let lag_id = sai_core::oid::LagId::try_from(lag).unwrap();
    let tid = t.sai.store().unwrap().get::<sai_core::lag::Lag>(lag_id).unwrap().tid;
    assert!(matches!(
        t.egress(t.host_egress(0, ip).unwrap()),
        EgressKind::Forward { dest: Dest::Lag(trunk), .. } if trunk == tid
    ));
    assert_eq!(t.sai.remove(LAG, lag), Err(Error::ObjectInUse));
    t.sai.remove_entry(t.sw, &nbr_key(rif, ip)).unwrap();
    t.sai.remove(RIF, rif).unwrap();
    assert!(!t.vlan_exists(ivid));
    assert!(t.pvid(17) == 1 && t.pvid(21) == 1);
    t.sai.remove(LAG_MEMBER, m4).unwrap();
    t.sai.remove(LAG_MEMBER, m5).unwrap();
    t.sai.remove(LAG, lag).unwrap();
    common::model_and_chip_agree(&t.sai).unwrap();
}

/// What the C++ only keeps, its defaults, and the attributes it refuses.
#[test]
fn kept_attributes_defaults_and_refusals() {
    let mut t = T::new();
    let p = t.ports[1];
    let rif = t
        .sai
        .create(
            RIF,
            t.sw,
            &[
                Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(t.vr0)),
                Attr::new(
                    sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
                    enumerator(sys::SAI_ROUTER_INTERFACE_TYPE_PORT),
                ),
                Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, Value::Oid(p)),
                Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_NAT_ZONE_ID, Value::U8(3)),
                Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_IS_VIRTUAL, Value::Bool(false)),
            ],
        )
        .unwrap();
    assert_eq!(
        t.sai.get(
            RIF,
            rif,
            &[
                sys::SAI_ROUTER_INTERFACE_ATTR_NAT_ZONE_ID,
                sys::SAI_ROUTER_INTERFACE_ATTR_NEIGHBOR_MISS_PACKET_ACTION,
                sys::SAI_ROUTER_INTERFACE_ATTR_MTU,
                sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE,
                sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
                sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID,
                sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID,
            ]
        ),
        Ok(vec![
            Value::U8(3),
            enumerator(sys::SAI_PACKET_ACTION_TRAP),
            Value::U32(1514),
            Value::Bool(true),
            enumerator(sys::SAI_ROUTER_INTERFACE_TYPE_PORT),
            Value::Oid(p),
            Value::Oid(RawOid::NULL),
        ])
    );
    // a kept create-only attribute is refused on set (C++ attr_invalid)
    assert_eq!(
        t.sai.set(
            RIF,
            rif,
            &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_V4_MCAST_ENABLE, Value::Bool(true))
        ),
        Ok(())
    );
    // admin state: the backend has no control for it (NotSupported): kept only
    t.sai
        .set(
            RIF,
            rif,
            &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE, Value::Bool(false)),
        )
        .unwrap();
    assert_eq!(
        get1(&t.sai, RIF, rif, sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V6_STATE),
        Ok(Value::Bool(false))
    );
    // a RIF's own MAC: its own my-station entry, which moves with it and goes with it
    let stations = t.chip().stations.len();
    t.sai
        .set(
            RIF,
            rif,
            &Attr::new(
                sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS,
                Value::Mac([2, 0, 0, 0, 1, 1]),
            ),
        )
        .unwrap();
    assert_eq!(t.chip().stations.len(), stations + 1);
    t.sai
        .set(
            RIF,
            rif,
            &Attr::new(
                sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS,
                Value::Mac([2, 0, 0, 0, 1, 2]),
            ),
        )
        .unwrap();
    assert_eq!(t.chip().stations.len(), stations + 1);
    assert!(t.chip().stations.values().any(|m| *m == MacAddr([2, 0, 0, 0, 1, 2])));
    // a next hop takes no set (C++: ATTR_NOT_SUPPORTED_0 for every attribute); a type other than IP
    // is refused
    let nh = t.nh(rif, ip4(10, 0, 0, 9)).unwrap();
    assert_eq!(
        t.sai.set(
            NH,
            nh,
            &Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(ip4(10, 0, 0, 8)))
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    assert_eq!(
        t.sai.create(
            NH,
            t.sw,
            &[Attr::new(
                sys::SAI_NEXT_HOP_ATTR_TYPE,
                enumerator(sys::SAI_NEXT_HOP_TYPE_MPLS)
            )]
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    // a neighbor: DST_MAC is mandatory; PACKET_ACTION is not kept (not readable, as the C++)
    let ip = ip4(10, 0, 0, 9);
    assert_eq!(
        t.sai.create_entry(t.sw, nbr_key(rif, ip), &[]),
        Err(Error::MandatoryAttributeMissing)
    );
    t.nbr(rif, ip, 9).unwrap();
    assert_eq!(
        t.sai.get_entry(
            t.sw,
            &nbr_key(rif, ip),
            &[
                sys::SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE,
                sys::SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION
            ]
        ),
        Err(Error::attr(AttrError::NotSupported, 1))
    );
    t.sai
        .set_entry(
            t.sw,
            &nbr_key(rif, ip),
            &Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac(nmac(0x44))),
        )
        .unwrap();
    let egress = t.neighbor_egress(rif, ip);
    assert!(matches!(t.egress(egress), EgressKind::Forward { dmac: MacAddr(m), .. } if m == nmac(0x44)));
    // a trapped neighbor sits on the CPU egress: its MAC cannot be changed
    let ip2 = ip4(10, 0, 0, 10);
    t.sai
        .create_entry(
            t.sw,
            nbr_key(rif, ip2),
            &[
                Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac(nmac(1))),
                Attr::new(
                    sys::SAI_NEIGHBOR_ENTRY_ATTR_PACKET_ACTION,
                    enumerator(sys::SAI_PACKET_ACTION_TRAP),
                ),
            ],
        )
        .unwrap();
    assert_eq!(t.egress(t.neighbor_egress(rif, ip2)), EgressKind::Cpu);
    assert_eq!(
        t.sai.set_entry(
            t.sw,
            &nbr_key(rif, ip2),
            &Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac(nmac(2)))
        ),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    // a route: META_DATA kept; an action a route cannot have
    let key = route_key(t.vr0, prefix(10, 7, 0, 0, 16));
    t.sai
        .create_entry(
            t.sw,
            key,
            &[Attr::new(sys::SAI_ROUTE_ENTRY_ATTR_META_DATA, Value::U32(7))],
        )
        .unwrap();
    assert_eq!(
        t.sai.get_entry(
            t.sw,
            &key,
            &[
                sys::SAI_ROUTE_ENTRY_ATTR_META_DATA,
                sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION
            ]
        ),
        Ok(vec![Value::U32(7), enumerator(sys::SAI_PACKET_ACTION_FORWARD)])
    );
    assert_eq!(
        t.sai.set_entry(t.sw, &key, &action(sys::SAI_PACKET_ACTION_LOG)),
        Err(Error::NotSupported)
    );
    // DENY drops and reads back as DENY
    t.sai
        .set_entry(t.sw, &key, &action(sys::SAI_PACKET_ACTION_DENY))
        .unwrap();
    assert_eq!(
        t.egress(egress_of(t.route_target(0, prefix(10, 7, 0, 0, 16)))),
        EgressKind::Drop
    );
    assert_eq!(
        t.sai.get_entry(t.sw, &key, &[sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION]),
        Ok(vec![enumerator(sys::SAI_PACKET_ACTION_DENY)])
    );
    // a connected-subnet route on the RIF: glean
    let subnet = route_key(t.vr0, prefix(10, 0, 0, 0, 24));
    t.sai.create_entry(t.sw, subnet, &[next_hop_attr(rif)]).unwrap();
    assert_eq!(t.sai.remove(RIF, rif), Err(Error::ObjectInUse));
    // RIF counters: the virtual backend has none
    assert_eq!(
        t.sai
            .stats_get(RIF, rif, &[sys::SAI_ROUTER_INTERFACE_STAT_IN_OCTETS], false),
        Err(Error::NotSupported)
    );
    assert_eq!(t.sai.stats_capability(RIF), Ok(vec![]));
    common::model_and_chip_agree(&t.sai).unwrap();
}

/// The switch MAC's my-station entry moves when the switch MAC changes.
#[test]
fn the_switch_mac_station_follows_the_switch_mac() {
    let mut t = T::new();
    let set_mac = |t: &mut T, m| {
        t.sai.set(
            SWITCH,
            t.sw,
            &Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(m)),
        )
    };
    // no station yet: nothing to move
    set_mac(&mut t, [2, 0, 0, 0, 0, 1]).unwrap();
    assert!(t.chip().stations.is_empty());
    let _rif = t.port_rif(t.vr0, t.ports[0]);
    assert_eq!(
        t.chip().stations.values().collect::<Vec<_>>(),
        [&MacAddr([2, 0, 0, 0, 0, 1])]
    );
    set_mac(&mut t, [2, 0, 0, 0, 0, 2]).unwrap();
    assert_eq!(
        t.chip().stations.values().collect::<Vec<_>>(),
        [&MacAddr([2, 0, 0, 0, 0, 2])]
    );
    // a failed move keeps the old MAC and station
    t.sai
        .backend_mut()
        .fail_nth("l3_station_add", 1, Error::TableFull)
        .unwrap();
    assert_eq!(set_mac(&mut t, [2, 0, 0, 0, 0, 3]), Err(Error::TableFull));
    assert_eq!(
        get1(&t.sai, SWITCH, t.sw, sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS),
        Ok(Value::Mac([2, 0, 0, 0, 0, 2]))
    );
    assert_eq!(
        t.chip().stations.values().collect::<Vec<_>>(),
        [&MacAddr([2, 0, 0, 0, 0, 2])]
    );
}

/// `SAI_OPENBCM_RIF_VLAN_BASE` moves the internal VLANs; a bad value refuses the switch.
#[test]
fn the_internal_vlan_base_comes_from_the_profile() {
    let mut sai = Sai::new(VirtualBackend::new());
    let pairs = |base: &str| {
        Profile::from_pairs([
            ("SAI_RS_VIRTUAL_PORTS", "4".to_owned()),
            (l3::RIF_VLAN_BASE_KEY, base.to_owned()),
        ])
    };
    assert_eq!(
        sai.create_switch(&init_attrs(), &pairs("4095")),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.create_switch(&init_attrs(), &pairs("x")),
        Err(Error::InvalidParameter)
    );
    let sw = sai.create_switch(&init_attrs(), &pairs("3000")).unwrap().raw();
    let ports = oid_list(get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let vr = oid(get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID).unwrap());
    sai.create(RIF, sw, &common::port_rif(vr, ports[0])).unwrap();
    assert!(sai.backend().vlans().contains_key(&Vid::new(3000).unwrap()));
    // the next one counts down, past a VLAN that exists
    sai.create(VLAN, sw, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(2999))])
        .unwrap();
    sai.create(RIF, sw, &common::port_rif(vr, ports[1])).unwrap();
    assert!(sai.backend().vlans().contains_key(&Vid::new(2998).unwrap()));
}

// ------------------------------------------------------------------------ failure injection

/// What a failed call must leave as it was: the L3 objects and tables of the model, the chip's
/// L3 tables, VLANs and port VLAN ids.
fn snapshot(sai: &Sai<VirtualBackend>) -> String {
    let store = sai.store().unwrap();
    let objects: Vec<String> = store
        .iter::<l3::VirtualRouter>()
        .map(|o| format!("{o:?}"))
        .chain(store.iter::<l3::RouterInterface>().map(|o| format!("{o:?}")))
        .chain(store.iter::<l3::NextHop>().map(|o| format!("{o:?}")))
        .chain(store.iter::<l3::NextHopGroup>().map(|o| format!("{o:?}")))
        .chain(store.iter::<l3::NextHopGroupMember>().map(|o| format!("{o:?}")))
        .collect();
    let pvids: Vec<u16> = (0..8u16)
        .map(|i| sai.backend().port_state(HwPort(1 + 4 * i)).unwrap().pvid.get())
        .collect();
    format!(
        "{objects:?}\n{:?}\n{:?}\n{:?}\n{pvids:?}",
        l3::tables(store),
        sai.backend().l3(),
        sai.backend().vlans()
    )
}

/// A switch with a resolved next hop on a port RIF, an unresolved one, a group with the resolved
/// one, a route on each; the shared egress objects made (so a lazily made one is no change).
struct Setup {
    t: T,
    rif: RawOid,
    nh: RawOid,
    nh_unresolved: RawOid,
    group: RawOid,
    member: RawOid,
    spare: RawOid,
}

fn setup() -> Setup {
    use sai_core::backend::L3 as _;
    let mut t = T::new();
    t.sai.backend_mut().l3_egress_cpu().unwrap();
    t.sai.backend_mut().l3_egress_drop().unwrap();
    let rif = t.port_rif(t.vr0, t.ports[0]);
    t.nbr(rif, ip4(10, 0, 0, 2), 2).unwrap();
    let nh = t.nh(rif, ip4(10, 0, 0, 2)).unwrap();
    let nh_unresolved = t.nh(rif, ip4(10, 0, 0, 3)).unwrap();
    let group = t.group();
    let member = t.member(group, nh, 0).unwrap();
    let spare = t.port_rif(t.vr0, t.ports[2]);
    t.route_nh(t.vr0, prefix(192, 0, 2, 0, 24), group).unwrap();
    t.route_nh(t.vr0, prefix(198, 51, 100, 0, 24), nh_unresolved).unwrap();
    Setup {
        t,
        rif,
        nh,
        nh_unresolved,
        group,
        member,
        spare,
    }
}

type Step = fn(&mut Setup) -> sai_core::Result<()>;

const L3_OPS: &[&str] = &[
    "vlan_create",
    "vlan_port_add",
    "vlan_port_remove",
    "vlan_destroy",
    "port_pvid_set",
    "l3_intf_create",
    "l3_intf_update",
    "l3_intf_destroy",
    "l3_intf_admin_set",
    "l3_station_add",
    "l3_station_delete",
    "l3_egress_create",
    "l3_egress_update",
    "l3_egress_destroy",
    "l3_egress_cpu",
    "l3_egress_drop",
    "l3_host_add",
    "l3_host_delete",
    "l3_route_add",
    "l3_route_delete",
    "l3_ecmp_create",
    "l3_ecmp_update",
    "l3_ecmp_destroy",
    "chip_limits",
];

#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let steps: &[(&str, Step)] = &[
        ("port RIF with its own MAC", |s| {
            let attrs = vec![
                Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(s.t.vr0)),
                Attr::new(
                    sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
                    enumerator(sys::SAI_ROUTER_INTERFACE_TYPE_PORT),
                ),
                Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, Value::Oid(s.t.ports[1])),
                Attr::new(
                    sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS,
                    Value::Mac([2, 0, 0, 0, 9, 9]),
                ),
            ];
            s.t.sai.create(RIF, s.t.sw, &attrs).map(drop)
        }),
        ("VLAN RIF", |s| {
            let v = s.t.vlan(300)?;
            let r = s.t.rif(s.t.vr0, sys::SAI_ROUTER_INTERFACE_TYPE_VLAN, Some(v));
            if r.is_err() {
                s.t.sai.remove(VLAN, v)?;
            }
            r.map(drop)
        }),
        ("virtual router", |s| s.t.vr(&[]).map(drop)),
        ("neighbor that resolves a next hop with a route", |s| {
            s.t.nbr(s.rif, ip4(10, 0, 0, 3), 3)
        }),
        ("neighbor without host route", |s| {
            s.t.sai.create_entry(
                s.t.sw,
                nbr_key(s.rif, ip4(10, 0, 0, 4)),
                &[
                    Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac(nmac(4))),
                    Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE, Value::Bool(true)),
                ],
            )
        }),
        ("next hop without neighbor", |s| {
            s.t.nh(s.rif, ip4(10, 0, 0, 5)).map(drop)
        }),
        ("route", |s| s.t.route_nh(s.t.vr0, prefix(203, 0, 113, 0, 24), s.nh)),
        ("route moved to another next hop", |s| {
            s.t.sai.set_entry(
                s.t.sw,
                &route_key(s.t.vr0, prefix(198, 51, 100, 0, 24)),
                &next_hop_attr(s.nh),
            )
        }),
        ("route removed", |s| {
            s.t.sai
                .remove_entry(s.t.sw, &route_key(s.t.vr0, prefix(198, 51, 100, 0, 24)))
        }),
        ("second group member", |s| s.t.member(s.group, s.nh, 2).map(drop)),
        ("member weight", |s| {
            s.t.sai.set(
                NHGM,
                s.member,
                &Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT, Value::U32(4)),
            )
        }),
        ("member to an unresolved next hop", |s| {
            s.t.sai.set(
                NHGM,
                s.member,
                &Attr::new(
                    sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID,
                    Value::Oid(s.nh_unresolved),
                ),
            )
        }),
        ("last member removed", |s| s.t.sai.remove(NHGM, s.member)),
        ("RIF MTU", |s| {
            s.t.sai.set(
                RIF,
                s.rif,
                &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_MTU, Value::U32(9000)),
            )
        }),
        ("RIF MAC", |s| {
            s.t.sai.set(
                RIF,
                s.rif,
                &Attr::new(
                    sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS,
                    Value::Mac([2, 0, 0, 0, 7, 7]),
                ),
            )
        }),
        ("RIF admin state", |s| {
            s.t.sai.set(
                RIF,
                s.rif,
                &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_ADMIN_V4_STATE, Value::Bool(false)),
            )
        }),
        ("neighbor MAC", |s| {
            s.t.sai.set_entry(
                s.t.sw,
                &nbr_key(s.rif, ip4(10, 0, 0, 2)),
                &Attr::new(sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS, Value::Mac(nmac(0x55))),
            )
        }),
        ("RIF removed", |s| s.t.sai.remove(RIF, s.spare)),
    ];
    let mut injected = 0;
    for (name, step) in steps {
        for op in L3_OPS {
            for n in 1..=4 {
                let mut s = setup();
                // the reference run: what the step does without a fault
                let before = snapshot(&s.t.sai);
                s.t.sai.backend_mut().fail_nth(op, n, Error::TableFull).unwrap();
                // Ok: the fault was not reached, or the C++ ignores it there
                if step(&mut s).is_ok() {
                    break;
                }
                injected += 1;
                assert_eq!(snapshot(&s.t.sai), before, "{name}: {op} #{n} failed and left changes");
                common::model_and_chip_agree(&s.t.sai).unwrap();
            }
        }
    }
    assert!(injected >= 20, "only {injected} faults reached a step");
}

// ------------------------------------------------------------------------------ call log

/// tests/driver-scripts/06-l3.ops of sai-openbcm, run by the C++ frontend_driver on the C++
/// library (fake backend behind its recording backend): every op after the switch, its status
/// and the backend calls it made (`be` lines; the switch creation's own lines differ by design).
const CPP_06_L3: &str = include_str!("l3-06-cpp.txt");

fn status_name(result: sai_core::Result<()>) -> String {
    let code = match result {
        Ok(()) => return "SUCCESS".to_owned(),
        Err(e) => e.status(),
    };
    let name = match code {
        sys::SAI_STATUS_ITEM_ALREADY_EXISTS => "ITEM_ALREADY_EXISTS",
        sys::SAI_STATUS_OBJECT_IN_USE => "OBJECT_IN_USE",
        sys::SAI_STATUS_INVALID_PARAMETER => "INVALID_PARAMETER",
        sys::SAI_STATUS_NOT_SUPPORTED => "NOT_SUPPORTED",
        sys::SAI_STATUS_ITEM_NOT_FOUND => "ITEM_NOT_FOUND",
        sys::SAI_STATUS_INVALID_OBJECT_ID => "INVALID_OBJECT_ID",
        other => return other.to_string(),
    };
    name.to_owned()
}

struct Driver {
    sai: Sai<Recording<VirtualBackend>>,
    log: CallLog,
    sw: RawOid,
    names: std::collections::BTreeMap<&'static str, RawOid>,
    out: String,
}

impl Driver {
    fn id(&self, name: &str) -> RawOid {
        *self.names.get(name).unwrap_or_else(|| panic!("no {name}"))
    }

    fn record(&mut self, op: &str, result: sai_core::Result<()>) {
        use std::fmt::Write as _;
        writeln!(self.out, "> {op}\n= {}", status_name(result)).unwrap();
        for line in self.log.take() {
            writeln!(self.out, "  {line}").unwrap();
        }
    }

    fn create(&mut self, op: &str, name: &'static str, object_type: sys::sai_object_type_t, attrs: &[Attr]) {
        let result = self.sai.create(object_type, self.sw, attrs);
        if let Ok(id) = result {
            self.names.insert(name, id);
        }
        self.record(op, result.map(drop));
    }

    fn entry(&mut self, op: &str, key: EntryKey, attrs: &[Attr]) {
        let result = self.sai.create_entry(self.sw, key, attrs);
        self.record(op, result);
    }
}

#[test]
fn the_call_log_of_06_l3_is_the_cpp_one() {
    let rec = Recording::new(VirtualBackend::new(), Mode::Normal);
    let log = rec.log();
    let (sai, sw) = cpp_switch(rec, 8);
    log.take();
    let ports = oid_list(get1_rec(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_LIST));
    let mut names = std::collections::BTreeMap::new();
    names.insert(
        "vr0",
        oid(get1_rec(
            &sai,
            SWITCH,
            sw,
            sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
        )),
    );
    names.insert("cpu", oid(get1_rec(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_CPU_PORT)));
    for (i, name) in ["port0", "port1", "port2", "port3", "port4", "port5", "port6", "port7"]
        .into_iter()
        .enumerate()
    {
        names.insert(name, ports[i]);
    }
    log.take();
    let mut d = Driver {
        sai,
        log,
        sw,
        names,
        out: String::new(),
    };
    let rif_attrs = |d: &Driver, vr: &str, port: &str| {
        vec![
            Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(d.id(vr))),
            Attr::new(
                sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
                enumerator(sys::SAI_ROUTER_INTERFACE_TYPE_PORT),
            ),
            Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID, Value::Oid(d.id(port))),
        ]
    };
    let nh_attrs = |d: &Driver, ip: IpAddr, rif: &str| {
        vec![
            Attr::new(sys::SAI_NEXT_HOP_ATTR_TYPE, enumerator(sys::SAI_NEXT_HOP_TYPE_IP)),
            Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(ip)),
            Attr::new(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, Value::Oid(d.id(rif))),
        ]
    };
    let dmac = |last| {
        [Attr::new(
            sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS,
            Value::Mac(nmac(last)),
        )]
    };
    let member_attrs = |d: &Driver, nh: &str, weight: Option<u32>| {
        let mut a = vec![
            Attr::new(
                sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID,
                Value::Oid(d.id("grp")),
            ),
            Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID, Value::Oid(d.id(nh))),
        ];
        if let Some(w) = weight {
            a.push(Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT, Value::U32(w)));
        }
        a
    };
    let rkey = |d: &Driver, p: IpPrefix, vr: &str| route_key(d.id(vr), p);
    let nkey = |d: &Driver, ip: IpAddr, rif: &str| nbr_key(d.id(rif), ip);

    let a = rif_attrs(&d, "vr0", "port3");
    d.create(
        "create router_interface rif3 VIRTUAL_ROUTER_ID=vr0 TYPE=PORT PORT_ID=port3",
        "rif3",
        RIF,
        &a,
    );
    d.create(
        "create router_interface rifdup VIRTUAL_ROUTER_ID=vr0 TYPE=PORT PORT_ID=port3",
        "rifdup",
        RIF,
        &a,
    );
    let a = nh_attrs(&d, ip4(10, 0, 0, 2), "rif3");
    d.create(
        "create next_hop nh1 TYPE=IP IP=10.0.0.2 ROUTER_INTERFACE_ID=rif3",
        "nh1",
        NH,
        &a,
    );
    let (k, a) = (
        rkey(&d, prefix(192, 168, 0, 0, 24), "vr0"),
        [next_hop_attr(d.id("nh1"))],
    );
    d.entry("create route_entry 192.168.0.0/24@vr0 NEXT_HOP_ID=nh1", k, &a);
    let k = nkey(&d, ip4(10, 0, 0, 2), "rif3");
    d.entry(
        "create neighbor_entry 10.0.0.2@rif3 DST_MAC_ADDRESS=00:00:5e:00:53:01",
        k,
        &dmac(0x01),
    );
    let (k, a) = (rkey(&d, prefix(10, 0, 0, 1, 32), "vr0"), [next_hop_attr(d.id("cpu"))]);
    d.entry("create route_entry 10.0.0.1/32@vr0 NEXT_HOP_ID=cpu", k, &a);
    let k = rkey(&d, prefix(0, 0, 0, 0, 0), "vr0");
    d.entry(
        "create route_entry 0.0.0.0/0@vr0 PACKET_ACTION=DROP",
        k,
        &[action(sys::SAI_PACKET_ACTION_DROP)],
    );
    let k = rkey(&d, prefix(192, 168, 0, 77, 24), "vr0");
    let got = d.sai.get_entry(
        d.sw,
        &k,
        &[
            sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID,
            sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION,
        ],
    );
    assert_eq!(
        got,
        Ok(vec![
            Value::Oid(d.id("nh1")),
            enumerator(sys::SAI_PACKET_ACTION_FORWARD)
        ])
    );
    d.record(
        "get route_entry 192.168.0.77/24@vr0 NEXT_HOP_ID PACKET_ACTION",
        got.map(drop),
    );
    let vmac = [0x02, 0x00, 0x00, 0x00, 0x0a, 0x01];
    d.create(
        "create virtual_router vr1 SRC_MAC_ADDRESS=02:00:00:00:0a:01",
        "vr1",
        VR,
        &[Attr::new(
            sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS,
            Value::Mac(vmac),
        )],
    );
    let got = d
        .sai
        .get(VR, d.id("vr1"), &[sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS]);
    assert_eq!(got, Ok(vec![Value::Mac(vmac)]));
    d.record("get virtual_router vr1 SRC_MAC_ADDRESS", got.map(drop));
    let a = rif_attrs(&d, "vr1", "port6");
    d.create(
        "create router_interface rif6 VIRTUAL_ROUTER_ID=vr1 TYPE=PORT PORT_ID=port6",
        "rif6",
        RIF,
        &a,
    );
    let a = rif_attrs(&d, "vr0", "port7");
    d.create(
        "create router_interface rif7 VIRTUAL_ROUTER_ID=vr0 TYPE=PORT PORT_ID=port7",
        "rif7",
        RIF,
        &a,
    );
    let r = d.sai.set(
        RIF,
        d.id("rif6"),
        &Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_MTU, Value::U32(9100)),
    );
    d.record("set router_interface rif6 MTU=9100", r);
    let k = nkey(&d, ip4(10, 9, 0, 1), "rif6");
    d.entry(
        "create neighbor_entry 10.9.0.1@rif6 DST_MAC_ADDRESS=00:00:5e:00:53:11",
        k,
        &dmac(0x11),
    );
    let k = nkey(&d, ip4(10, 9, 0, 1), "rif7");
    d.entry(
        "create neighbor_entry 10.9.0.1@rif7 DST_MAC_ADDRESS=00:00:5e:00:53:12",
        k,
        &dmac(0x12),
    );
    let a = nh_attrs(&d, ip4(10, 9, 0, 1), "rif6");
    d.create(
        "create next_hop nh6 TYPE=IP IP=10.9.0.1 ROUTER_INTERFACE_ID=rif6",
        "nh6",
        NH,
        &a,
    );
    let a = nh_attrs(&d, ip4(10, 9, 0, 1), "rif7");
    d.create(
        "create next_hop nh7 TYPE=IP IP=10.9.0.1 ROUTER_INTERFACE_ID=rif7",
        "nh7",
        NH,
        &a,
    );
    let (k, a) = (
        rkey(&d, prefix(198, 51, 100, 0, 24), "vr1"),
        [next_hop_attr(d.id("nh6"))],
    );
    d.entry("create route_entry 198.51.100.0/24@vr1 NEXT_HOP_ID=nh6", k, &a);
    let (k, a) = (
        rkey(&d, prefix(203, 0, 113, 0, 24), "vr1"),
        [next_hop_attr(d.id("nh7"))],
    );
    d.entry("create route_entry 203.0.113.0/24@vr1 NEXT_HOP_ID=nh7", k, &a);
    let a = nh_attrs(&d, ip4(10, 8, 1, 1), "rif7");
    d.create(
        "create next_hop nhb TYPE=IP IP=10.8.1.1 ROUTER_INTERFACE_ID=rif7",
        "nhb",
        NH,
        &a,
    );
    d.create(
        "create next_hop_group grp TYPE=ECMP",
        "grp",
        NHG,
        &[Attr::new(
            sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE,
            enumerator(sys::SAI_NEXT_HOP_GROUP_TYPE_ECMP),
        )],
    );
    let (k, a) = (rkey(&d, prefix(192, 0, 2, 0, 24), "vr0"), [next_hop_attr(d.id("grp"))]);
    d.entry("create route_entry 192.0.2.0/24@vr0 NEXT_HOP_ID=grp", k, &a);
    let a = member_attrs(&d, "nh7", None);
    d.create(
        "create next_hop_group_member ma NEXT_HOP_GROUP_ID=grp NEXT_HOP_ID=nh7",
        "ma",
        NHGM,
        &a,
    );
    let a = member_attrs(&d, "nhb", Some(2));
    d.create(
        "create next_hop_group_member mb NEXT_HOP_GROUP_ID=grp NEXT_HOP_ID=nhb WEIGHT=2",
        "mb",
        NHGM,
        &a,
    );
    let k = nkey(&d, ip4(10, 8, 1, 1), "rif7");
    d.entry(
        "create neighbor_entry 10.8.1.1@rif7 DST_MAC_ADDRESS=00:00:5e:00:53:22",
        k,
        &dmac(0x22),
    );
    let r = d.sai.set(
        NHGM,
        d.id("ma"),
        &Attr::new(sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT, Value::U32(3)),
    );
    d.record("set next_hop_group_member ma WEIGHT=3", r);
    let got = d.sai.get(
        NHG,
        d.id("grp"),
        &[
            sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT,
            sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST,
        ],
    );
    assert_eq!(
        got,
        Ok(vec![Value::U32(2), Value::OidList(vec![d.id("ma"), d.id("mb")])])
    );
    d.record(
        "get next_hop_group grp NEXT_HOP_COUNT NEXT_HOP_MEMBER_LIST",
        got.map(drop),
    );
    let got = d.sai.get(
        SWITCH,
        d.sw,
        &[
            sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY,
            sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY,
        ],
    );
    assert_eq!(got, Ok(vec![Value::U32(4095), Value::U32(16379)]));
    d.record(
        "get switch switch AVAILABLE_NEXT_HOP_GROUP_ENTRY AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY",
        got.map(drop),
    );
    let remove = |d: &mut Driver, op: &str, t, name: &str| {
        let r = d.sai.remove(t, d.id(name));
        d.record(op, r);
    };
    let remove_entry = |d: &mut Driver, op: &str, k: EntryKey| {
        let r = d.sai.remove_entry(d.sw, &k);
        d.record(op, r);
    };
    remove(&mut d, "remove next_hop nh7", NH, "nh7");
    remove(&mut d, "remove next_hop_group_member ma", NHGM, "ma");
    remove(&mut d, "remove next_hop_group_member mb", NHGM, "mb");
    let k = rkey(&d, prefix(192, 0, 2, 0, 24), "vr0");
    remove_entry(&mut d, "remove route_entry 192.0.2.0/24@vr0", k);
    remove(&mut d, "remove next_hop_group grp", NHG, "grp");
    let k = rkey(&d, prefix(198, 51, 100, 0, 24), "vr1");
    remove_entry(&mut d, "remove route_entry 198.51.100.0/24@vr1", k);
    let k = rkey(&d, prefix(203, 0, 113, 0, 24), "vr1");
    remove_entry(&mut d, "remove route_entry 203.0.113.0/24@vr1", k);
    remove(&mut d, "remove next_hop nh6", NH, "nh6");
    remove(&mut d, "remove next_hop nh7", NH, "nh7");
    remove(&mut d, "remove next_hop nhb", NH, "nhb");
    let k = nkey(&d, ip4(10, 9, 0, 1), "rif6");
    remove_entry(&mut d, "remove neighbor_entry 10.9.0.1@rif6", k);
    let k = nkey(&d, ip4(10, 9, 0, 1), "rif7");
    remove_entry(&mut d, "remove neighbor_entry 10.9.0.1@rif7", k);
    let k = nkey(&d, ip4(10, 8, 1, 1), "rif7");
    remove_entry(&mut d, "remove neighbor_entry 10.8.1.1@rif7", k);
    remove(&mut d, "remove router_interface rif6", RIF, "rif6");
    remove(&mut d, "remove router_interface rif7", RIF, "rif7");
    remove(&mut d, "remove virtual_router vr1", VR, "vr1");
    let k = rkey(&d, prefix(192, 168, 0, 0, 24), "vr0");
    remove_entry(&mut d, "remove route_entry 192.168.0.0/24@vr0", k);
    remove(&mut d, "remove next_hop nh1", NH, "nh1");
    let k = nkey(&d, ip4(10, 0, 0, 2), "rif3");
    remove_entry(&mut d, "remove neighbor_entry 10.0.0.2@rif3", k);
    let k = rkey(&d, prefix(10, 0, 0, 1, 32), "vr0");
    remove_entry(&mut d, "remove route_entry 10.0.0.1/32@vr0", k);
    let k = rkey(&d, prefix(0, 0, 0, 0, 0), "vr0");
    remove_entry(&mut d, "remove route_entry 0.0.0.0/0@vr0", k);
    remove(&mut d, "remove router_interface rif3", RIF, "rif3");
    d.create(
        "create vlan v200 VLAN_ID=200",
        "v200",
        VLAN,
        &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(200))],
    );
    let a = [
        Attr::new(
            sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID,
            Value::Oid(d.id("vr0")),
        ),
        Attr::new(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            enumerator(sys::SAI_ROUTER_INTERFACE_TYPE_VLAN),
        ),
        Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID, Value::Oid(d.id("v200"))),
    ];
    d.create(
        "create router_interface vrif VIRTUAL_ROUTER_ID=vr0 TYPE=VLAN VLAN_ID=v200",
        "vrif",
        RIF,
        &a,
    );
    remove(&mut d, "remove router_interface vrif", RIF, "vrif");
    remove(&mut d, "remove vlan v200", VLAN, "v200");
    let r = d.sai.remove(SWITCH, d.sw);
    d.record("remove_switch", r);
    if d.out != CPP_06_L3 {
        let (ours, cpp): (Vec<&str>, Vec<&str>) = (d.out.lines().collect(), CPP_06_L3.lines().collect());
        let first = ours
            .iter()
            .zip(&cpp)
            .position(|(a, b)| a != b)
            .unwrap_or(ours.len().min(cpp.len()));
        panic!(
            "the call log differs from the C++ one at line {}:\n  rust: {:?}\n  c++:  {:?}\n--- rust\n{}",
            first + 1,
            ours.get(first),
            cpp.get(first),
            d.out
        );
    }
}

fn get1_rec(
    sai: &Sai<Recording<VirtualBackend>>,
    t: sys::sai_object_type_t,
    id: RawOid,
    attr: sys::sai_attr_id_t,
) -> Value {
    sai.get(t, id, &[attr]).unwrap().remove(0)
}

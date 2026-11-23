//! The switch object: creation and removal, the objects every switch has, the switch's
//! attributes. Ports `sai_switch.cpp` (792 lines; its event delivery is `crate::sai` and the
//! rim's notifier, the backend choice the rim's `backends.rs` over [`crate::keys`]).
//!
//! - **Create** ([`create`]), in the C++ order: the attribute rules ([`precheck`]: unknown
//!   attributes `ATTR_NOT_SUPPORTED`, `TYPE` other than NPU `INVALID_ATTR_VALUE`, no
//!   `INIT_SWITCH` `MANDATORY_ATTRIBUTE_MISSING`, `INIT_SWITCH=false` `NOT_SUPPORTED`, the board
//!   quirk keys), the external PHY / `MACsec` tables, backend `init`, their attach, the default
//!   objects ([`build_objects`]: CPU port, every front-panel port disabled with the quirks
//!   applied, 8 queues + 8 priority groups + 8 scheduler groups per port, default virtual
//!   router, STP instance, trap group, `LAG_HASH` / `ECMP_HASH`, the .1Q bridge with a bridge
//!   port per port, VLAN 1 with the chip's members), `FDB_AGING_TIME`, the temperature
//!   publisher ([`crate::temp`]). A quirk the chip refuses fails the switch (`FAILURE`).
//! - **Attributes** ([`get`], [`set`]): the capacity attributes from the backend's
//!   `chip_limits` / `table_usage`, the default objects, the temperatures, the `QoS` map and
//!   hash attributes (their areas), the 23 `*_NOTIFY` callbacks (stored; only port state and
//!   FDB events are raised), and 22 kept-only attributes ([`SWITCH_STUBS`]).
//! - **Remove** ([`remove`]): publisher stop, the external PHY / `MACsec` detach, host
//!   interface netdevs, backend `shutdown`.
use crate::backend::{Backend, HwPort, Profile, Vid};
use crate::error::{AttrError, Error, Result};
use crate::model::{Bag, Bridge, BridgePort, Kept, Port, Switch, Vlan, VlanMember};
use crate::oid::{BridgeId, BridgePortId, ObjectType, PortId, SwitchId, TypedId};
use crate::port::{self, PortQuirks, stub_value};
use crate::store::Store;
use crate::temp::{self, Publisher};
use crate::value::{Attr, Pointer, Value};
use crate::{acl, extphy, hash, hostif, keys, l3, log, macsec, qos};
use sai_sys as sys;
use std::collections::BTreeMap;

/// The switch's MAC until one is given (C++ `Ctx::src_mac`).
pub const DEFAULT_SRC_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x01];
/// Egress queues per front-panel port.
pub const QUEUES_PER_PORT: u8 = 8;
/// Ingress priority groups per front-panel port.
pub const IPGS_PER_PORT: u8 = 8;

/// Switch attributes accepted, kept and returned without hardware effect (C++
/// `switch_stubs`): id, the default a get answers while unset, create-only.
pub const SWITCH_STUBS: &[(sys::sai_attr_id_t, i64, bool)] = &[
    (sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED, 0, false),
    (sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED, 0, false),
    (
        sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_ALGORITHM,
        sys::SAI_HASH_ALGORITHM_CRC as i64,
        false,
    ),
    (
        sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_ALGORITHM,
        sys::SAI_HASH_ALGORITHM_CRC as i64,
        false,
    ),
    (sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_OFFSET, 0, false),
    (sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_OFFSET, 0, false),
    (sys::SAI_SWITCH_ATTR_RESTART_WARM, 0, false),
    (sys::SAI_SWITCH_ATTR_WARM_RECOVER, 0, false),
    (sys::SAI_SWITCH_ATTR_PRE_SHUTDOWN, 0, false),
    (sys::SAI_SWITCH_ATTR_FAST_API_ENABLE, 0, false),
    (sys::SAI_SWITCH_ATTR_UNINIT_DATA_PLANE_ON_REMOVAL, 1, false),
    (sys::SAI_SWITCH_ATTR_VXLAN_DEFAULT_PORT, 4789, false),
    (sys::SAI_SWITCH_ATTR_BCAST_CPU_FLOOD_ENABLE, 0, false),
    (sys::SAI_SWITCH_ATTR_MCAST_CPU_FLOOD_ENABLE, 0, false),
    (
        sys::SAI_SWITCH_ATTR_SWITCHING_MODE,
        sys::SAI_SWITCH_SWITCHING_MODE_STORE_AND_FORWARD as i64,
        false,
    ),
    (
        sys::SAI_SWITCH_ATTR_FDB_UNICAST_MISS_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_FORWARD as i64,
        false,
    ),
    (
        sys::SAI_SWITCH_ATTR_FDB_BROADCAST_MISS_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_FORWARD as i64,
        false,
    ),
    (
        sys::SAI_SWITCH_ATTR_FDB_MULTICAST_MISS_PACKET_ACTION,
        sys::SAI_PACKET_ACTION_FORWARD as i64,
        false,
    ),
    (sys::SAI_SWITCH_ATTR_COUNTER_REFRESH_INTERVAL, 1, false),
    (sys::SAI_SWITCH_ATTR_SWITCH_ID, 0, true),
    (sys::SAI_SWITCH_ATTR_SWITCH_PROFILE_ID, 0, true),
    (sys::SAI_SWITCH_ATTR_MAX_SYSTEM_CORES, 0, true),
];

/// The 23 notification callbacks a switch accepts and keeps (C++ `is_notify_attr`).
pub const NOTIFY_ATTRS: [sys::sai_attr_id_t; 23] = [
    sys::SAI_SWITCH_ATTR_BFD_SESSION_STATE_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_BRIDGE_PORT_HW_PROTECTION_SWITCHOVER_NOTIFY,
    sys::SAI_SWITCH_ATTR_EXTENDED_PORT_STATE_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY,
    sys::SAI_SWITCH_ATTR_ICMP_ECHO_SESSION_STATE_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_IPSEC_POST_STATUS_NOTIFY,
    sys::SAI_SWITCH_ATTR_IPSEC_SA_STATUS_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_MACSEC_POST_STATUS_NOTIFY,
    sys::SAI_SWITCH_ATTR_NAT_EVENT_NOTIFY,
    sys::SAI_SWITCH_ATTR_NEXT_HOP_GROUP_HW_PROTECTION_SWITCHOVER_NOTIFY,
    sys::SAI_SWITCH_ATTR_PACKET_EVENT_NOTIFY,
    sys::SAI_SWITCH_ATTR_PORT_HOST_TX_READY_NOTIFY,
    sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_QUEUE_PFC_DEADLOCK_NOTIFY,
    sys::SAI_SWITCH_ATTR_SWITCH_ASIC_SDK_HEALTH_EVENT_NOTIFY,
    sys::SAI_SWITCH_ATTR_SWITCH_IPSEC_POST_STATUS_NOTIFY,
    sys::SAI_SWITCH_ATTR_SWITCH_MACSEC_POST_STATUS_NOTIFY,
    sys::SAI_SWITCH_ATTR_SWITCH_SHUTDOWN_REQUEST_NOTIFY,
    sys::SAI_SWITCH_ATTR_SWITCH_STATE_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_TAM_EVENT_LEARN_NOTIFY,
    sys::SAI_SWITCH_ATTR_TAM_EVENT_NOTIFY,
    sys::SAI_SWITCH_ATTR_TAM_TEL_TYPE_CONFIG_CHANGE_NOTIFY,
    sys::SAI_SWITCH_ATTR_TWAMP_SESSION_EVENT_NOTIFY,
];

/// Create-time attributes for other platforms or about the caller's environment: accepted
/// and ignored (logged) (C++ `create_ignorable`).
const CREATE_IGNORED: [sys::sai_attr_id_t; 8] = [
    sys::SAI_SWITCH_ATTR_HARDWARE_ACCESS_BUS,
    sys::SAI_SWITCH_ATTR_PLATFROM_CONTEXT,
    sys::SAI_SWITCH_ATTR_REGISTER_READ,
    sys::SAI_SWITCH_ATTR_REGISTER_WRITE,
    sys::SAI_SWITCH_ATTR_FIRMWARE_PATH_NAME,
    sys::SAI_SWITCH_ATTR_FIRMWARE_LOAD_METHOD,
    sys::SAI_SWITCH_ATTR_FIRMWARE_LOAD_TYPE,
    sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO,
];

/// Whether `attr` is one of the 23 notification callbacks.
pub fn is_notify_attr(attr: sys::sai_attr_id_t) -> bool {
    NOTIFY_ATTRS.contains(&attr)
}

fn stub(attr: sys::sai_attr_id_t) -> Option<&'static (sys::sai_attr_id_t, i64, bool)> {
    SWITCH_STUBS.iter().find(|(id, ..)| *id == attr)
}

/// What a create call asks for, once its attributes passed the C++ rules.
#[derive(Debug, Default)]
struct CreateRequest {
    src_mac: Option<[u8; 6]>,
    fdb_aging: Option<u32>,
    /// Callbacks and stubs.
    kept: Kept,
}

fn request(attrs: &[Attr]) -> Result<CreateRequest> {
    let mut init = None;
    let mut req = CreateRequest::default();
    // An attribute given twice is refused (the C++ takes the last one).
    if attrs
        .iter()
        .enumerate()
        .any(|(i, a)| attrs.iter().skip(i + 1).any(|b| b.id == a.id))
    {
        return Err(Error::InvalidParameter);
    }
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(on)) => init = Some(*on),
            (sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)) => req.src_mac = Some(*mac),
            (sys::SAI_SWITCH_ATTR_TYPE, value) => {
                if *value != Value::Enum(sys::SAI_SWITCH_TYPE_NPU.cast_signed()) {
                    return Err(Error::attr(AttrError::InvalidValue, index));
                }
            }
            (id, value) if is_notify_attr(id) || stub(id).is_some() => {
                req.kept.insert(id, value.clone());
            }
            (sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(seconds)) => req.fdb_aging = Some(*seconds),
            (id, _) if CREATE_IGNORED.contains(&id) => {
                log::notice(sys::SAI_API_SWITCH, format_args!("create_switch: ignoring attr {id}"));
            }
            (id, _) => {
                log::error(
                    sys::SAI_API_SWITCH,
                    format_args!("create_switch: attr {id} not supported"),
                );
                return Err(Error::attr(AttrError::NotSupported, index));
            }
        }
    }
    match init {
        None => Err(Error::MandatoryAttributeMissing),
        Some(false) => {
            log::error(
                sys::SAI_API_SWITCH,
                format_args!("INIT_SWITCH=false (connect to existing switch) is not supported"),
            );
            Err(Error::NotSupported)
        }
        Some(true) => Ok(req),
    }
}

/// The checks of `create_switch` that come before the backend is chosen (C++ order): the
/// attributes, `INIT_SWITCH`, the board quirk keys, the external PHY and `MACsec` tables. The
/// rim calls it before it picks the backend; [`create`] runs it again.
pub fn precheck(attrs: &[Attr], profile: &Profile) -> Result<()> {
    request(attrs)?;
    PortQuirks::parse(profile)?;
    // SAI_OPENBCM_EXT_PHY_FILE / _EXT_MACSEC_FILE: a bad table refuses the switch before the SDK starts.
    extphy::ext_phy_load(profile)?;
    macsec::ext_macsec_load(profile)?;
    Ok(())
}

/// A switch that [`create`] brought up.
#[derive(Debug)]
pub(crate) struct Created {
    pub(crate) id: SwitchId,
    pub(crate) store: Store,
    pub(crate) temp: Option<Publisher>,
}

/// `create_switch` after the attributes were decoded (and the backend chosen): brings the
/// backend up with `events` and builds the switch. On any error the backend is left shut down.
pub(crate) fn create<B: Backend>(
    backend: &mut B,
    attrs: &[Attr],
    profile: &Profile,
    events: crate::backend::EventSink,
) -> Result<Created> {
    let req = request(attrs)?;
    let quirks = PortQuirks::parse(profile)?;
    let mut ext = extphy::ext_phy_load(profile)?;
    let macsec_table = macsec::ext_macsec_load(profile)?;
    let l3_config = l3::Config::from_profile(profile)?;
    log::notice(
        sys::SAI_API_SWITCH,
        format_args!("initializing backend {}", backend_name(backend)),
    );
    if let Err(e) = backend.init(profile, events) {
        log::error(
            sys::SAI_API_SWITCH,
            format_args!("backend {} init failed: {}", backend_name(backend), e.status()),
        );
        return Err(e);
    }
    // Lanes to ports, link gates: before the ports are disabled below.
    let attached = extphy::ext_phy_attach(backend, &mut ext).and_then(|()| {
        // Engines a previous process left on go back to bypass.
        macsec::ext_macsec_attach(backend, &mut ext, macsec_table)
            .inspect_err(|_| extphy::ext_phy_detach(backend, &ext))
    });
    if let Err(e) = attached {
        macsec::ext_macsec_detach(backend, &ext);
        backend.shutdown();
        return Err(e);
    }
    let built = build_objects(backend, &req, quirks, &ext);
    let (id, mut store) = match built {
        Ok((id, mut store, true)) => {
            *store.ext_mut() = ext;
            (id, store)
        }
        Ok((_, mut store, false)) => {
            *store.ext_mut() = ext;
            log::error(
                sys::SAI_API_SWITCH,
                format_args!(
                    "a SAI_OPENBCM_PORT_* quirk could not be applied to every port (see above); refusing to create the switch"
                ),
            );
            // C++ order: the objects (host interface netdevs), MACsec, the external PHY, the chip.
            hostif::hostif_netifs_destroy(backend, &store);
            macsec::ext_macsec_detach(backend, store.ext());
            extphy::ext_phy_detach(backend, store.ext());
            backend.shutdown();
            return Err(Error::Failure);
        }
        Err(e) => {
            macsec::ext_macsec_detach(backend, &ext);
            extphy::ext_phy_detach(backend, &ext);
            backend.shutdown();
            return Err(e);
        }
    };
    l3::configure(&mut store, l3_config);
    if let Some(seconds) = req.fdb_aging {
        if backend.l2_age_set(seconds).is_ok() {
            store.update::<Switch, _>(id, |s| s.fdb_aging = seconds)?;
        }
    }
    // Temperature files for fan control. Never from the fake on a host with a switch ASIC: fan
    // control would trust made-up readings.
    let temp = if !backend.is_fake() || !temp::hardware_present() {
        backend
            .temperature_reader()
            .and_then(|reader| Publisher::start(profile, reader))
    } else {
        if !keys::get(profile, keys::TEMP_FILE).is_empty() || !keys::get(profile, keys::TEMP_PROM).is_empty() {
            log::warn(
                sys::SAI_API_SWITCH,
                format_args!("fake backend on hardware: not publishing ASIC temperature files"),
            );
        }
        None
    };
    log::notice(
        sys::SAI_API_SWITCH,
        format_args!(
            "switch created: {} front-panel ports, backend {}",
            front_ports(&store).count(),
            backend_name(backend)
        ),
    );
    Ok(Created { id, store, temp })
}

/// The backend's name as the C++ logs it (`fake` for the in-memory chip).
fn backend_name<B: Backend + ?Sized>(backend: &B) -> &'static str {
    if backend.is_fake() { "fake" } else { backend.name() }
}

/// `remove_switch` (and an API uninitialised with a switch): the publisher first (it removes
/// its files), the external PHY gates and `MACsec` state, the host interface netdevs, then the
/// backend.
pub(crate) fn remove<B: Backend>(backend: &mut B, store: &Store, temp: Option<Publisher>) {
    if let Some(publisher) = temp {
        publisher.stop();
    }
    teardown(backend, store);
    log::notice(sys::SAI_API_SWITCH, format_args!("switch removed"));
}

/// C++ `switch_clear_objects` + detach + `shutdown`.
fn teardown<B: Backend>(backend: &mut B, store: &Store) {
    // Link gates off before the backend goes away; the PHYs keep their MACsec state (the next
    // switch creation puts every engine back into bypass).
    extphy::ext_phy_detach(backend, store.ext());
    macsec::ext_macsec_detach(backend, store.ext());
    hostif::hostif_netifs_destroy(backend, store);
    backend.shutdown();
}

fn front_ports(store: &Store) -> impl Iterator<Item = (PortId, &Port)> {
    store
        .iter::<Port>()
        .filter(|(_, p)| matches!(p.kind, crate::model::PortKind::Front { .. }))
}

/// The objects every switch has (C++ `switch_build_objects`). `false` in the answer: a board
/// quirk was refused for some port (the caller tears down).
#[allow(
    clippy::too_many_lines,
    reason = "the C++ order of the default objects, in one place"
)]
fn build_objects<B: Backend>(
    backend: &mut B,
    req: &CreateRequest,
    quirks: PortQuirks,
    ext: &extphy::External,
) -> Result<(SwitchId, Store, bool)> {
    let mut store = Store::new();
    let cpu_hw = backend.cpu_port();
    let cpu_port = PortId::from_index(u64::from(cpu_hw.0));
    store.insert_at(cpu_port, Port::cpu(cpu_hw))?;
    let mut quirks_ok = true;
    let mut ports = Vec::new();
    for info in backend.ports() {
        let mut port = Port::front(info.hw, info.lanes, info.speed, info.name);
        port.autoneg = info.autoneg;
        // Safety rule: every front-panel port starts admin-down (SAI default), whatever the SDK init
        // or the rc script left behind.
        if let Err(e) = backend.port_admin_set(info.hw, false) {
            log::error(
                sys::SAI_API_SWITCH,
                format_args!("port {}: disable failed ({})", port.name, e.status()),
            );
        }
        // The external PHY (if the board has a table): line side as for admin down.
        extphy::ext_phy_admin_at(ext, info.hw, false);
        let macsec_ifg = macsec::macsec_port_ifg(&store, info.hw);
        quirks_ok &= port::port_quirks_apply(backend, &mut port, &quirks, macsec_ifg);
        if let Some(on) = quirks.autoneg {
            port.autoneg = on;
        }
        if backend.port_link_get(info.hw) == Ok(true) {
            port.oper_up = true;
        }
        let id = PortId::from_index(u64::from(info.hw.0));
        store.insert_at(id, port)?;
        build_port_queues(&mut store, id, info.hw)?;
        ports.push(id);
    }
    // The default virtual router (VRF 0), spanning tree instance and trap group, and the hash
    // objects, named by the read-only switch attribute that answers with each.
    let mut objects = BTreeMap::new();
    objects.insert(
        sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
        l3::default_virtual_router(&mut store)?.raw(),
    );
    let stp = store.insert_bag(Bag {
        object_type: ObjectType::Stp,
        attrs: Kept::new(),
    })?;
    objects.insert(sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, stp);
    objects.insert(
        sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP,
        hostif::default_trap_group(&mut store)?,
    );
    for attr in [sys::SAI_SWITCH_ATTR_LAG_HASH, sys::SAI_SWITCH_ATTR_ECMP_HASH] {
        let hash = store.insert_bag(Bag {
            object_type: ObjectType::Hash,
            attrs: Kept::new(),
        })?;
        objects.insert(attr, hash);
    }
    let default_bridge: BridgeId = store.insert(Bridge { kept: Kept::new() })?;
    let mut bridge_ports: Vec<(BridgePortId, HwPort)> = Vec::new();
    for port in &ports {
        let hw = store.get::<Port>(*port)?.hw;
        let bridge_port = store.insert(BridgePort {
            bridge: default_bridge,
            attach: crate::lag::Attach::Port(*port),
            admin: true, // the SDK forwards on these ports
            learn: crate::l2::FdbLearning::Hw,
            kept: Kept::new(),
        })?;
        bridge_ports.push((bridge_port, hw));
    }
    // VLAN 1 with the SDK's own membership (orchagent removes what it does not want).
    // A VLAN's id is its VID (`crate::l2`).
    let default_vlan = crate::oid::VlanId::from_index(u64::from(Vid::DEFAULT.get()));
    store.insert_at(
        default_vlan,
        Vlan {
            vid: Vid::DEFAULT,
            kept: Kept::new(),
        },
    )?;
    if let Ok(members) = backend.vlan_members(Vid::DEFAULT) {
        for (hw, untagged) in members {
            if let Some((bridge_port, _)) = bridge_ports.iter().find(|(_, port_hw)| *port_hw == hw) {
                store.insert(VlanMember {
                    vlan: default_vlan,
                    bridge_port: *bridge_port,
                    tagging: if untagged {
                        crate::l2::Tagging::Untagged
                    } else {
                        crate::l2::Tagging::Tagged
                    },
                })?;
            }
        }
    }
    let id = SwitchId::from_index(1);
    store.insert_at(
        id,
        Switch {
            src_mac: req.src_mac.unwrap_or(DEFAULT_SRC_MAC),
            cpu_port,
            default_vlan,
            default_bridge,
            objects,
            kept: req.kept.clone(),
            acl: acl::AclState::default(),
            fdb_aging: 0,
            quirks,
        },
    )?;
    Ok((id, store, quirks_ok))
}

/// The queues, priority groups and scheduler groups of a front-panel port. Their ids carry the
/// port and the index (the C++ layout `(hw << 8) | n`): one flat level, a scheduler group per
/// queue with the port as its parent (C++ `qos_build_port`).
fn build_port_queues(store: &mut Store, port: PortId, hw: HwPort) -> Result<()> {
    let index_of = |n: u8| (u64::from(hw.0) << 8) | u64::from(n);
    for queue in 0..QUEUES_PER_PORT {
        let group = ObjectType::SchedulerGroup.id(index_of(queue));
        store.insert_bag_at(
            group,
            Bag {
                object_type: ObjectType::SchedulerGroup,
                attrs: Kept::from([
                    (sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID, Value::Oid(port.raw())),
                    (sys::SAI_SCHEDULER_GROUP_ATTR_LEVEL, Value::U8(0)),
                    (sys::SAI_SCHEDULER_GROUP_ATTR_MAX_CHILDS, Value::U8(1)),
                    (sys::SAI_SCHEDULER_GROUP_ATTR_PARENT_NODE, Value::Oid(port.raw())),
                ]),
            },
        )?;
        store.insert_bag_at(
            ObjectType::Queue.id(index_of(queue)),
            Bag {
                object_type: ObjectType::Queue,
                attrs: Kept::from([
                    (
                        sys::SAI_QUEUE_ATTR_TYPE,
                        Value::Enum(sys::SAI_QUEUE_TYPE_UNICAST.cast_signed()),
                    ),
                    (sys::SAI_QUEUE_ATTR_PORT, Value::Oid(port.raw())),
                    (sys::SAI_QUEUE_ATTR_INDEX, Value::U8(queue)),
                    (sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE, Value::Oid(group)),
                ]),
            },
        )?;
    }
    for group in 0..IPGS_PER_PORT {
        store.insert_bag_at(
            ObjectType::IngressPriorityGroup.id(index_of(group)),
            Bag {
                object_type: ObjectType::IngressPriorityGroup,
                attrs: Kept::from([
                    (sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT, Value::Oid(port.raw())),
                    (sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX, Value::U8(group)),
                ]),
            },
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------- attributes

fn not_supported() -> Error {
    Error::attr(AttrError::NotSupported, 0)
}

/// Capacity and capability attributes orchagent reads at init and CRM polls (C++
/// `get_capacity`): the backend's `chip_limits` / `table_usage`, asked once per attribute.
/// `None`: not one of them.
fn get_capacity<B: Backend>(backend: &B, attr: sys::sai_attr_id_t) -> Option<Result<Value>> {
    // The free entries of the forwarding tables (the L3 area has the same rules).
    if let Some(value) = l3::available(backend, attr) {
        return Some(value);
    }
    let limits = || backend.chip_limits().map_err(|e| e.for_attr(0));
    let usage = || backend.table_usage().map_err(|e| e.for_attr(0));
    Some(match attr {
        sys::SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY => limits().map(|l| Value::U32(l.acl_prio_min)),
        sys::SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY => limits().map(|l| Value::U32(l.acl_prio_max)),
        sys::SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT => u32::try_from(acl::acl_ingress_actions(backend).len())
            .map(Value::U32)
            .map_err(|_| Error::Failure),
        sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS | sys::SAI_SWITCH_ATTR_ACL_STAGE_EGRESS => {
            // The field processor's actions per stage (egress: none unless the backend has
            // egress tables).
            Ok(Value::AclCapability {
                action_list_mandatory: false,
                actions: if attr == sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS {
                    acl::acl_ingress_actions(backend)
                } else {
                    acl::acl_egress_actions(backend)
                },
                non_contiguous_exact_match: false,
                match_type: 0,
            })
        }
        sys::SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS => limits().map(|l| Value::U32(l.ecmp_groups)),
        sys::SAI_SWITCH_ATTR_ECMP_MEMBERS => limits().map(|l| Value::U32(l.ecmp_paths)),
        sys::SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT => limits().map(|l| Value::U32(l.ecmp_members)),
        // An empty (or unresolved) next-hop group holds no SDK group yet.
        sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY => usage().map(|u| Value::U32(u.ecmp_groups_free)),
        sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY => usage().map(|u| Value::U32(u.ecmp_members_free)),
        sys::SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE => limits().map(|l| Value::U64(l.buffer_bytes / 1024)),
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY => usage().map(|u| Value::U32(u.v4_routes_free)),
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY => usage().map(|u| Value::U32(u.v6_routes_free)),
        // A next hop uses its neighbor's egress object.
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY | sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY => {
            usage().map(|u| Value::U32(u.egress_free))
        }
        // A neighbor takes a host entry (IPv6: two) and an egress object.
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY => {
            usage().map(|u| Value::U32(u.hosts_free.min(u.egress_free)))
        }
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY => {
            usage().map(|u| Value::U32((u.hosts_free / 2).min(u.egress_free)))
        }
        sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY => usage().map(|u| Value::U32(u.fdb_free)),
        _ => return None,
    })
}

/// Gets one switch attribute (C++ `get_one`; errors carry index 0).
pub(crate) fn get<B: Backend>(backend: &B, store: &Store, id: SwitchId, attr: sys::sai_attr_id_t) -> Result<Value> {
    if let Some(value) = get_capacity(backend, attr) {
        return value;
    }
    let switch = store.get::<Switch>(id)?;
    let object = |a| switch.objects.get(&a).copied().map(Value::Oid).ok_or(Error::Failure);
    let count = |n: usize| u32::try_from(n).map(Value::U32).map_err(|_| Error::Failure);
    match attr {
        sys::SAI_SWITCH_ATTR_TYPE => return Ok(Value::Enum(sys::SAI_SWITCH_TYPE_NPU.cast_signed())),
        sys::SAI_SWITCH_ATTR_INIT_SWITCH => return Ok(Value::Bool(true)),
        sys::SAI_SWITCH_ATTR_OPER_STATUS => return Ok(Value::Enum(sys::SAI_SWITCH_OPER_STATUS_UP.cast_signed())),
        // == NUMBER_OF_ACTIVE_PORTS
        sys::SAI_SWITCH_ATTR_PORT_NUMBER | sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_SUPPORTED_PORTS => {
            return count(front_ports(store).count());
        }
        sys::SAI_SWITCH_ATTR_PORT_LIST => {
            return Ok(Value::OidList(front_ports(store).map(|(p, _)| p.raw()).collect()));
        }
        sys::SAI_SWITCH_ATTR_CPU_PORT => return Ok(Value::Oid(switch.cpu_port.raw())),
        sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID
        | sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID
        | sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP => return object(attr),
        sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID => return Ok(Value::Oid(switch.default_vlan.raw())),
        sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID => return Ok(Value::Oid(switch.default_bridge.raw())),
        sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS => return Ok(Value::Mac(switch.src_mac)),
        sys::SAI_SWITCH_ATTR_PORT_MAX_MTU => return Ok(Value::U32(port::PORT_MAX_MTU)),
        sys::SAI_SWITCH_ATTR_FDB_AGING_TIME => return Ok(Value::U32(switch.fdb_aging)),
        sys::SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES => return Ok(Value::U8(QUEUES_PER_PORT)),
        sys::SAI_SWITCH_ATTR_NUMBER_OF_UNICAST_QUEUES | sys::SAI_SWITCH_ATTR_NUMBER_OF_QUEUES => {
            return Ok(Value::U32(u32::from(QUEUES_PER_PORT)));
        }
        sys::SAI_SWITCH_ATTR_NUMBER_OF_MULTICAST_QUEUES => return Ok(Value::U32(0)),
        // Asked for by syncd: nothing to identify the chip by.
        sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO => return Ok(Value::I8List(Vec::new())),
        _ => {}
    }
    if temp::is_temp_attr(attr) {
        return temp::switch_temp_get(backend, attr);
    }
    if let Some(value) = qos::switch_qos_get(store, id, attr) {
        return value;
    }
    if let Some(value) = hash::switch_hash_get(backend, store, id, attr) {
        return value;
    }
    if is_notify_attr(attr) {
        return Ok(match switch.kept.get(&attr) {
            Some(Value::Pointer(p)) => Value::Pointer(*p),
            _ => Value::Pointer(Pointer::NULL),
        });
    }
    if let Some((_, def, _)) = stub(attr) {
        return match switch.kept.get(&attr) {
            Some(value) => Ok(value.clone()),
            None => stub_value(sys::SAI_OBJECT_TYPE_SWITCH, attr, *def).ok_or(Error::Failure),
        };
    }
    Err(not_supported())
}

/// Sets one switch attribute (C++ `set_switch_attribute`; errors carry index 0).
pub(crate) fn set<B: Backend>(backend: &mut B, store: &mut Store, id: SwitchId, attr: &Attr) -> Result<()> {
    match (attr.id, &attr.value) {
        (sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)) => {
            let old = store.update::<Switch, _>(id, |s| core::mem::replace(&mut s.src_mac, *mac))?;
            // The router MAC follows (the my-station entry); a refusal puts the old one back.
            l3::l3_switch_mac_changed(backend, store, old, *mac).inspect_err(|_| {
                let _ = store.update::<Switch, _>(id, |s| s.src_mac = old);
            })
        }
        (sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(seconds)) => {
            backend.l2_age_set(*seconds).map_err(|e| e.for_attr(0))?;
            store.update::<Switch, _>(id, |s| s.fdb_aging = *seconds)
        }
        (attr_id, Value::Pointer(p)) if is_notify_attr(attr_id) => {
            store.update::<Switch, _>(id, |s| drop(s.kept.insert(attr_id, Value::Pointer(*p))))
        }
        _ => {
            if let Some(done) = qos::switch_qos_set(backend, store, id, attr) {
                return done; // SWITCH QOS_*_MAP
            }
            if let Some(done) = hash::switch_hash_set(backend, store, id, attr) {
                return done; // *_DEFAULT_HASH_SEED
            }
            if let Some((_, _, create_only)) = stub(attr.id) {
                if *create_only {
                    return Err(Error::attr(AttrError::InvalidValue, 0));
                }
                store.update::<Switch, _>(id, |s| drop(s.kept.insert(attr.id, attr.value.clone())))?;
                log::info(
                    sys::SAI_API_SWITCH,
                    format_args!("switch attr {} stored (not programmed)", attr.id),
                );
                return Ok(());
            }
            log::warn(
                sys::SAI_API_SWITCH,
                format_args!("set switch attr {} not supported", attr.id),
            );
            Err(not_supported())
        }
    }
}

/// The status of a set the rim refused before decoding the value (an attribute the metadata
/// does not let a set carry), as the C++ answers it: a create-only stub `INVALID_ATTR_VALUE`,
/// anything else `ATTR_NOT_SUPPORTED`.
pub(crate) fn refused_set(attr: sys::sai_attr_id_t) -> Error {
    match stub(attr) {
        Some((_, _, true)) => Error::attr(AttrError::InvalidValue, 0),
        _ => not_supported(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_defaults_have_the_metadata_kind() {
        for (attr, def, _) in SWITCH_STUBS {
            let value = stub_value(sys::SAI_OBJECT_TYPE_SWITCH, *attr, *def).unwrap();
            assert_eq!(
                value.value_type(),
                sai_meta::attribute(sys::SAI_OBJECT_TYPE_SWITCH, *attr)
                    .unwrap()
                    .value_type
            );
        }
        for (attr, def) in port::PORT_STUBS {
            let value = stub_value(sys::SAI_OBJECT_TYPE_PORT, *attr, *def).unwrap();
            assert_eq!(
                value.value_type(),
                sai_meta::attribute(sys::SAI_OBJECT_TYPE_PORT, *attr)
                    .unwrap()
                    .value_type
            );
        }
        for attr in NOTIFY_ATTRS {
            assert_eq!(
                sai_meta::attribute(sys::SAI_OBJECT_TYPE_SWITCH, attr)
                    .unwrap()
                    .value_type,
                sai_meta::ValueType::Pointer
            );
        }
        assert_eq!(SWITCH_STUBS.len(), 22);
    }

    #[test]
    fn create_attributes_follow_the_cpp_rules() {
        let init = |on| Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(on));
        assert_eq!(request(&[]).unwrap_err(), Error::MandatoryAttributeMissing);
        assert_eq!(request(&[init(false)]).unwrap_err(), Error::NotSupported);
        let npu = Attr::new(
            sys::SAI_SWITCH_ATTR_TYPE,
            Value::Enum(sys::SAI_SWITCH_TYPE_PHY.cast_signed()),
        );
        assert_eq!(
            request(&[init(true), npu]).unwrap_err(),
            Error::attr(AttrError::InvalidValue, 1)
        );
        let odd = Attr::new(sys::SAI_SWITCH_ATTR_PORT_NUMBER, Value::U32(1));
        assert_eq!(
            request(&[init(true), odd]).unwrap_err(),
            Error::attr(AttrError::NotSupported, 1)
        );
        let ignored = Attr::new(sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO, Value::I8List(vec![1]));
        assert!(request(&[ignored, init(true)]).unwrap().kept.is_empty());
    }
}

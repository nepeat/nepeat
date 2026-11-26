//! Calls into libifcs: one private wrapper per C signature (one unsafe call site each), and the
//! safe methods of [`Ifcs`] named after the IFCS functions, each passing its table entry to its
//! signature's wrapper. Signatures are Rust types: `ifcs_devport_attr_set` and
//! `ifcs_sysport_attr_set` share one (an `ifcs_devport_t` and an `ifcs_handle_t` are both
//! `u32`), so a symbol listed under a wrapper whose type it does not have does not compile.
//!
//! Each wrapper's contract with libifcs: every pointer argument points at memory borrowed for the
//! call (an [`Attrs`] list and the buffers its list members point at, a handle, an output
//! count), sized as the count arguments say; libifcs writes at most the counted elements and
//! keeps no pointer after returning (the IFCS API copies what it stores). The functions are called
//! with the SAI lock held or from the temperature reader (`sai-backend-teralynx`'s locking rules);
//! libifcs serialises them itself (its API lock).
//!
//! Not wrapped yet (G2: their areas are not ported): the route / L2 entry key accessors and
//! entry calls, the IP prefix accessors, the queue / BAM queue / `QoS` map / traffic monitor
//! calls, `ifcs_l2_entry_get_all` (its callback), `ifcs_status_to_string`. Their table entries are
//! resolved at load all the same.
#![allow(unsafe_code)]

use core::ffi::{CStr, c_int};
use ifcs_sys as sys;

use crate::Ifcs;
use crate::attr::Attrs;
use crate::handle::Handle;
use crate::status::Status;

/// The status answered for a table entry that is `None` (never: `Table::resolve` fills every
/// entry or fails): `IFCS_UNINIT`.
const MISSING: Status = Status(sys::IFCS_UNINIT.cast_signed());

/// `(node, u32 object, u32 count, ifcs_attr_t*)`: object attribute sets.
fn obj_attrs(f: sys::ifcs_sysport_attr_set_fn, node: u32, obj: u32, attrs: &mut Attrs) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    // SAFETY: the module's contract; `p` points at `n` attributes of `attrs` (or is NULL / the
    // placeholder of the empty forms), alive for the call.
    Status(unsafe { f(node, obj, n, p) })
}

/// `(node, u32 object, u32 count, ifcs_attr_t*, u32* got)`: object attribute gets.
fn obj_attrs_get(f: sys::ifcs_sysport_attr_get_fn, node: u32, obj: u32, attrs: &mut Attrs) -> (Status, u32) {
    let Some(f) = f else { return (MISSING, 0) };
    let (n, p) = attrs.args();
    let mut got = 0u32;
    // SAFETY: the module's contract; libifcs writes the values of the `n` attributes and `got`.
    let rc = unsafe { f(node, obj, n, p, &raw mut got) };
    (Status(rc), got)
}

/// `(node, u32 count, ifcs_attr_t*)`: node create, node attribute set.
fn node_attrs(f: sys::ifcs_node_attr_set_fn, node: u32, attrs: &mut Attrs) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    // SAFETY: the module's contract.
    Status(unsafe { f(node, n, p) })
}

/// `(node, u32 count, ifcs_attr_t*, u32* got)`: node attribute get.
fn node_attrs_get(f: sys::ifcs_node_attr_get_fn, node: u32, attrs: &mut Attrs) -> (Status, u32) {
    let Some(f) = f else { return (MISSING, 0) };
    let (n, p) = attrs.args();
    let mut got = 0u32;
    // SAFETY: the module's contract.
    let rc = unsafe { f(node, n, p, &raw mut got) };
    (Status(rc), got)
}

/// `(node, ifcs_handle_t* handle, u32 count, ifcs_attr_t*)`: object creates. `handle` goes in
/// as the handle asked for (`IFCS_NULL_HANDLE`: any) and comes back as the one made.
fn create(f: sys::ifcs_mdg_create_fn, node: u32, handle: &mut Handle, attrs: &mut Attrs) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    // SAFETY: the module's contract; libifcs writes the handle it made.
    Status(unsafe { f(node, &raw mut handle.0, n, p) })
}

/// `(node, ifcs_handle_t)`: object deletes.
fn delete(f: sys::ifcs_mdg_delete_fn, node: u32, handle: Handle) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: scalars only.
    Status(unsafe { f(node, handle.0) })
}

/// `(node, ifcs_handle_t, u32 count, ifcs_handle_t* members, u32 count, ifcs_attr_t*)`: member adds.
fn members_add(
    f: sys::ifcs_stp_port_add_fn,
    node: u32,
    obj: Handle,
    members: &mut [Handle],
    attrs: &mut Attrs,
) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    let count = u32::try_from(members.len()).unwrap_or(u32::MAX);
    // SAFETY: the module's contract; `members` holds `count` handles (`Handle` has the layout of
    // `ifcs_handle_t`: a one-field struct over it, checked in attr.rs).
    Status(unsafe { f(node, obj.0, count, members.as_mut_ptr().cast(), n, p) })
}

/// `(node, ifcs_handle_t, u32 count, ifcs_handle_t* members)`: member removes.
fn members_remove(f: sys::ifcs_lag_member_remove_fn, node: u32, obj: Handle, members: &mut [Handle]) -> Status {
    let Some(f) = f else { return MISSING };
    let count = u32::try_from(members.len()).unwrap_or(u32::MAX);
    // SAFETY: the module's contract, as `members_add`.
    Status(unsafe { f(node, obj.0, count, members.as_mut_ptr().cast()) })
}

/// `(node, u32 object, u32 count, int* ids, u64* values, u32* got)`: counter reads. `values` must
/// hold `ids.len()` entries, or more where the C++ asks for "all" (`count` larger than `ids`:
/// see [`Ifcs::ifcs_devport_stats_get`]).
fn stats_get(
    f: sys::ifcs_devport_stats_get_fn,
    node: u32,
    obj: u32,
    count: u32,
    ids: &mut [c_int],
    values: &mut [u64],
) -> (Status, u32) {
    let Some(f) = f else { return (MISSING, 0) };
    let mut got = 0u32;
    // SAFETY: the module's contract; the caller checked that `ids` and `values` hold `count`
    // elements each (the "all" form reads no id and writes `count` - 1 values).
    let rc = unsafe { f(node, obj, count, ids.as_mut_ptr(), values.as_mut_ptr(), &raw mut got) };
    (Status(rc), got)
}

/// `(node, u32 object, u32 count, int* ids)`: counter clears.
fn stats_clear(f: sys::ifcs_devport_stats_clear_fn, node: u32, obj: u32, ids: &mut [c_int]) -> Status {
    let Some(f) = f else { return MISSING };
    let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    // SAFETY: the module's contract; `ids` holds `count` ids.
    Status(unsafe { f(node, obj, count, ids.as_mut_ptr()) })
}

/// `(node)`: callback unregisters.
fn node_only(f: sys::ifcs_l2_event_unregister_notify_fn, node: u32) -> Status {
    let Some(f) = f else { return MISSING };
    // SAFETY: a scalar.
    Status(unsafe { f(node) })
}

/// `(node, u32 count, ifcs_attr_t*, ifcs_usage_t*)`: table usage.
fn usage(f: sys::ifcs_ecmp_usage_get_fn, node: u32, attrs: &mut Attrs, out: &mut sys::ifcs_usage_t) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    // SAFETY: the module's contract; libifcs writes `out`.
    Status(unsafe { f(node, n, p, out) })
}

/// `(node, ifcs_l2_entry_key_t*, u32 count, ifcs_attr_t*, ifcs_usage_t*)` with a `NULL` key: the
/// whole table (`ifcs_l2_entry_usage_get`), the only form the C++ uses.
fn l2_usage(f: sys::ifcs_l2_entry_usage_get_fn, node: u32, attrs: &mut Attrs, out: &mut sys::ifcs_usage_t) -> Status {
    let Some(f) = f else { return MISSING };
    let (n, p) = attrs.args();
    // SAFETY: the module's contract; a NULL key asks for the whole table (tl_switch.cpp).
    Status(unsafe { f(node, core::ptr::null_mut(), n, p, out) })
}

macro_rules! methods {
    ($wrapper:ident $args:tt -> $ret:tt: $($name:ident),+ $(,)?) => {
        $(methods!(@one $wrapper $args $ret $name);)+
    };
    (@one $wrapper:ident ($($arg:ident: $ty:ty),*) ($ret:ty) $name:ident) => {
        impl Ifcs {
            #[doc = concat!("`", stringify!($name), "`.")]
            pub fn $name(&self, $($arg: $ty),*) -> $ret {
                $wrapper(self.t().$name, $($arg),*)
            }
        }
    };
}

methods!(obj_attrs(node: u32, obj: u32, attrs: &mut Attrs) -> (Status):
    ifcs_sysport_attr_set, ifcs_l2vni_attr_set, ifcs_netdev_attr_set, ifcs_intf_attr_set, ifcs_nexthop_attr_set,
    ifcs_hostif_trap_attr_set, ifcs_acl_meter_attr_set, ifcs_ace_attr_set, ifcs_qos_map_attr_set,
    ifcs_qos_map_group_attr_set, ifcs_aqm_curve_group_attr_set, ifcs_devport_attr_set, ifcs_cpu_queue_attr_set);
methods!(obj_attrs_get(node: u32, obj: u32, attrs: &mut Attrs) -> ((Status, u32)):
    ifcs_sysport_attr_get, ifcs_devport_attr_get);
methods!(node_attrs(node: u32, attrs: &mut Attrs) -> (Status): ifcs_node_create, ifcs_node_attr_set);
methods!(node_attrs_get(node: u32, attrs: &mut Attrs) -> ((Status, u32)): ifcs_node_attr_get);
methods!(create(node: u32, handle: &mut Handle, attrs: &mut Attrs) -> (Status):
    ifcs_stp_create, ifcs_mc_dest_create, ifcs_mdg_create, ifcs_l2vni_create, ifcs_netdev_create, ifcs_l3vni_create,
    ifcs_intf_create, ifcs_nexthop_create, ifcs_hostif_trap_create, ifcs_acl_match_profile_create,
    ifcs_acl_action_profile_create, ifcs_acl_table_create, ifcs_acl_create, ifcs_acl_match_create,
    ifcs_acl_action_create, ifcs_acl_meter_create, ifcs_acl_counter_create, ifcs_ace_create, ifcs_ecmp_create,
    ifcs_lag_create, ifcs_extended_field_list_create, ifcs_acl_comparator_create, ifcs_acl_comparator_group_create,
    ifcs_qos_map_create, ifcs_qos_map_group_create, ifcs_aqm_curve_group_create, ifcs_dup_copy_dest_port_create,
    ifcs_collector_create, ifcs_collector_set_create);
methods!(delete(node: u32, handle: Handle) -> (Status):
    ifcs_mc_dest_delete, ifcs_mdg_delete, ifcs_l2vni_delete, ifcs_netdev_delete, ifcs_intf_delete,
    ifcs_nexthop_delete, ifcs_hostif_trap_delete, ifcs_acl_match_profile_delete, ifcs_acl_action_profile_delete,
    ifcs_acl_table_delete, ifcs_acl_delete, ifcs_acl_match_delete, ifcs_acl_action_delete, ifcs_acl_meter_delete,
    ifcs_acl_counter_delete, ifcs_ace_delete, ifcs_ecmp_delete, ifcs_lag_delete, ifcs_extended_field_list_delete,
    ifcs_acl_comparator_delete, ifcs_acl_comparator_group_delete, ifcs_qos_map_delete, ifcs_qos_map_group_delete,
    ifcs_aqm_curve_group_delete, ifcs_dup_copy_dest_port_delete, ifcs_collector_delete, ifcs_collector_set_delete);
methods!(members_add(node: u32, obj: Handle, members: &mut [Handle], attrs: &mut Attrs) -> (Status):
    ifcs_stp_port_add, ifcs_mdg_mc_dest_add, ifcs_l2vni_member_add, ifcs_acl_table_acl_add, ifcs_acl_ace_add,
    ifcs_ecmp_member_add, ifcs_lag_member_add, ifcs_ecmp_member_replace, ifcs_acl_port_add,
    ifcs_collector_set_member_add);
methods!(members_remove(node: u32, obj: Handle, members: &mut [Handle]) -> (Status):
    ifcs_mdg_mc_dest_remove, ifcs_l2vni_member_remove, ifcs_acl_table_acl_remove, ifcs_acl_ace_remove,
    ifcs_ecmp_member_remove, ifcs_stp_port_remove, ifcs_lag_member_remove, ifcs_acl_port_remove,
    ifcs_collector_set_member_remove);
methods!(stats_clear(node: u32, obj: u32, ids: &mut [c_int]) -> (Status):
    ifcs_devport_stats_clear, ifcs_intf_stats_clear, ifcs_acl_counter_stats_clear);
methods!(node_only(node: u32) -> (Status): ifcs_linkscan_event_unregister_notify, ifcs_l2_event_unregister_notify);
methods!(usage(node: u32, attrs: &mut Attrs, out: &mut sys::ifcs_usage_t) -> (Status):
    ifcs_nexthop_usage_get, ifcs_ecmp_usage_get);
methods!(l2_usage(node: u32, attrs: &mut Attrs, out: &mut sys::ifcs_usage_t) -> (Status): ifcs_l2_entry_usage_get);

impl Ifcs {
    /// `ifcs_devport_stats_get` / `ifcs_hostif_trap_stats_get` / `ifcs_acl_counter_stats_get` /
    /// `ifcs_intf_stats_get` / `ifcs_cpu_queue_stats_get` by id list: `values` gets one value per
    /// id. The ids slice and the values slice must be equally long (else nothing is called and
    /// `IFCS_PARAM` is answered).
    fn by_ids(
        f: sys::ifcs_devport_stats_get_fn,
        node: u32,
        obj: u32,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        let Ok(count) = u32::try_from(ids.len()) else {
            return (Status(sys::IFCS_PARAM.cast_signed()), 0);
        };
        if values.len() != ids.len() {
            return (Status(sys::IFCS_PARAM.cast_signed()), 0);
        }
        stats_get(f, node, obj, count, ids, values)
    }

    /// `ifcs_devport_stats_get(node, devport, count, ids, values, &got)`, by id list.
    pub fn ifcs_devport_stats_get(
        &self,
        node: u32,
        devport: u32,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        Self::by_ids(self.t().ifcs_devport_stats_get, node, devport, ids, values)
    }

    /// `ifcs_devport_stats_get(node, devport, IFCS_DEVPORT_STATS_ID_ALL, ids, values, &got)`: the
    /// vendor CLI's form (every counter, by id; `values` gets `IFCS_DEVPORT_STATS_ID_MAX_COUNT`).
    /// `ids` and `values` hold `IFCS_DEVPORT_STATS_ID_ALL` entries, as the C++ arrays.
    pub fn ifcs_devport_stats_get_all(&self, node: u32, devport: u32) -> (Status, u32, Vec<u64>) {
        let all = sys::IFCS_DEVPORT_STATS_ID_ALL;
        let len = usize::try_from(all).unwrap_or(0);
        let mut ids = vec![0 as c_int; len];
        let mut values = vec![0u64; len];
        let (st, got) = stats_get(
            self.t().ifcs_devport_stats_get,
            node,
            devport,
            all,
            &mut ids,
            &mut values,
        );
        (st, got, values)
    }

    /// `ifcs_hostif_trap_stats_get` by id list.
    pub fn ifcs_hostif_trap_stats_get(
        &self,
        node: u32,
        trap: Handle,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        Self::by_ids(self.t().ifcs_hostif_trap_stats_get, node, trap.0, ids, values)
    }

    /// `ifcs_acl_counter_stats_get` by id list.
    pub fn ifcs_acl_counter_stats_get(
        &self,
        node: u32,
        counter: Handle,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        Self::by_ids(self.t().ifcs_acl_counter_stats_get, node, counter.0, ids, values)
    }

    /// `ifcs_intf_stats_get` by id list.
    pub fn ifcs_intf_stats_get(&self, node: u32, intf: Handle, ids: &mut [c_int], values: &mut [u64]) -> (Status, u32) {
        Self::by_ids(self.t().ifcs_intf_stats_get, node, intf.0, ids, values)
    }

    /// `ifcs_cpu_queue_stats_get` by id list.
    pub fn ifcs_cpu_queue_stats_get(
        &self,
        node: u32,
        queue: u32,
        ids: &mut [c_int],
        values: &mut [u64],
    ) -> (Status, u32) {
        Self::by_ids(self.t().ifcs_cpu_queue_stats_get, node, queue, ids, values)
    }

    /// Whether libifcs exports its BAR register accessors (`node_pci_reg_read32` / `_write32`).
    pub fn has_reg_access(&self) -> bool {
        self.0.regs.is_some()
    }

    /// libifcs' `node_pci_reg_read32(node, off, &val)`: `Ok(val)`, `Err(rc)` (`-1`: libifcs does not
    /// export it).
    pub fn reg_read32(&self, node: u32, off: u32) -> Result<u32, c_int> {
        let Some((rd, _)) = self.0.regs else { return Err(-1) };
        let off = i32::try_from(off).map_err(|_| -1)?;
        let mut v = 0u32;
        // SAFETY: the module's contract: `v` is valid for the one write; libifcs reads its BAR at
        // `off` through its own mapping (the vendor CLI's `pci read`).
        let rc = unsafe { rd(node, off, &raw mut v) };
        if rc == 0 { Ok(v) } else { Err(rc) }
    }

    /// libifcs' `node_pci_reg_write32(node, off, val)`: `Err(rc)` on failure (`-1`: not exported).
    pub fn reg_write32(&self, node: u32, off: u32, val: u32) -> Result<(), c_int> {
        let Some((_, wr)) = self.0.regs else { return Err(-1) };
        let off = i32::try_from(off).map_err(|_| -1)?;
        // SAFETY: scalars; libifcs writes its BAR at `off` (the vendor CLI's `pci write`).
        let rc = unsafe { wr(node, off, val) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    /// `ifcs_init(config, log_name, boot, &num_nodes)`.
    pub fn ifcs_init(&self, config: &CStr, log_name: &CStr, boot: i32) -> (Status, u32) {
        let Some(f) = self.t().ifcs_init else {
            return (MISSING, 0);
        };
        let mut nodes = 0u32;
        // SAFETY: the module's contract; two NUL-terminated strings libifcs reads (it copies the
        // config path's contents and the log name), the node count it writes.
        let rc = unsafe { f(config.as_ptr(), log_name.as_ptr(), boot, &raw mut nodes) };
        (Status(rc), nodes)
    }

    /// `ifcs_deinit(type)`.
    pub fn ifcs_deinit(&self, kind: i32) -> Status {
        let Some(f) = self.t().ifcs_deinit else { return MISSING };
        // SAFETY: a scalar.
        Status(unsafe { f(kind) })
    }

    /// `ifcs_attr_t_init(&attr)` (the vendor accessor sequence of `innovium.user`).
    pub fn ifcs_attr_t_init(&self, attr: &mut sys::ifcs_attr_t) -> Status {
        let Some(f) = self.t().ifcs_attr_t_init else {
            return MISSING;
        };
        // SAFETY: the module's contract; one attribute libifcs initialises.
        Status(unsafe { f(attr) })
    }

    /// `ifcs_attr_t_id_set(&attr, id)`.
    pub fn ifcs_attr_t_id_set(&self, attr: &mut sys::ifcs_attr_t, id: u32) -> Status {
        let Some(f) = self.t().ifcs_attr_t_id_set else {
            return MISSING;
        };
        // SAFETY: as `ifcs_attr_t_init`.
        Status(unsafe { f(attr, id) })
    }

    /// `ifcs_attr_t_value_data_set(&attr, value)`.
    pub fn ifcs_attr_t_value_data_set(&self, attr: &mut sys::ifcs_attr_t, value: bool) -> Status {
        let Some(f) = self.t().ifcs_attr_t_value_data_set else {
            return MISSING;
        };
        let v = if value {
            sys::IFCS_BOOL_TRUE
        } else {
            sys::IFCS_BOOL_FALSE
        };
        // SAFETY: as `ifcs_attr_t_init`.
        Status(unsafe { f(attr, v.cast_signed()) })
    }
}

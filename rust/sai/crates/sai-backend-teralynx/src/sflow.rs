//! `sFlow` to psample (`tl_sflow.{h,cpp}`). Ported in G1: `pre_enable` (node
//! `EGRESS_SFLOW_ENABLE`, a boot-time attribute, with `SAI_TERALYNX_EGRESS_SFLOW=on`) and the
//! shutdown's state reset. Generic netlink host interfaces, the SAMPLEPACKET trap and its meter,
//! the ipd ioctls are G2.
#![forbid(unsafe_code)]

use ifcs::{Attrs, sys};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};

/// The `sFlow` area's state (G1: what the shutdown resets).
#[derive(Debug)]
pub struct SflowArea {
    /// The samples' CPU queue, -2 = not read yet (`queue_`).
    pub queue: i32,
    /// Node `EGRESS_SFLOW_ENABLE` as last read, -1 = unknown (`egress_`).
    pub egress: i32,
}

impl Default for SflowArea {
    fn default() -> Self {
        Self { queue: -2, egress: -1 }
    }
}

impl SflowArea {
    /// `SflowArea::pre_enable`.
    pub fn pre_enable(core: &Core) {
        if !core.settings.egress_sflow {
            return;
        }
        let mut e = Attrs::new().data(sys::IFCS_NODE_ATTR_EGRESS_SFLOW_ENABLE, true);
        let rc = core.lib.ifcs_node_attr_set(NODE, &mut e);
        if !rc.is_success() {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!("node EGRESS_SFLOW_ENABLE: {} (egress sampling stays off)", rc.name()),
            );
        }
    }

    /// `SflowArea::shutdown` (no generic netlink host interface or policed queue exists in G1:
    /// no IFCS or ipd call).
    pub const fn shutdown(&mut self) {
        self.queue = -2;
        self.egress = -1;
    }
}

//! Port counters by SAI id (`tl_counters.{h,cpp}`): isai 6.2.0's mapping on this chip (101 ids),
//! one `ifcs_devport_stats_get` for the IFCS counters a request needs, and the diag shell's
//! `counters <devport>`.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::sync::OnceLock;

use ifcs::sys;
use sai_core::error::{Error, Result};
use sai_sys as s;

use crate::core::{Core, NODE};
use crate::hostif;

/// A SAI port counter and the IFCS devport counters summed for it (none: always 0, supported).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatMap {
    /// `SAI_PORT_STAT_*`.
    pub sai: s::sai_stat_id_t,
    /// `IFCS_DEVPORT_STATS_ID_*`.
    pub ifcs: Vec<u32>,
}

/// The map, in the C++ order.
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "the C++ table, one entry per line"
)]
pub fn stat_map() -> &'static [StatMap] {
    static MAP: OnceLock<Vec<StatMap>> = OnceLock::new();
    MAP.get_or_init(|| {
        #[allow(clippy::wildcard_imports, reason = "the C++ D() macro: the IFCS counter ids by name")]
        use sys::*;
        let m = |sai: s::sai_stat_id_t, ifcs: &[u32]| StatMap {
            sai,
            ifcs: ifcs.to_vec(),
        };
        let mut v = vec![
            m(s::SAI_PORT_STAT_IF_IN_OCTETS, &[IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL]),
            m(
                s::SAI_PORT_STAT_IF_IN_UCAST_PKTS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_UNICAST],
            ),
            m(
                s::SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS,
                &[
                    IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST,
                    IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST,
                ],
            ),
            // isai: sysport drop reasons + the ACL drop counter
            m(s::SAI_PORT_STAT_IF_IN_DISCARDS, &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_DROP]),
            m(
                s::SAI_PORT_STAT_IF_IN_ERRORS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_ANY],
            ),
            m(s::SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS, &[]),
            m(
                s::SAI_PORT_STAT_IF_IN_BROADCAST_PKTS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST],
            ),
            m(
                s::SAI_PORT_STAT_IF_IN_MULTICAST_PKTS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST],
            ),
            m(s::SAI_PORT_STAT_IF_OUT_OCTETS, &[IFCS_DEVPORT_STATS_ID_TX_BYTES_OK]),
            m(
                s::SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_UNICAST],
            ),
            m(
                s::SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS,
                &[
                    IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST,
                    IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST,
                ],
            ),
            // isai: the egress drop counter
            m(
                s::SAI_PORT_STAT_IF_OUT_DISCARDS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_DROP],
            ),
            m(
                s::SAI_PORT_STAT_IF_OUT_ERRORS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_ANY],
            ),
            m(
                s::SAI_PORT_STAT_IF_OUT_BROADCAST_PKTS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST],
            ),
            m(
                s::SAI_PORT_STAT_IF_OUT_MULTICAST_PKTS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST],
            ),
            m(s::SAI_PORT_STAT_ETHER_STATS_DROP_EVENTS, &[]),
            m(s::SAI_PORT_STAT_ETHER_STATS_FRAGMENTS, &[]),
            m(s::SAI_PORT_STAT_ETHER_STATS_JABBERS, &[]),
            m(s::SAI_PORT_STAT_ETHER_STATS_COLLISIONS, &[]),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_MULTICAST_PKTS,
                &[
                    IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST,
                    IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST,
                ],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_BROADCAST_PKTS,
                &[
                    IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST,
                    IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST,
                ],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_UNDERSIZE_PKTS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_RUNT],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_OVERSIZE_PKTS,
                &[
                    IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_OVER,
                    IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_OVER,
                ],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_RX_OVERSIZE_PKTS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_OVER],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_TX_OVERSIZE_PKTS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_OVER],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_OCTETS,
                &[IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL, IFCS_DEVPORT_STATS_ID_TX_BYTES_ALL],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_PKTS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_ALL, IFCS_DEVPORT_STATS_ID_TX_FRAMES_ALL],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_CRC_ALIGN_ERRORS,
                &[
                    IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_CRC,
                    IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_CRC,
                ],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_TX_NO_ERRORS,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_OK],
            ),
            m(
                s::SAI_PORT_STAT_ETHER_STATS_RX_NO_ERRORS,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_OK],
            ),
            m(
                s::SAI_PORT_STAT_GREEN_WRED_DROPPED_PACKETS,
                &[IFCS_DEVPORT_STATS_ID_AQM_GREEN_PACKET_DROP_COUNT],
            ),
            m(
                s::SAI_PORT_STAT_YELLOW_WRED_DROPPED_PACKETS,
                &[IFCS_DEVPORT_STATS_ID_AQM_YELLOW_PACKET_DROP_COUNT],
            ),
            m(
                s::SAI_PORT_STAT_RED_WRED_DROPPED_PACKETS,
                &[IFCS_DEVPORT_STATS_ID_AQM_RED_PACKET_DROP_COUNT],
            ),
            m(
                s::SAI_PORT_STAT_WRED_DROPPED_PACKETS,
                &[
                    IFCS_DEVPORT_STATS_ID_AQM_GREEN_PACKET_DROP_COUNT,
                    IFCS_DEVPORT_STATS_ID_AQM_YELLOW_PACKET_DROP_COUNT,
                    IFCS_DEVPORT_STATS_ID_AQM_RED_PACKET_DROP_COUNT,
                ],
            ),
            m(s::SAI_PORT_STAT_PAUSE_RX_PKTS, &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_PAUSE]),
            m(s::SAI_PORT_STAT_PAUSE_TX_PKTS, &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_PAUSE]),
            m(
                s::SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES,
                &[IFCS_DEVPORT_STATS_ID_RX_KRPFEC_CORRECTED_CW_COUNT],
            ),
            m(
                s::SAI_PORT_STAT_IF_IN_FEC_NOT_CORRECTABLE_FRAMES,
                &[IFCS_DEVPORT_STATS_ID_RX_KRPFEC_UNCORRECTED_CW_COUNT],
            ),
            m(
                s::SAI_PORT_STAT_IF_IN_FEC_SYMBOL_ERRORS,
                &[IFCS_DEVPORT_STATS_ID_RX_KRPFEC_CW_SYMBOL_ERR],
            ),
        ];
        // frame size buckets: ETHER_STATS_* = rx + tx, ETHER_IN_* rx, ETHER_OUT_* tx; 64 B includes
        // the runts (LEN_LT_64) for IN / OUT, as isai
        let buckets: [(s::sai_stat_id_t, s::sai_stat_id_t, s::sai_stat_id_t, &[u32]); 10] = [
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_64_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_64_OCTETS,
                &[1],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_65_TO_127_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_65_TO_127_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_65_TO_127_OCTETS,
                &[2],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_128_TO_255_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_128_TO_255_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_128_TO_255_OCTETS,
                &[3],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_256_TO_511_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_256_TO_511_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_256_TO_511_OCTETS,
                &[4],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_512_TO_1023_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_512_TO_1023_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_512_TO_1023_OCTETS,
                &[5],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_1024_TO_1518_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_1024_TO_1518_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_1024_TO_1518_OCTETS,
                &[6],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_1519_TO_2047_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_1519_TO_2047_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_1519_TO_2047_OCTETS,
                &[7],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_2048_TO_4095_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_2048_TO_4095_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_2048_TO_4095_OCTETS,
                &[8],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_4096_TO_9216_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_4096_TO_9216_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_4096_TO_9216_OCTETS,
                &[9, 10],
            ),
            (
                s::SAI_PORT_STAT_ETHER_STATS_PKTS_9217_TO_16383_OCTETS,
                s::SAI_PORT_STAT_ETHER_IN_PKTS_9217_TO_16383_OCTETS,
                s::SAI_PORT_STAT_ETHER_OUT_PKTS_9217_TO_16383_OCTETS,
                &[11],
            ),
        ];
        let (rx, tx) = (
            IFCS_DEVPORT_STATS_ID_RX_FRAMES_LEN_LT_64,
            IFCS_DEVPORT_STATS_ID_TX_FRAMES_LEN_LT_64,
        );
        for (all, inn, out, offs) in buckets {
            let (mut a, mut i, mut o) = (m(all, &[]), m(inn, &[]), m(out, &[]));
            for &off in offs {
                a.ifcs.extend([rx + off, tx + off]);
                i.ifcs.push(rx + off);
                o.ifcs.push(tx + off);
            }
            if offs.first() == Some(&1) {
                i.ifcs.push(rx);
                o.ifcs.push(tx);
            }
            v.extend([a, i, o]);
        }
        for p in 0..8 {
            // PFC frames and pause time per priority
            v.push(m(
                s::SAI_PORT_STAT_PFC_0_RX_PKTS + 2 * p,
                &[IFCS_DEVPORT_STATS_ID_RX_FRAMES_PFC0 + p],
            ));
            v.push(m(
                s::SAI_PORT_STAT_PFC_0_TX_PKTS + 2 * p,
                &[IFCS_DEVPORT_STATS_ID_TX_FRAMES_PFC0 + p],
            ));
            v.push(m(
                s::SAI_PORT_STAT_PFC_0_RX_PAUSE_DURATION + 2 * p,
                &[IFCS_DEVPORT_STATS_ID_RX_TIME_PFC0 + p],
            ));
            v.push(m(
                s::SAI_PORT_STAT_PFC_0_TX_PAUSE_DURATION + 2 * p,
                &[IFCS_DEVPORT_STATS_ID_TX_TIME_PFC0 + p],
            ));
        }
        v
    })
}

fn find(id: s::sai_stat_id_t) -> Option<&'static StatMap> {
    stat_map().iter().find(|m| m.sai == id)
}

/// `port_stat_mapped`: the backend has the counter (a constant 0 counts, as isai's).
pub fn port_stat_mapped(id: s::sai_stat_id_t) -> bool {
    find(id).is_some()
}

/// `port_stat_ids`: every mapped id, in map order.
pub fn port_stat_ids() -> Vec<s::sai_stat_id_t> {
    stat_map().iter().map(|m| m.sai).collect()
}

/// `port_stats_by_id`: one `ifcs_devport_stats_get` of every IFCS counter the ids need (first
/// use order, no repeats), then each id's sum. An unmapped id: `NOT_SUPPORTED`, no call.
pub fn port_stats_by_id(core: &Core, hw: u32, ids: &[s::sai_stat_id_t]) -> Result<Vec<u64>> {
    core.front(hw).ok_or(Error::InvalidPortNumber)?;
    let maps: Vec<&StatMap> = ids
        .iter()
        .map(|&id| find(id).ok_or(Error::NotSupported))
        .collect::<Result<_>>()?;
    let mut want: Vec<u32> = Vec::new();
    for m in &maps {
        for x in &m.ifcs {
            if !want.contains(x) {
                want.push(*x);
            }
        }
    }
    let mut vals = vec![0u64; want.len()];
    if !want.is_empty() {
        let mut req: Vec<i32> = want.iter().map(|&x| i32::try_from(x).unwrap_or(i32::MAX)).collect();
        let (rc, _) = core.lib.ifcs_devport_stats_get(NODE, hw, &mut req, &mut vals);
        if !rc.is_success() {
            return Err(core.fail(rc.reason(), "devport stats get (by id)"));
        }
    }
    Ok(maps
        .iter()
        .map(|m| {
            m.ifcs
                .iter()
                .filter_map(|x| want.iter().position(|w| w == x).and_then(|i| vals.get(i)))
                .fold(0u64, |a, b| a.wrapping_add(*b))
        })
        .collect())
}

/// `counters_report`: the diag shell's `counters <devport>`: every mapped counter (8 per line,
/// `id=value`), then the CPU queues.
pub fn counters_report(core: &Core, hw: i32) -> String {
    let ids = port_stat_ids();
    let values = u32::try_from(hw)
        .map_err(|_| Error::InvalidPortNumber)
        .and_then(|hw| port_stats_by_id(core, hw, &ids));
    let v = match values {
        Ok(v) => v,
        Err(e) => return format!("devport {hw}: counters unreadable ({})\n", e.status()),
    };
    let mut out = format!(
        "devport {hw}: {} SAI port counters (SAI_PORT_STAT_* id = value)\n",
        ids.len()
    );
    for (i, (id, val)) in ids.iter().zip(&v).enumerate() {
        let sep = if i % 8 == 7 || i + 1 == ids.len() { "\n" } else { " " };
        let _ = write!(out, "{}={val}{sep}", id.cast_signed());
    }
    for q in 0..8 {
        if let Ok((pk, by, dr)) = hostif::cpu_queue_stats(core, q) {
            let _ = writeln!(out, "cpu queue {q}: tx {pk} bytes {by} drops {dr}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_is_isais_101_ids() {
        assert_eq!(stat_map().len(), 101);
        let ids = port_stat_ids();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "an id mapped twice");
        assert!(port_stat_mapped(s::SAI_PORT_STAT_IF_OUT_OCTETS));
        assert_eq!(
            find(s::SAI_PORT_STAT_IF_OUT_OCTETS).unwrap().ifcs,
            vec![sys::IFCS_DEVPORT_STATS_ID_TX_BYTES_OK]
        );
        // 64 B: rx + tx for STATS, plus the runts for IN / OUT
        let r = sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_LEN_LT_64;
        let t = sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_LEN_LT_64;
        assert_eq!(
            find(s::SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS).unwrap().ifcs,
            vec![r + 1, t + 1]
        );
        assert_eq!(
            find(s::SAI_PORT_STAT_ETHER_IN_PKTS_64_OCTETS).unwrap().ifcs,
            vec![r + 1, r]
        );
        assert_eq!(
            find(s::SAI_PORT_STAT_ETHER_OUT_PKTS_4096_TO_9216_OCTETS).unwrap().ifcs,
            vec![t + 9, t + 10]
        );
        assert_eq!(
            find(s::SAI_PORT_STAT_PFC_3_TX_PKTS).unwrap().ifcs,
            vec![sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_PFC0 + 3]
        );
        assert_eq!(
            find(s::SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS).unwrap().ifcs,
            Vec::<u32>::new()
        );
        assert!(!port_stat_mapped(s::SAI_PORT_STAT_IF_IN_VLAN_DISCARDS));
    }
}

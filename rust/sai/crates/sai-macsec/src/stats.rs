//! Statistics: the SAI ids and the result names a `stats-*` sequence reports them under
//! (lower case, without the `SAI_MACSEC_SA_STAT_` / `SAI_MACSEC_FLOW_STAT_` prefix).
use sai_sys as sys;
use std::collections::BTreeMap;

/// Counter values by result name.
pub(crate) type Counters = BTreeMap<String, u64>;

/// How the PHY's counters behave (the table's `counters` line).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CounterMode {
    /// Reading a counter clears it: the model accumulates what it reads.
    #[default]
    ClearOnRead,
    /// The counter keeps counting: the model reports the last value read.
    Absolute,
}

/// SA statistics and their result names (`kSaStats`).
pub const SA_STATS: &[(sys::sai_stat_id_t, &str)] = &[
    (sys::SAI_MACSEC_SA_STAT_OCTETS_ENCRYPTED, "octets_encrypted"),
    (sys::SAI_MACSEC_SA_STAT_OCTETS_PROTECTED, "octets_protected"),
    (sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED, "out_pkts_encrypted"),
    (sys::SAI_MACSEC_SA_STAT_OUT_PKTS_PROTECTED, "out_pkts_protected"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_UNCHECKED, "in_pkts_unchecked"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_DELAYED, "in_pkts_delayed"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_LATE, "in_pkts_late"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_INVALID, "in_pkts_invalid"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_NOT_VALID, "in_pkts_not_valid"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_NOT_USING_SA, "in_pkts_not_using_sa"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_UNUSED_SA, "in_pkts_unused_sa"),
    (sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK, "in_pkts_ok"),
    (sys::SAI_MACSEC_SA_STAT_IN_OCTETS_DECRYPTED, "in_octets_decrypted"),
    (sys::SAI_MACSEC_SA_STAT_IN_OCTETS_VALIDATED, "in_octets_validated"),
];

/// Flow statistics and their result names (`kFlowStats`).
pub const FLOW_STATS: &[(sys::sai_stat_id_t, &str)] = &[
    (sys::SAI_MACSEC_FLOW_STAT_OTHER_ERR, "other_err"),
    (sys::SAI_MACSEC_FLOW_STAT_OCTETS_UNCONTROLLED, "octets_uncontrolled"),
    (sys::SAI_MACSEC_FLOW_STAT_OCTETS_CONTROLLED, "octets_controlled"),
    (sys::SAI_MACSEC_FLOW_STAT_OUT_OCTETS_COMMON, "out_octets_common"),
    (
        sys::SAI_MACSEC_FLOW_STAT_UCAST_PKTS_UNCONTROLLED,
        "ucast_pkts_uncontrolled",
    ),
    (sys::SAI_MACSEC_FLOW_STAT_UCAST_PKTS_CONTROLLED, "ucast_pkts_controlled"),
    (
        sys::SAI_MACSEC_FLOW_STAT_MULTICAST_PKTS_UNCONTROLLED,
        "multicast_pkts_uncontrolled",
    ),
    (
        sys::SAI_MACSEC_FLOW_STAT_MULTICAST_PKTS_CONTROLLED,
        "multicast_pkts_controlled",
    ),
    (
        sys::SAI_MACSEC_FLOW_STAT_BROADCAST_PKTS_UNCONTROLLED,
        "broadcast_pkts_uncontrolled",
    ),
    (
        sys::SAI_MACSEC_FLOW_STAT_BROADCAST_PKTS_CONTROLLED,
        "broadcast_pkts_controlled",
    ),
    (sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS, "control_pkts"),
    (sys::SAI_MACSEC_FLOW_STAT_PKTS_UNTAGGED, "pkts_untagged"),
    (
        sys::SAI_MACSEC_FLOW_STAT_IN_TAGGED_CONTROL_PKTS,
        "in_tagged_control_pkts",
    ),
    (sys::SAI_MACSEC_FLOW_STAT_OUT_PKTS_TOO_LONG, "out_pkts_too_long"),
    (sys::SAI_MACSEC_FLOW_STAT_IN_PKTS_NO_TAG, "in_pkts_no_tag"),
    (sys::SAI_MACSEC_FLOW_STAT_IN_PKTS_BAD_TAG, "in_pkts_bad_tag"),
    (sys::SAI_MACSEC_FLOW_STAT_IN_PKTS_NO_SCI, "in_pkts_no_sci"),
    (sys::SAI_MACSEC_FLOW_STAT_IN_PKTS_UNKNOWN_SCI, "in_pkts_unknown_sci"),
    (sys::SAI_MACSEC_FLOW_STAT_IN_PKTS_OVERRUN, "in_pkts_overrun"),
];

/// The values of `ids` out of `acc`: 0 for a statistic nothing has reported (also for an id
/// that is not a statistic of the object at all, as in the C++ `stats_fill`).
pub(crate) fn fill(table: &[(sys::sai_stat_id_t, &str)], acc: &Counters, ids: &[sys::sai_stat_id_t]) -> Vec<u64> {
    ids.iter()
        .map(|id| {
            table
                .iter()
                .find(|(stat, _)| stat == id)
                .and_then(|(_, name)| acc.get(*name))
                .copied()
                .unwrap_or(0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_sai_names_in_lower_case() {
        // spot checks against the header's enumerator names
        assert!(SA_STATS.contains(&(sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK, "in_pkts_ok")));
        assert!(FLOW_STATS.contains(&(sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS, "control_pkts")));
        assert_eq!((SA_STATS.len(), FLOW_STATS.len()), (14, 19));
        for table in [SA_STATS, FLOW_STATS] {
            let mut ids: Vec<_> = table.iter().map(|(id, _)| *id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), table.len());
        }
    }

    #[test]
    fn fill_reads_zero_for_what_was_never_reported() {
        let mut acc = Counters::new();
        acc.insert("in_pkts_ok".into(), 7);
        acc.insert("x_unknown".into(), 9);
        assert_eq!(
            fill(
                SA_STATS,
                &acc,
                &[
                    sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK,
                    sys::SAI_MACSEC_SA_STAT_OCTETS_ENCRYPTED,
                    9999
                ]
            ),
            [7, 0, 0]
        );
    }
}

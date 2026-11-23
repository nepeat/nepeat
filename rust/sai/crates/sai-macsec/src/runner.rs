//! The effect side: running a board sequence on the MACsec PHY of a port.
//!
//! The model decides *which* sequence runs for an object change and with which parameter
//! bytes; what a sequence does on the bus is board data (the external MACsec table,
//! `ext_macsec.h`) and is not known here.
use crate::ids::{An, Cipher, Dir, PortKey};
use crate::secret::{Params, Results};
use sai_core::Error;

/// A sequence that did not complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeqError {
    /// The status the SAI call fails with (an MDIO error's status, `Failure` for a compare
    /// or poll that did not match, `InvalidParameter` for a parameter the sequence needs
    /// and did not get).
    pub status: Error,
    /// Why, for the log and the diag status text ("line 11: E phy 0x10 1.a100 read ...").
    /// Must not contain parameter bytes: it is logged and shown.
    pub why: String,
}

/// What the model needs from the board's sequence table and the bus under it.
///
/// Contract for implementations:
/// - Sequence names are the ones [`seq`] builds (documented at the top of `ext_macsec.h`).
/// - `run` checks, before the first bus access, that every parameter byte the sequence
///   names is there (`InvalidParameter`, nothing accessed): a sequence is never started
///   with a key that is too short.
/// - `run` serialises with whatever else uses the PHY's bus (the C++ runs under the external
///   PHY table's sequence lock, so a sequence never interleaves with a link poll).
/// - Parameter bytes are never logged or put into [`SeqError::why`].
/// - A failed `run` may have executed a prefix of the sequence.
pub trait SeqRunner {
    /// The names of all sequences the table has.
    fn sequence_names(&self) -> Vec<String>;

    /// Whether the table has sequence `name`.
    fn has(&self, name: &str) -> bool {
        self.sequence_names().iter().any(|n| n == name)
    }

    /// The result names sequence `name` reads (its `X ... <result>[<word>]` operations);
    /// empty for a sequence the table does not have. Used for the statistics capability.
    fn result_names(&self, name: &str) -> Vec<String>;

    /// Runs sequence `name` on the PHY of `port`. Parameters by the names of
    /// [`crate::secret::param`] with the byte orders documented there.
    fn run(&mut self, port: PortKey, name: &str, params: &Params) -> Result<Results, SeqError>;

    /// The average inter-frame gap of the ASIC MAC of `port` changed because a MACSEC_PORT
    /// was created or removed: `macsec_bits` is the table's `ifg-bits` while the port has a
    /// MACSEC_PORT object, and 0 when it has none (the implementation then restores the
    /// port's own gap: its board quirk or the Ethernet 96 bit times; `port_ifg_apply`).
    /// Must not re-apply anything else of the port (a link flap on a live port).
    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError>;
}

/// The names of the sequences the model runs, exactly as `ext_macsec.h` documents them.
pub mod seq {
    use super::{An, Cipher, Dir};

    /// `enable.<dir>`: MACSEC_PORT created; the engine out of bypass, control frames in clear, the rest dropped.
    pub fn enable(dir: Dir) -> String {
        format!("enable.{}", dir.name())
    }
    /// `disable.<dir>`: MACSEC_PORT removed; the engine into bypass.
    pub fn disable(dir: Dir) -> String {
        format!("disable.{}", dir.name())
    }
    /// `reset.<dir>` (optional): empties the engine's SA tables, before `enable`.
    pub fn reset(dir: Dir) -> String {
        format!("reset.{}", dir.name())
    }
    /// `probe.<dir>` (optional): result `bypass` != 0 when the engine is in bypass.
    pub fn probe(dir: Dir) -> String {
        format!("probe.{}", dir.name())
    }
    /// `gate-open.<dir>` (optional): data frames pass through the SC.
    pub fn gate_open(dir: Dir) -> String {
        format!("gate-open.{}", dir.name())
    }
    /// `gate-close.<dir>` (optional): data frames are dropped.
    pub fn gate_close(dir: Dir) -> String {
        format!("gate-close.{}", dir.name())
    }
    /// `sa-add[-first].<dir>.<an>.<cipher>[.replay]`, the candidates in the order they are
    /// looked for; the last one is run even when the table lacks it (and then fails).
    pub fn sa_add_candidates(dir: Dir, an: An, cipher: Cipher, first: bool, replay: bool) -> Vec<String> {
        let base = format!(".{}.{}.{}", dir.name(), an.get(), cipher.name());
        let mut names = Vec::with_capacity(4);
        if first && replay {
            names.push(format!("sa-add-first{base}.replay"));
        }
        if first {
            names.push(format!("sa-add-first{base}"));
        }
        if replay {
            names.push(format!("sa-add{base}.replay"));
        }
        names.push(format!("sa-add{base}"));
        names
    }
    /// `sa-del.<dir>.<an>`
    pub fn sa_del(dir: Dir, an: An) -> String {
        format!("sa-del.{}.{}", dir.name(), an.get())
    }
    /// `sa-del-last.<dir>.<an>`: the SC's last installed SA (default: `sa-del`).
    pub fn sa_del_last(dir: Dir, an: An) -> String {
        format!("sa-del-last.{}.{}", dir.name(), an.get())
    }
    /// `pn.<dir>.<an>` (optional): result `pn` = next (egress) / lowest acceptable (ingress) PN.
    pub fn pn(dir: Dir, an: An) -> String {
        format!("pn.{}.{}", dir.name(), an.get())
    }
    /// `stats-sa.<dir>.<an>` (optional).
    pub fn stats_sa(dir: Dir, an: An) -> String {
        format!("stats-sa.{}.{}", dir.name(), an.get())
    }
    /// `stats-flow.<dir>` (optional).
    pub fn stats_flow(dir: Dir) -> String {
        format!("stats-flow.{}", dir.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_names_are_the_documented_ones() {
        let an = An::new(2).unwrap();
        assert_eq!(seq::enable(Dir::Egress), "enable.egress");
        assert_eq!(seq::disable(Dir::Ingress), "disable.ingress");
        assert_eq!(seq::reset(Dir::Egress), "reset.egress");
        assert_eq!(seq::probe(Dir::Ingress), "probe.ingress");
        assert_eq!(seq::gate_open(Dir::Egress), "gate-open.egress");
        assert_eq!(seq::gate_close(Dir::Ingress), "gate-close.ingress");
        assert_eq!(seq::sa_del(Dir::Egress, an), "sa-del.egress.2");
        assert_eq!(seq::sa_del_last(Dir::Ingress, an), "sa-del-last.ingress.2");
        assert_eq!(seq::pn(Dir::Egress, an), "pn.egress.2");
        assert_eq!(seq::stats_sa(Dir::Ingress, an), "stats-sa.ingress.2");
        assert_eq!(seq::stats_flow(Dir::Egress), "stats-flow.egress");
        assert_eq!(
            seq::sa_add_candidates(Dir::Ingress, an, Cipher::GcmAesXpn256, true, true),
            [
                "sa-add-first.ingress.2.gcm-aes-xpn-256.replay",
                "sa-add-first.ingress.2.gcm-aes-xpn-256",
                "sa-add.ingress.2.gcm-aes-xpn-256.replay",
                "sa-add.ingress.2.gcm-aes-xpn-256",
            ]
        );
        assert_eq!(
            seq::sa_add_candidates(Dir::Egress, an, Cipher::GcmAes128, false, false),
            ["sa-add.egress.2.gcm-aes-128"]
        );
    }
}

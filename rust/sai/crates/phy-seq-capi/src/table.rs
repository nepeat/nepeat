//! A parsed table in the form its C accessors hand out: the operations of every sequence as
//! the header's `phy_seq_op`, ready to be pointed at. No unsafe.

use phy_seq::{CounterMode, ExtPhyTable, Format, MacsecTable, Op, Section};

use crate::flat::to_flat;
use crate::sys;

/// One sequence: its name, its operations, and the result names they refer to.
pub(crate) struct Seq {
    pub(crate) name: String,
    pub(crate) ops: Vec<sys::phy_seq_op>,
    pub(crate) names: Vec<u8>,
}

impl Seq {
    fn new(name: &str, ops: &[Op]) -> Self {
        let mut names = Vec::new();
        let ops = ops.iter().map(|op| to_flat(op, &mut names)).collect();
        Self {
            name: name.to_owned(),
            ops,
            names,
        }
    }
}

/// What a `phy_seq_table` pointer points to. Immutable once built.
pub(crate) struct Table {
    pub(crate) info: sys::phy_seq_info,
    pub(crate) ports: Vec<sys::phy_seq_port>,
    pub(crate) seqs: Vec<Seq>,
}

fn count(n: usize) -> u32 {
    // a table of more than 4 G ports or sequences does not fit in memory first
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Table {
    /// Parses `text` as a table of `format`.
    pub(crate) fn parse(format: Format, text: &[u8]) -> Result<Self, phy_seq::ParseError> {
        match format {
            Format::ExtMacsec => MacsecTable::parse(text).map(|t| Self::macsec(&t)),
            Format::ExtPhy => ExtPhyTable::parse(text).map(|t| Self::ext_phy(&t)),
        }
    }

    fn macsec(t: &MacsecTable) -> Self {
        let ports = t
            .ports()
            .map(|(lane, phy)| sys::phy_seq_port {
                lane,
                phy: u32::from(phy),
                line: 0,
            })
            .collect();
        let seqs: Vec<Seq> = t.sequences().map(|(name, seq)| Seq::new(name, seq.ops())).collect();
        let info = sys::phy_seq_info {
            format: sys::PHY_SEQ_FORMAT_EXT_MACSEC,
            ports: count(t.ports().len()),
            sequences: count(seqs.len()),
            clear_on_read: u32::from(t.counters() == CounterMode::ClearOnRead),
            ifg_bits: u32::from(t.ifg_bits()),
            poll_ms: 0,
            up_polls: 0,
            holdoff_ms: 0,
        };
        Self { info, ports, seqs }
    }

    fn ext_phy(t: &ExtPhyTable) -> Self {
        let ports = t
            .ports()
            .iter()
            .map(|p| sys::phy_seq_port {
                lane: p.lane(),
                phy: 0,
                line: p.line(),
            })
            .collect();
        let seqs: Vec<Seq> = t
            .ports()
            .iter()
            .flat_map(|p| Section::ALL.map(|s| Seq::new(s.keyword(), p.ops(s))))
            .collect();
        let info = sys::phy_seq_info {
            format: sys::PHY_SEQ_FORMAT_EXT_PHY,
            ports: count(t.ports().len()),
            sequences: count(seqs.len()),
            clear_on_read: 0,
            ifg_bits: 0,
            poll_ms: t.poll_ms(),
            up_polls: t.up_polls(),
            holdoff_ms: t.holdoff_ms(),
        };
        Self { info, ports, seqs }
    }
}

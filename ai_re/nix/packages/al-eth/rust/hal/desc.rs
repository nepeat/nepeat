// SPDX-License-Identifier: GPL-2.0

//! Descriptor encoding and decoding: Tx packets (al_eth_tx_pkt_prepare),
//! Rx buffers (al_eth_rx_buffer_add) and Rx completions (al_eth_pkt_rx).
//! Descriptors are four little-endian words: Tx data {len_ctrl, meta_ctrl,
//! buf_ptr lo, hi}, Tx meta {len_ctrl, meta_ctrl, meta1, meta2}, Rx
//! {len_ctrl, buf2_ptr_lo, buf1_ptr lo, hi}, Rx completion {ctrl_meta, len,
//! word2, word3}.

use crate::hal::io::{CDescMem, DescMem, RegIo};
use crate::hal::regs::k;
use crate::hal::udma::Queue;
use crate::hal::{Error, Result};

/// Most buffers in one packet (AL_ETH_PKT_MAX_BUFS).
pub const MAX_BUFS: usize = k::AL_ETH_PKT_MAX_BUFS as usize;

/// struct al_eth_meta_data, the fields al_eth_main.c sets (words_valid = 4:
/// L3/L4 header geometry and MSS). The other fields are zero in the driver.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TxMeta {
    pub l3_header_len: u8,
    pub l3_header_offset: u8,
    /// in 32-bit words
    pub l4_header_len: u8,
    /// 14 bits
    pub mss_val: u16,
}

/// What al_eth_tx_pkt_prepare reads from struct al_eth_pkt on our path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TxPkt<'a> {
    pub flags: u32,
    pub l3_proto_idx: u32,
    pub l4_proto_idx: u32,
    /// 2 bits
    pub source_vlan_count: u8,
    pub meta: Option<TxMeta>,
    /// (bus address, length) of each buffer: head first, then the frags
    pub bufs: &'a [(u64, u32)],
}

/// al_eth_tx_pkt_prepare: writes the packet's descriptors (a meta
/// descriptor when there is meta data, then one per buffer) and returns how
/// many; 0 when there is no room (as the C). Rings no doorbell.
pub fn tx_prepare<M: DescMem>(q: &mut Queue, mem: &mut M, pkt: &TxPkt<'_>) -> Result<u32> {
    if pkt.bufs.is_empty() || pkt.bufs.len() > MAX_BUFS || mem.len() != q.size() as usize {
        return Err(Error::Invalid);
    }
    // the C asserts: no L4 partial checksum without L4 checksum, no TSO with a timestamp
    if pkt.flags & (k::AL_ETH_TX_FLAGS_L4_CSUM | k::AL_ETH_TX_FLAGS_L4_PARTIAL_CSUM) == k::AL_ETH_TX_FLAGS_L4_PARTIAL_CSUM
        || pkt.flags & (k::AL_ETH_TX_FLAGS_TSO | k::AL_ETH_TX_FLAGS_TS) == (k::AL_ETH_TX_FLAGS_TSO | k::AL_ETH_TX_FLAGS_TS)
    {
        return Err(Error::Invalid);
    }
    let mut flags = k::AL_M2S_DESC_FIRST | k::AL_M2S_DESC_CONCAT | (pkt.flags & k::AL_ETH_TX_FLAGS_INT);
    let tx_descs = pkt.bufs.len() as u32 + pkt.meta.is_some() as u32;
    if q.available() < tx_descs {
        return Ok(0);
    }
    if let Some(m) = pkt.meta {
        let mut w0 = (flags | k::AL_M2S_DESC_META_DATA) & !k::AL_M2S_DESC_CONCAT;
        flags &= !(k::AL_M2S_DESC_FIRST | k::AL_ETH_TX_FLAGS_INT);
        let (idx, ring_id) = q.desc_take();
        w0 |= ring_id << k::AL_M2S_DESC_RING_ID_SHIFT;
        w0 |= 4 << 12; // words_valid: word 2 only
        let mss = (m.mss_val & 0x3fff) as u32;
        let mut w2 = m.l3_header_len as u32 & k::AL_ETH_TX_META_L3_LEN_MASK;
        w2 |= (m.l3_header_offset as u32 & k::AL_ETH_TX_META_L3_OFF_MASK) << k::AL_ETH_TX_META_L3_OFF_SHIFT;
        w2 |= (m.l4_header_len as u32 & 0x3f) << 16;
        if pkt.flags & k::AL_ETH_TX_FLAGS_TS != 0 {
            // ts_index is 0 in the driver
        } else {
            w0 |= ((mss & 0x3c00) >> 10) << k::AL_ETH_TX_META_MSS_MSB_TS_VAL_SHIFT;
        }
        w2 |= (mss & 0x03ff) << k::AL_ETH_TX_META_MSS_LSB_VAL_SHIFT;
        // outer L3 offset/length (tunnels): 0 in the driver, so words 0 and 3 get nothing
        mem.write(idx, [w0, 0, w2, 0]);
    }
    let mut meta_ctrl = pkt.flags & k::AL_ETH_TX_PKT_META_FLAGS;
    meta_ctrl |= pkt.l3_proto_idx;
    meta_ctrl |= pkt.l4_proto_idx << k::AL_ETH_TX_L4_PROTO_IDX_SHIFT;
    meta_ctrl |= ((pkt.source_vlan_count & 3) as u32) << k::AL_ETH_TX_SRC_VLAN_CNT_SHIFT;
    // vlan_mod_*, tunnel_mode, outer_l3_proto_idx and tgtid are 0 in the driver
    flags |= pkt.flags & k::AL_ETH_TX_PKT_UDMA_FLAGS;
    let last = pkt.bufs.len() - 1;
    for (i, &(addr, len)) in pkt.bufs.iter().enumerate() {
        let mut fl = flags;
        let (idx, ring_id) = q.desc_take();
        fl |= ring_id << k::AL_M2S_DESC_RING_ID_SHIFT;
        if i == last {
            fl |= k::AL_M2S_DESC_LAST;
        }
        flags &= k::AL_ETH_TX_FLAGS_NO_SNOOP;
        flags |= k::AL_M2S_DESC_CONCAT;
        fl |= len & k::AL_M2S_DESC_LEN_MASK;
        // meta_ctrl only in the first data descriptor: the C leaves the word
        // of the others as it was, and so do we
        let w1 = if i == 0 { meta_ctrl } else { mem.read(idx)[1] };
        mem.write(idx, [fl, w1, addr as u32, (addr >> 32) as u32]);
    }
    Ok(tx_descs)
}

/// al_eth_tx_dma_action: hand `descs` prepared descriptors to the engine.
pub fn tx_action<R: RegIo, M: DescMem>(io: &R, q: &Queue, mem: &M, descs: u32) -> Result {
    q.action_add(io, mem, descs)
}

/// al_eth_comp_tx_get: descriptors the engine finished since the last call.
pub fn tx_completed<R: RegIo>(io: &R, q: &mut Queue) -> u32 {
    let n = q.cdesc_get_all(io);
    if n != 0 {
        q.cdesc_ack(n);
    }
    n
}

/// al_eth_rx_buffer_add with flags = AL_ETH_RX_FLAGS_INT and no header
/// buffer: one Rx descriptor for the buffer at `addr`.
pub fn rx_buffer_add<M: DescMem>(q: &mut Queue, mem: &mut M, addr: u64, len: u32, flags: u32) -> Result {
    if mem.len() != q.size() as usize || flags & k::AL_S2M_DESC_DUAL_BUF != 0 {
        return Err(Error::Invalid);
    }
    let tgtid = ((flags & k::AL_ETH_RX_FLAGS_TGTID_MASK) as u64) << k::AL_UDMA_DESC_TGTID_SHIFT;
    if q.available() < 1 {
        return Err(Error::NoSpace);
    }
    let (idx, ring_id) = q.desc_take();
    let mut fl = flags & !k::AL_ETH_RX_FLAGS_TGTID_MASK;
    fl |= ring_id << k::AL_S2M_DESC_RING_ID_SHIFT;
    fl |= len & k::AL_S2M_DESC_LEN_MASK;
    let ptr = addr | tgtid;
    // word 1 (second buffer) is not written without a header buffer
    let w1 = mem.read(idx)[1];
    mem.write(idx, [fl, w1, ptr as u32, (ptr >> 32) as u32]);
    Ok(())
}

/// al_eth_rx_buffer_action: hand `descs` Rx descriptors to the engine.
pub fn rx_action<R: RegIo, M: DescMem>(io: &R, q: &Queue, mem: &M, descs: u32) -> Result {
    q.action_add(io, mem, descs)
}

/// What al_eth_pkt_rx reports of a received packet.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RxPkt {
    pub descs: u32,
    /// length of the data in each buffer
    pub lens: [u32; MAX_BUFS],
    /// ctrl_meta of the last descriptor (error, checksum, protocol bits)
    pub flags: u32,
    pub l3_proto_idx: u32,
    pub l4_proto_idx: u32,
    pub rxhash: u16,
    pub l3_offset: u16,
    pub rx_header_len: u32,
}

/// al_eth_pkt_rx: the next complete packet of the completion ring (acked),
/// or None.
pub fn rx_packet<C: CDescMem>(q: &mut Queue, cmem: &C) -> Result<Option<RxPkt>> {
    let (first, n) = match q.cdesc_packet_get(cmem)? {
        None => return Ok(None),
        Some(x) => x,
    };
    if n as usize > MAX_BUFS {
        return Err(Error::Hardware);
    }
    // the ring-id words of these descriptors were read by cdesc_packet_get
    cmem.dma_rmb();
    let size = q.size() as usize;
    let mut p = RxPkt { descs: n, ..Default::default() };
    let mut d = [0u32; 4];
    for i in 0..n as usize {
        d = cmem.read((first + i) % size);
        if i == 0 && d[2] & k::AL_UDMA_CDESC_BUF2_USED != 0 {
            p.rx_header_len = (d[2] & k::AL_S2M_DESC_LEN2_MASK) >> k::AL_S2M_DESC_LEN2_SHIFT;
        }
        p.lens[i] = if d[0] & k::AL_UDMA_CDESC_BUF1_USED != 0 && d[0] & k::AL_UDMA_CDESC_DDP == 0 {
            d[1] & k::AL_S2M_DESC_LEN_MASK
        } else {
            0
        };
    }
    p.flags = d[0];
    p.l3_proto_idx = p.flags & k::AL_ETH_RX_L3_PROTO_IDX_MASK;
    p.l4_proto_idx = (p.flags >> k::AL_ETH_RX_L4_PROTO_IDX_SHIFT) & k::AL_ETH_RX_L4_PROTO_IDX_MASK;
    p.rxhash = ((d[1] & k::AL_ETH_RX_HASH_MASK) >> k::AL_ETH_RX_HASH_SHIFT) as u16;
    p.l3_offset = ((d[2] & k::AL_ETH_RX_L3_OFFSET_MASK) >> k::AL_ETH_RX_L3_OFFSET_SHIFT) as u16;
    q.cdesc_ack(n);
    Ok(Some(p))
}

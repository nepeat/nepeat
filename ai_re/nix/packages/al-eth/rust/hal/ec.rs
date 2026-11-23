// SPDX-License-Identifier: GPL-2.0

//! The Ethernet controller (EC window): parser, forwarding tables, MAC
//! address filter, hash, flow control thresholds (al_hal_eth_main.c).

use crate::hal::io::{field_set, masked, RegIo};
use crate::hal::regs::{k, off, tables};
use crate::hal::{Error, Result};

/// struct al_eth_fwd_mac_table_entry
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FwdMacEntry {
    pub addr: [u8; 6],
    pub mask: [u8; 6],
    pub tx_target: u8,
    pub tx_valid: bool,
    pub rx_valid: bool,
    pub udma_mask: u8,
    pub qid: u8,
    pub filter: bool,
}

/// struct al_eth_fwd_ctrl_table_entry
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FwdCtrlEntry {
    pub prio_sel: u32,
    pub queue_sel_1: u32,
    pub queue_sel_2: u32,
    pub udma_sel: u32,
    pub hdr_split_len_sel: u32,
    pub filter: bool,
}

const fn bytes_lo(a: &[u8; 6]) -> u32 {
    ((a[2] as u32) << 24) | ((a[3] as u32) << 16) | ((a[4] as u32) << 8) | a[5] as u32
}

const fn bytes_hi(a: &[u8; 6]) -> u32 {
    ((a[0] as u32) << 8) | a[1] as u32
}

fn mac_idx(idx: usize) -> Result<usize> {
    if idx < k::AL_ETH_FWD_MAC_NUM as usize {
        Ok(idx * off::EC_FWD_MAC_STRIDE)
    } else {
        Err(Error::Invalid)
    }
}

/// al_eth_mac_addr_store
pub fn mac_addr_store<R: RegIo>(ec: &R, idx: usize, addr: &[u8; 6]) -> Result {
    let o = mac_idx(idx)?;
    ec.w32(off::EC_FWD_MAC0_DATA_L + o, bytes_lo(addr));
    ec.w32(off::EC_FWD_MAC0_DATA_H + o, bytes_hi(addr));
    Ok(())
}

/// al_eth_mac_addr_read
pub fn mac_addr_read<R: RegIo>(ec: &R, idx: usize) -> Result<[u8; 6]> {
    let o = mac_idx(idx)?;
    let lo = ec.r32(off::EC_FWD_MAC0_DATA_L + o);
    let hi = ec.r32(off::EC_FWD_MAC0_DATA_H + o) as u16;
    Ok([(hi >> 8) as u8, hi as u8, (lo >> 24) as u8, (lo >> 16) as u8, (lo >> 8) as u8, lo as u8])
}

/// al_eth_fwd_mac_table_set
pub fn fwd_mac_table_set<R: RegIo>(ec: &R, idx: usize, e: &FwdMacEntry) -> Result {
    let o = mac_idx(idx)?;
    ec.w32(off::EC_FWD_MAC0_DATA_L + o, bytes_lo(&e.addr));
    ec.w32(off::EC_FWD_MAC0_DATA_H + o, bytes_hi(&e.addr));
    ec.w32(off::EC_FWD_MAC0_MASK_L + o, bytes_lo(&e.mask));
    ec.w32(off::EC_FWD_MAC0_MASK_H + o, bytes_hi(&e.mask));
    let mut v = 0;
    if e.filter {
        v |= k::EC_FWD_MAC_CTRL_RX_VAL_DROP;
    }
    v |= ((e.udma_mask as u32) << k::EC_FWD_MAC_CTRL_RX_VAL_UDMA_SHIFT) & k::EC_FWD_MAC_CTRL_RX_VAL_UDMA_MASK;
    v |= ((e.qid as u32) << k::EC_FWD_MAC_CTRL_RX_VAL_QID_SHIFT) & k::EC_FWD_MAC_CTRL_RX_VAL_QID_MASK;
    if e.rx_valid {
        v |= k::EC_FWD_MAC_CTRL_RX_VALID;
    }
    v |= ((e.tx_target as u32) << k::EC_FWD_MAC_CTRL_TX_VAL_SHIFT) & k::EC_FWD_MAC_CTRL_TX_VAL_MASK;
    if e.tx_valid {
        v |= k::EC_FWD_MAC_CTRL_TX_VALID;
    }
    ec.w32(off::EC_FWD_MAC0_CTRL + o, v);
    Ok(())
}

/// al_eth_thash_table_set
pub fn thash_table_set<R: RegIo>(ec: &R, idx: u32, udma: u8, queue: u32) -> Result {
    if idx >= k::AL_ETH_RX_THASH_TABLE_SIZE {
        return Err(Error::Invalid);
    }
    let mut e = ((udma as u32) << k::AL_ETH_THASH_UDMA_SHIFT) & k::AL_ETH_THASH_UDMA_MASK;
    e |= (queue << k::AL_ETH_THASH_Q_SHIFT) & k::AL_ETH_THASH_Q_MASK;
    ec.w32(off::EC_RFW_THASH_TABLE_ADDR, idx);
    ec.w32(off::EC_RFW_THASH_TABLE_DATA, e);
    Ok(())
}

/// al_eth_fsm_table_set
pub fn fsm_table_set<R: RegIo>(ec: &R, idx: u32, entry: u32) -> Result {
    if idx >= k::AL_ETH_RX_FSM_TABLE_SIZE {
        return Err(Error::Invalid);
    }
    ec.w32(off::EC_RFW_FSM_TABLE_ADDR, idx);
    ec.w32(off::EC_RFW_FSM_TABLE_DATA, entry);
    Ok(())
}

/// al_eth_fwd_pbits_table_set
pub fn fwd_pbits_table_set<R: RegIo>(ec: &R, idx: u32, prio: u8) -> Result {
    if idx >= k::AL_ETH_FWD_PBITS_TABLE_NUM || prio as u32 >= k::AL_ETH_FWD_PRIO_TABLE_NUM {
        return Err(Error::Invalid);
    }
    ec.w32(off::EC_RFW_PBITS_TABLE_ADDR, idx);
    ec.w32(off::EC_RFW_PBITS_TABLE_DATA, prio as u32);
    Ok(())
}

/// al_eth_fwd_priority_table_set
pub fn fwd_priority_table_set<R: RegIo>(ec: &R, prio: u8, qid: u8) -> Result {
    if prio as u32 >= k::AL_ETH_FWD_PRIO_TABLE_NUM {
        return Err(Error::Invalid);
    }
    ec.w32(off::EC_RFW_PRIORITY0_QUEUE + prio as usize * off::EC_RFW_PRIORITY_STRIDE, qid as u32);
    Ok(())
}

/// al_eth_ctrl_table_def_set (+ al_eth_fwd_ctrl_entry_to_val)
pub fn ctrl_table_def_set<R: RegIo>(ec: &R, use_table: bool, e: &FwdCtrlEntry) {
    let mut v = 0;
    v = field_set(v, 0xf, 0, e.prio_sel);
    v = field_set(v, 0xf0, 4, e.queue_sel_1);
    v = field_set(v, 0x300, 8, e.queue_sel_2);
    v = field_set(v, 0x3c00, 10, e.udma_sel);
    v = field_set(v, 0x38000, 15, e.hdr_split_len_sel);
    if e.hdr_split_len_sel != k::AL_ETH_CTRL_TABLE_HDR_SPLIT_LEN_SEL_0 {
        v |= 1 << 18;
    }
    v = field_set(v, 1 << 19, 19, e.filter as u32);
    if use_table {
        v |= k::EC_RFW_CTRL_TABLE_DEF_SEL;
    }
    ec.w32(off::EC_RFW_CTRL_TABLE_DEF, v);
}

/// al_eth_hash_key_set
pub fn hash_key_set<R: RegIo>(ec: &R, idx: u32, val: u32) -> Result {
    if idx >= k::AL_ETH_RX_HASH_KEY_NUM {
        return Err(Error::Invalid);
    }
    ec.w32(off::EC_RFW_HASH0_KEY + idx as usize * off::EC_RFW_HASH_STRIDE, val);
    Ok(())
}

/// al_eth_epe_entry_set
fn epe_entry_set<R: RegIo>(ec: &R, idx: usize, p: &[u32; 3], c: &[u32; 6]) {
    let ep = idx * off::EC_EPE_P_STRIDE;
    ec.w32(off::EC_EPE_P0_COMP_DATA + ep, p[0]);
    ec.w32(off::EC_EPE_P0_COMP_MASK + ep, p[1]);
    ec.w32(off::EC_EPE_P0_COMP_CTRL + ep, p[2]);
    let mp = idx * off::EC_MSP_C_STRIDE;
    ec.w32(off::EC_MSP_C0_P_COMP_DATA + mp, p[0]);
    ec.w32(off::EC_MSP_C0_P_COMP_MASK + mp, p[1]);
    ec.w32(off::EC_MSP_C0_P_COMP_CTRL + mp, p[2]);
    for t in 0..2 {
        let e = t * off::EC_EPE_STRIDE;
        ec.w32(off::EC_EPE0_ACT_TABLE_ADDR + e, idx as u32);
        ec.w32(off::EC_EPE0_ACT_TABLE_DATA_6 + e, c[5]);
        ec.w32(off::EC_EPE0_ACT_TABLE_DATA_2 + e, c[1]);
        ec.w32(off::EC_EPE0_ACT_TABLE_DATA_3 + e, c[2]);
        ec.w32(off::EC_EPE0_ACT_TABLE_DATA_4 + e, c[3]);
        ec.w32(off::EC_EPE0_ACT_TABLE_DATA_5 + e, c[4]);
        ec.w32(off::EC_EPE0_ACT_TABLE_DATA_1 + e, c[0]);
    }
}

/// al_eth_epe_init with the Rx parser enabled (the driver always enables it).
fn epe_init<R: RegIo>(ec: &R) {
    for (i, (p, c)) in tables::EPE_P_REGS.iter().zip(tables::EPE_CONTROL.iter()).enumerate() {
        epe_entry_set(ec, i, p, c);
    }
    ec.w32(off::EC_EPE0_RES_DEF, 0x0800_0080);
    ec.w32(off::EC_EPE0_RES_IN, 0x7);
    ec.w32(off::EC_EPE0_RES_DEF + off::EC_EPE_STRIDE, 0x0800_0080);
    ec.w32(off::EC_EPE0_RES_IN + off::EC_EPE_STRIDE, 0);
    ec.w32(off::EC_EPE_H8_HDR_LEN, (4 << 16) | 4);
    ec.w32(off::EC_RFW_META, k::EC_RFW_META_L3_LEN_CALC);
    ec.w32(off::EC_RFW_CHECKSUM, k::EC_RFW_CHECKSUM_HDR_SEL);
}

/// The EC part of al_eth_adapter_init for UDMA 0, revision 2: TSO meta
/// cache, engines, parser, and clearing of the SRAMs before their parity
/// interrupts are unmasked (RMN 5714).
pub(crate) fn init<R: RegIo>(ec: &R) {
    for i in 0..4 {
        ec.w32(off::EC_TSO_CACHE_TABLE_ADDR, i);
        ec.w32(off::EC_TSO_CACHE_TABLE_DATA_1, 0);
        ec.w32(off::EC_TSO_CACHE_TABLE_DATA_2, 0);
        ec.w32(off::EC_TSO_CACHE_TABLE_DATA_3, 0);
        ec.w32(off::EC_TSO_CACHE_TABLE_DATA_4, 0);
    }
    ec.w32(off::EC_GEN_EN, 0xffff_ffff);
    ec.w32(off::EC_GEN_FIFO_EN, 0xffff_ffff);
    masked(ec, off::EC_GEN_EN_EXT, k::EC_GEN_EN_EXT_CACHE_WORD_SPLIT, k::EC_GEN_EN_EXT_CACHE_WORD_SPLIT);
    ec.w32(off::EC_TSO_CFG_ADD_0, k::EC_TSO_CFG_ADD_0_MSS_SEL);
    ec.w32(off::EC_TSO_CFG_TUNNEL, k::EC_TSO_CFG_TUNNEL_INIT);
    ec.w32(off::EC_MAC_GEN, 1);
    ec.w32(off::EC_TMI_TX_CFG, k::EC_TMI_TX_CFG_EN_FWD_TO_RX | k::EC_TMI_TX_CFG_SWAP_BYTES);
    ec.w32(off::EC_TFW_UDMA0_FWD_DEC, 0x3fb);
    ec.w32(off::EC_RFW_DEFAULT0_OPT_1, 1);
    ec.w32(off::EC_RFW_VID_TABLE_ADDR, 0);
    ec.w32(off::EC_RFW_VID_TABLE_DATA, 0);
    ec.w32(off::EC_RFW_THASH_CFG_1, k::EC_RFW_THASH_CFG_1_ENABLE_IP_SWAP | k::EC_RFW_THASH_CFG_1_ENABLE_PORT_SWAP);
    epe_init(ec);
    let r = ec.r32(off::EC_TSO_IN_CFG);
    ec.w32(off::EC_TSO_IN_CFG, r & !0x7f00);
    // SRAMs: MHASH, THASH, VLAN, control table, Tx VLAN table
    for i in 0..k::AL_ETH_FWD_MAC_HASH_NUM {
        ec.w32(off::EC_RFW_MHASH_TABLE_ADDR, i);
        ec.w32(off::EC_RFW_MHASH_TABLE_DATA, 0);
    }
    for i in 0..k::AL_ETH_RX_THASH_TABLE_SIZE {
        ec.w32(off::EC_RFW_THASH_TABLE_ADDR, i);
        ec.w32(off::EC_RFW_THASH_TABLE_DATA, 0);
    }
    for i in 0..k::AL_ETH_FWD_VID_TABLE_NUM {
        ec.w32(off::EC_RFW_VID_TABLE_ADDR, i);
        ec.w32(off::EC_RFW_VID_TABLE_DATA, 0);
    }
    for i in 0..k::AL_ETH_RX_CTRL_TABLE_SIZE {
        ec.w32(off::EC_RFW_CTRL_TABLE_ADDR, i);
        ec.w32(off::EC_RFW_CTRL_TABLE_DATA, 0);
    }
    for i in 0..k::AL_ETH_FWD_VID_TABLE_NUM {
        ec.w32(off::EC_TFW_TX_VID_TABLE_ADDR, i);
        ec.w32(off::EC_TFW_TX_VID_TABLE_DATA, 0);
    }
}

/// al_eth_ec_error_ints_masks_get for revision 2, with the RMN 5715 / 11018 /
/// 5714 workarounds: the EC error interrupts to unmask, groups A..D.
pub(crate) const fn error_int_masks() -> [u32; 4] {
    let a = k::AL_ETH_EC_IOFIC_GROUP_A_ERROR_INTS
        & !(k::AL_ETH_EC_IOFIC_GROUP_A_ERR_MSW_IN_SOP_IN_PKT
            | k::AL_ETH_EC_IOFIC_GROUP_A_ERR_MSW_IN_EOP_IN_IDLE
            | k::AL_ETH_EC_IOFIC_GROUP_A_ERR_FWD_OUT_EOP_META_EMPTY
            | k::AL_ETH_EC_IOFIC_GROUP_A_ERR_RXF_OUT_EOP_HDR_EMPTY);
    let b = k::AL_ETH_V2_V3_EC_IOFIC_GROUP_B_ERROR_INTS
        & !(k::AL_ETH_EC_IOFIC_GROUP_B_ERR_TMI_IN_SOP_IN_PKT | k::AL_ETH_EC_IOFIC_GROUP_B_ERR_TFW_OUT_EOP_HDR_EMPTY);
    let c = k::AL_ETH_V1_V2_EC_IOFIC_GROUP_C_ERROR_INTS
        & !k::AL_ETH_EC_IOFIC_GROUP_C_TSO_META_CACHE_1_4_PARITY
        & !k::AL_ETH_EC_IOFIC_GROUP_C_RMN_5714_PARITY;
    let d = k::AL_ETH_V2_V3_EC_IOFIC_GROUP_D_ERROR_INTS;
    [a, b, c, d]
}

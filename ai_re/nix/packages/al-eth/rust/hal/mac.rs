// SPDX-License-Identifier: GPL-2.0

//! The MAC window: mode setup, link parameters, MDIO clock, start/stop,
//! frame length, pause frames, link status, statistics (al_hal_eth_main.c).
//! Our boards: RGMII (DMC-12), SGMII and 10G serial (CS8210). The other
//! modes of the C HAL (25/40/50G, 2.5G SGMII, 10G SGMII) are not ported.

use crate::hal::io::{field_set, masked, RegIo};
use crate::hal::regs::{k, off};
use crate::hal::{Error, Result};

/// enum al_eth_mac_mode, the ported subset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacMode {
    Rgmii,
    Sgmii,
    TenGSerial,
}

impl MacMode {
    /// The C enum value.
    pub const fn raw(self) -> u32 {
        match self {
            MacMode::Rgmii => k::AL_ETH_MAC_MODE_RGMII,
            MacMode::Sgmii => k::AL_ETH_MAC_MODE_SGMII,
            MacMode::TenGSerial => k::AL_ETH_MAC_MODE_10GbE_Serial,
        }
    }

    pub fn from_raw(v: u32) -> Result<Self> {
        match v {
            k::AL_ETH_MAC_MODE_RGMII => Ok(MacMode::Rgmii),
            k::AL_ETH_MAC_MODE_SGMII => Ok(MacMode::Sgmii),
            k::AL_ETH_MAC_MODE_10GbE_Serial => Ok(MacMode::TenGSerial),
            _ => Err(Error::Unsupported),
        }
    }

    /// AL_ETH_IS_1G_MAC
    pub const fn is_1g(self) -> bool {
        matches!(self, MacMode::Rgmii | MacMode::Sgmii)
    }
}

/// al_eth_mac_config for revision 2.
pub fn config<R: RegIo>(mac: &R, mode: MacMode) {
    let gen_1g = |clk: u32, cfg: u32| {
        mac.w32(off::MAC_GEN_CLK_CFG, clk);
        mac.w32(off::MAC_1G_CMD_CFG, 0x0180_0010);
        mac.w32(off::MAC_1G_RX_SECTION_EMPTY, 0);
        mac.w32(off::MAC_1G_RX_SECTION_FULL, 0xc);
        mac.w32(off::MAC_1G_RX_ALMOST_EMPTY, 8);
        mac.w32(off::MAC_1G_RX_ALMOST_FULL, 8);
        mac.w32(off::MAC_1G_TX_SECTION_EMPTY, 8);
        mac.w32(off::MAC_1G_TX_SECTION_FULL, 0xc);
        mac.w32(off::MAC_1G_TX_ALMOST_EMPTY, 8);
        mac.w32(off::MAC_1G_TX_ALMOST_FULL, 8);
        mac.w32(off::MAC_GEN_CFG, cfg);
        mac.w32(off::MAC_GEN_MAC_1G_CFG, 2);
        masked(mac, off::MAC_GEN_MUX_SEL, !k::ETH_MAC_GEN_MUX_SEL_KR_IN_MASK, 0x0006_3910);
    };
    match mode {
        MacMode::Rgmii => {
            gen_1g(0x4000_3210, 0);
            mac.w32(off::MAC_GEN_RGMII_SEL, 0xf);
        }
        MacMode::Sgmii => {
            gen_1g(0x4005_3210, 0xc0);
            mac.w32(off::MAC_GEN_SD_FIFO_CTRL, 0x4f0);
            mac.w32(off::MAC_GEN_SD_FIFO_CTRL, 0x401);
            // PCS interface mode SGMII (not 1000Base-X); dev_ability 1000 Mb/s
            mac.w32(off::MAC_SGMII_REG_ADDR, 0x14);
            mac.w32(off::MAC_SGMII_REG_DATA, 0xb);
            mac.w32(off::MAC_SGMII_REG_ADDR, 4);
            mac.w32(off::MAC_SGMII_REG_DATA, 0x9a0);
            masked(mac, off::MAC_GEN_LED_CFG, k::ETH_MAC_GEN_LED_CFG_SEL_MASK, k::ETH_MAC_GEN_LED_CFG_SEL_DEFAULT_REG);
        }
        MacMode::TenGSerial => {
            mac.w32(off::MAC_10G_CMD_CFG, 0x0102_2810);
            mac.w32(off::MAC_GEN_CFG, 5);
            mac.w32(off::MAC_GEN_RXAUI_CFG, 7);
            mac.w32(off::MAC_GEN_SD_CFG, 0x1f1);
            mac.w32(off::MAC_GEN_XGMII_DFIFO_32_64, 0x401);
            mac.w32(off::MAC_GEN_XGMII_DFIFO_64_32, 0x401);
            masked(mac, off::MAC_GEN_MUX_SEL, !k::ETH_MAC_GEN_MUX_SEL_KR_IN_MASK, 0x0007_3910);
            mac.w32(off::MAC_GEN_CLK_CFG, 0x1000_3210);
            mac.w32(off::MAC_GEN_SD_FIFO_CTRL, 0x4f0);
            mac.w32(off::MAC_GEN_SD_FIFO_CTRL, 0x401);
            masked(mac, off::MAC_GEN_LED_CFG, k::ETH_MAC_GEN_LED_CFG_SEL_MASK, k::ETH_MAC_GEN_LED_CFG_SEL_DEFAULT_REG);
        }
    }
}

fn set(reg: &mut u32, mask: u32, on: bool) {
    if on {
        *reg |= mask;
    } else {
        *reg &= !mask;
    }
}

/// al_eth_mac_link_config: speed / duplex / autonegotiation of a 1G mode.
pub fn link_config<R: RegIo>(mac: &R, mode: MacMode, force_1000_base_x: bool, an_enable: bool, speed: u32, full_duplex: bool) -> Result {
    if !mode.is_1g() {
        return Err(Error::Invalid);
    }
    if !(mode != MacMode::Rgmii && an_enable) {
        if !matches!(speed, 10 | 100 | 1000) {
            return Err(Error::Invalid);
        }
        if speed == 1000 && !full_duplex {
            return Err(Error::Invalid);
        }
    }
    // al_eth_mac_link_config_1g_mac
    let mut mac_ctrl = mac.r32(off::MAC_1G_CMD_CFG);
    let mut sgmii_ctrl = 0;
    let mut sgmii_if_mode = 0;
    if mode == MacMode::Sgmii {
        mac.w32(off::MAC_SGMII_REG_ADDR, k::ETH_MAC_SGMII_REG_ADDR_CTRL_REG);
        sgmii_ctrl = mac.r32(off::MAC_SGMII_REG_DATA);
        if !force_1000_base_x {
            sgmii_if_mode = k::ETH_MAC_SGMII_REG_DATA_IF_MODE_SGMII_EN;
        }
        if an_enable {
            sgmii_if_mode |= k::ETH_MAC_SGMII_REG_DATA_IF_MODE_SGMII_AN;
            sgmii_ctrl |= k::ETH_MAC_SGMII_REG_DATA_CTRL_AN_ENABLE;
        } else {
            sgmii_ctrl &= !k::ETH_MAC_SGMII_REG_DATA_CTRL_AN_ENABLE;
        }
    }
    if mode == MacMode::Rgmii {
        // speed from the MAC, not from the PHY
        let mut rgmii = mac.r32(off::MAC_GEN_RGMII_CFG);
        rgmii &= !(k::ETH_MAC_GEN_RGMII_CFG_ENA_AUTO | k::ETH_MAC_GEN_RGMII_CFG_SET_1000_SEL | k::ETH_MAC_GEN_RGMII_CFG_SET_10_SEL);
        mac.w32(off::MAC_GEN_RGMII_CFG, rgmii);
    }
    if full_duplex {
        mac_ctrl &= !k::ETH_1G_MAC_CMD_CFG_HD_EN;
    } else {
        mac_ctrl |= k::ETH_1G_MAC_CMD_CFG_HD_EN;
        sgmii_if_mode |= k::ETH_MAC_SGMII_REG_DATA_IF_MODE_SGMII_DUPLEX;
    }
    if speed == 1000 {
        mac_ctrl |= k::ETH_1G_MAC_CMD_CFG_1G_SPD;
        sgmii_if_mode |= k::ETH_MAC_SGMII_REG_DATA_IF_MODE_SGMII_SPEED_1000;
    } else {
        mac_ctrl &= !k::ETH_1G_MAC_CMD_CFG_1G_SPD;
        if speed == 10 {
            mac_ctrl |= k::ETH_1G_MAC_CMD_CFG_10M_SPD;
        } else {
            sgmii_if_mode |= k::ETH_MAC_SGMII_REG_DATA_IF_MODE_SGMII_SPEED_100;
            mac_ctrl &= !k::ETH_1G_MAC_CMD_CFG_10M_SPD;
        }
    }
    if mode == MacMode::Sgmii {
        mac.w32(off::MAC_SGMII_REG_ADDR, k::ETH_MAC_SGMII_REG_ADDR_IF_MODE_REG);
        mac.w32(off::MAC_SGMII_REG_DATA, sgmii_if_mode);
        mac.w32(off::MAC_SGMII_REG_ADDR, k::ETH_MAC_SGMII_REG_ADDR_CTRL_REG);
        mac.w32(off::MAC_SGMII_REG_DATA, sgmii_ctrl);
    }
    mac.w32(off::MAC_1G_CMD_CFG, mac_ctrl);
    Ok(())
}

/// enum al_eth_ref_clk_freq -> kHz (al_eth_mdio_config; unknown values are
/// an error message and 375 MHz in C).
const fn ref_clk_khz(f: u32) -> u32 {
    match f {
        k::AL_ETH_REF_FREQ_187_5_MHZ => 187_500,
        k::AL_ETH_REF_FREQ_250_MHZ => 250_000,
        k::AL_ETH_REF_FREQ_500_MHZ => 500_000,
        k::AL_ETH_REF_FREQ_428_MHZ => 428_000,
        _ => 375_000,
    }
}

/// al_eth_mdio_config: always through the 10G MAC's MDIO (RMN 3022/3236).
pub fn mdio_config<R: RegIo>(mac: &R, clause_45: bool, ref_clk_freq: u32, mdio_clk_khz: u32) -> Result {
    if mdio_clk_khz == 0 {
        return Err(Error::Invalid);
    }
    let v = mac.r32(off::MAC_GEN_CFG);
    mac.w32(off::MAC_GEN_CFG, v | (1 << 10));
    let mut v = mac.r32(off::MAC_10G_MDIO_CFG_STATUS);
    set(&mut v, 1 << 6, clause_45);
    v &= !(0x1ff << 7);
    v |= (ref_clk_khz(ref_clk_freq) / (2 * mdio_clk_khz)) << 7;
    v = field_set(v, k::ETH_10G_MAC_MDIO_CFG_HOLD_TIME_MASK, k::ETH_10G_MAC_MDIO_CFG_HOLD_TIME_SHIFT, k::ETH_10G_MAC_MDIO_CFG_HOLD_TIME_7_CLK);
    mac.w32(off::MAC_10G_MDIO_CFG_STATUS, v);
    Ok(())
}

fn cmd_cfg(mode: MacMode) -> (usize, u32) {
    if mode.is_1g() {
        (off::MAC_1G_CMD_CFG, k::ETH_1G_MAC_CMD_CFG_TX_ENA | k::ETH_1G_MAC_CMD_CFG_RX_ENA)
    } else {
        (off::MAC_10G_CMD_CFG, k::ETH_10G_MAC_CMD_CFG_TX_ENA | k::ETH_10G_MAC_CMD_CFG_RX_ENA)
    }
}

/// al_eth_mac_start
pub fn start<R: RegIo>(mac: &R, mode: MacMode) {
    let (o, m) = cmd_cfg(mode);
    masked(mac, o, m, m);
}

/// al_eth_mac_stop
pub fn stop<R: RegIo>(mac: &R, mode: MacMode) {
    let (o, m) = cmd_cfg(mode);
    masked(mac, o, m, 0);
}

/// al_eth_rx_pkt_limit_config (revision 2: no Rx async FIFO limits)
pub fn rx_pkt_limit_config<R: RegIo>(ec: &R, mac: &R, mode: MacMode, min_rx_len: u32, max_rx_len: u32) -> Result {
    if max_rx_len > k::AL_ETH_MAX_FRAME_LEN {
        return Err(Error::Invalid);
    }
    ec.w32(off::EC_MAC_MIN_PKT, min_rx_len);
    ec.w32(off::EC_MAC_MAX_PKT, max_rx_len);
    // 16 more in the MAC, so the EC trims rather than the MAC
    let frm = if mode.is_1g() { off::MAC_1G_FRM_LEN } else { off::MAC_10G_FRM_LEN };
    mac.w32(frm, max_rx_len + 16);
    Ok(())
}

/// struct al_eth_flow_control_params, link pause only (PFC is not ported).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkPause {
    pub obey: bool,
    pub generate: bool,
    pub rx_fifo_th_high: u16,
    pub rx_fifo_th_low: u16,
    pub quanta: u16,
    pub quanta_th: u16,
    /// prio_q_map: per UDMA, per priority the queue bit mask (only [u][0] is used).
    pub prio_q_map: [[u8; 8]; 4],
}

/// al_eth_flow_control_config, AL_ETH_FLOW_CONTROL_TYPE_LINK_PAUSE.
pub fn link_pause_config<R: RegIo>(ec: &R, mac: &R, mode: MacMode, p: &LinkPause) {
    if mode.is_1g() {
        mac.w32(off::MAC_1G_PAUSE_QUANT, p.quanta as u32);
        ec.w32(off::EC_EFC_XOFF_TIMER_1G, p.quanta_th as u32);
    } else {
        mac.w32(off::MAC_10G_CL01_PAUSE_QUANTA, p.quanta as u32);
        mac.w32(off::MAC_10G_CL01_QUANTA_THRESH, p.quanta_th as u32);
    }
    ec.w32(off::EC_EFC_EC_PAUSE, p.obey as u32);
    ec.w32(off::EC_EFC_EC_XOFF, if p.generate { 1 << k::EC_EFC_EC_XOFF_MASK_2_SHIFT } else { 0 });
    if mode.is_1g() {
        ec.w32(off::EC_EFC_XON, k::EC_EFC_XON_MASK_2 | k::EC_EFC_XON_MASK_1);
    }
    ec.w32(off::EC_EFC_RX_FIFO_HYST, p.rx_fifo_th_low as u32 | ((p.rx_fifo_th_high as u32) << k::EC_EFC_RX_FIFO_HYST_TH_HIGH_SHIFT));
    for (i, map) in p.prio_q_map.iter().enumerate() {
        let o = i * off::EC_FC_UDMA_STRIDE;
        ec.w32(off::EC_FC_UDMA0_Q_PAUSE_0 + o, if p.obey { map[0] as u32 } else { 0 });
        ec.w32(off::EC_FC_UDMA0_Q_XOFF_0 + o, if p.generate { map[0] as u32 } else { 0 });
    }
}

/// al_eth_link_status_get: link up?
pub fn link_up<R: RegIo>(mac: &R, mode: MacMode) -> bool {
    match mode {
        MacMode::TenGSerial => {
            // al_eth_kr_pcs_reg_read(BASE_R_STATUS2)
            mac.w32(off::MAC_KR_PCS_ADDR, k::ETH_MAC_KR_PCS_BASE_R_STATUS2);
            let r = mac.r32(off::MAC_KR_PCS_DATA) as u16;
            if r & (1 << 15) == 0 {
                return false;
            }
            let s = mac.r32(off::MAC_10G_STATUS);
            s & (k::ETH_MAC_GEN_MAC_10G_STAT_REM_FAULT | k::ETH_MAC_GEN_MAC_10G_STAT_LOC_FAULT) == 0
        }
        MacMode::Sgmii => {
            mac.w32(off::MAC_SGMII_REG_ADDR, 1);
            // latched low: the second read is the current state
            let _ = mac.r32(off::MAC_SGMII_REG_DATA);
            let r = mac.r32(off::MAC_SGMII_REG_DATA);
            let up = r & (1 << 2) != 0;
            let st = mac.r32(off::MAC_SGMII_LINK_STAT);
            up && st & (1 << 5) != 0
        }
        MacMode::Rgmii => mac.r32(off::MAC_GEN_RGMII_STAT) & (1 << 4) != 0,
    }
}

/// struct al_eth_mac_stats, the fields the driver reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MacStats {
    pub if_in_ucast_pkts: u64,
    pub if_in_multicast_pkts: u64,
    pub if_in_broadcast_pkts: u64,
    pub ether_stats_pkts: u64,
    pub if_out_ucast_pkts: u64,
    pub if_out_multicast_pkts: u64,
    pub if_out_broadcast_pkts: u64,
    pub if_in_errors: u64,
    pub if_out_errors: u64,
    pub frames_received_ok: u64,
    pub frames_transmitted_ok: u64,
    pub octets_received_ok: u64,
    pub octets_transmitted_ok: u64,
    pub undersize_pkts: u64,
    pub fragments: u64,
    pub jabbers: u64,
    pub oversize_pkts: u64,
    pub fcs_errors: u64,
    pub alignment_errors: u64,
    pub drop_events: u64,
    pub pause_tx: u64,
    pub pause_rx: u64,
    pub frame_too_long_errors: u64,
    pub in_range_length_errors: u64,
    pub vlan_tx_ok: u64,
    pub vlan_rx_ok: u64,
    pub octets: u64,
    pub pkts_64: u64,
    pub pkts_65_127: u64,
    pub pkts_128_255: u64,
    pub pkts_256_511: u64,
    pub pkts_512_1023: u64,
    pub pkts_1024_1518: u64,
    pub pkts_1519_max: u64,
    pub eee_in: u64,
    pub eee_out: u64,
}

/// al_eth_mac_stats_get (1G MAC; 10G MAC revision 2 layout). Same register
/// read order as the C (the counters may be clear-on-read).
pub fn stats<R: RegIo>(mac: &R, mode: MacMode) -> MacStats {
    let r = |o: usize| mac.r32(o) as u64;
    let mut s = MacStats::default();
    if mode.is_1g() {
        s.if_in_ucast_pkts = r(off::MAC_1G_STAT_ifInUcastPkts);
        s.if_in_multicast_pkts = r(off::MAC_1G_STAT_ifInMulticastPkts);
        s.if_in_broadcast_pkts = r(off::MAC_1G_STAT_ifInBroadcastPkts);
        s.ether_stats_pkts = r(off::MAC_1G_STAT_etherStatsPkts);
        s.if_out_ucast_pkts = r(off::MAC_1G_STAT_ifOutUcastPkts);
        s.if_out_multicast_pkts = r(off::MAC_1G_STAT_ifOutMulticastPkts);
        s.if_out_broadcast_pkts = r(off::MAC_1G_STAT_ifOutBroadcastPkts);
        s.if_in_errors = r(off::MAC_1G_STAT_ifInErrors);
        s.if_out_errors = r(off::MAC_1G_STAT_ifOutErrors);
        s.frames_received_ok = r(off::MAC_1G_STAT_aFramesReceivedOK);
        s.frames_transmitted_ok = r(off::MAC_1G_STAT_aFramesTransmittedOK);
        s.octets_received_ok = r(off::MAC_1G_STAT_aOctetsReceivedOK);
        s.octets_transmitted_ok = r(off::MAC_1G_STAT_aOctetsTransmittedOK);
        s.undersize_pkts = r(off::MAC_1G_STAT_etherStatsUndersizePkts);
        s.fragments = r(off::MAC_1G_STAT_etherStatsFragments);
        s.jabbers = r(off::MAC_1G_STAT_etherStatsJabbers);
        s.oversize_pkts = r(off::MAC_1G_STAT_etherStatsOversizePkts);
        s.fcs_errors = r(off::MAC_1G_STAT_aFrameCheckSequenceErrors);
        s.alignment_errors = r(off::MAC_1G_STAT_aAlignmentErrors);
        s.drop_events = r(off::MAC_1G_STAT_etherStatsDropEvents);
        s.pause_tx = r(off::MAC_1G_STAT_aPAUSEMACCtrlFramesTransmitted);
        s.pause_rx = r(off::MAC_1G_STAT_aPAUSEMACCtrlFramesReceived);
        s.octets = r(off::MAC_1G_STAT_etherStatsOctets);
        s.pkts_64 = r(off::MAC_1G_STAT_etherStatsPkts64Octets);
        s.pkts_65_127 = r(off::MAC_1G_STAT_etherStatsPkts65to127Octets);
        s.pkts_128_255 = r(off::MAC_1G_STAT_etherStatsPkts128to255Octets);
        s.pkts_256_511 = r(off::MAC_1G_STAT_etherStatsPkts256to511Octets);
        s.pkts_512_1023 = r(off::MAC_1G_STAT_etherStatsPkts512to1023Octets);
        s.pkts_1024_1518 = r(off::MAC_1G_STAT_etherStatsPkts1024to1518Octets);
        s.pkts_1519_max = r(off::MAC_1G_STAT_etherStatsPkts1519toX);
    } else {
        s.if_in_ucast_pkts = r(off::MAC_10G_STAT_ifInUcastPkts);
        s.if_in_multicast_pkts = r(off::MAC_10G_STAT_ifInMulticastPkts);
        s.if_in_broadcast_pkts = r(off::MAC_10G_STAT_ifInBroadcastPkts);
        s.ether_stats_pkts = r(off::MAC_10G_STAT_etherStatsPkts);
        s.if_out_ucast_pkts = r(off::MAC_10G_STAT_ifOutUcastPkts);
        s.if_out_multicast_pkts = r(off::MAC_10G_STAT_ifOutMulticastPkts);
        s.if_out_broadcast_pkts = r(off::MAC_10G_STAT_ifOutBroadcastPkts);
        s.if_in_errors = r(off::MAC_10G_STAT_ifInErrors);
        s.if_out_errors = r(off::MAC_10G_STAT_ifOutErrors);
        s.frames_received_ok = r(off::MAC_10G_STAT_aFramesReceivedOK);
        s.frames_transmitted_ok = r(off::MAC_10G_STAT_aFramesTransmittedOK);
        // octets OK = interface octets - 18 per frame - 4 per VLAN frame; the C
        // multiplies the 32-bit counters in 32 bits (uint32_t fields), so do we
        let mul32 = |a: u32, b: u64| a.wrapping_mul(b as u32) as u64;
        let mut o = r(off::MAC_10G_STAT_ifInOctetsL) | (r(off::MAC_10G_STAT_ifInOctetsH) << 32);
        o = o.wrapping_sub(mul32(18, s.frames_received_ok));
        o = o.wrapping_sub(mul32(4, r(off::MAC_10G_STAT_VLANReceivedOK)));
        s.octets_received_ok = o;
        let mut o = r(off::MAC_10G_STAT_ifOutOctetsL) | (r(off::MAC_10G_STAT_ifOutOctetsH) << 32);
        o = o.wrapping_sub(mul32(18, s.frames_transmitted_ok));
        o = o.wrapping_sub(mul32(4, r(off::MAC_10G_STAT_VLANTransmittedOK)));
        s.octets_transmitted_ok = o;
        s.undersize_pkts = r(off::MAC_10G_STAT_etherStatsUndersizePkts);
        s.fragments = r(off::MAC_10G_STAT_etherStatsFragments);
        s.jabbers = r(off::MAC_10G_STAT_etherStatsJabbers);
        s.oversize_pkts = r(off::MAC_10G_STAT_etherStatsOversizePkts);
        s.fcs_errors = r(off::MAC_10G_STAT_aFrameCheckSequenceErrors);
        s.alignment_errors = r(off::MAC_10G_STAT_aAlignmentErrors);
        s.drop_events = r(off::MAC_10G_STAT_etherStatsDropEvents);
        s.pause_tx = r(off::MAC_10G_STAT_aPAUSEMACCtrlFramesTransmitted);
        s.pause_rx = r(off::MAC_10G_STAT_aPAUSEMACCtrlFramesReceived);
        s.frame_too_long_errors = r(off::MAC_10G_STAT_aFrameTooLongErrors);
        s.in_range_length_errors = r(off::MAC_10G_STAT_aInRangeLengthErrors);
        s.vlan_tx_ok = r(off::MAC_10G_STAT_VLANTransmittedOK);
        s.vlan_rx_ok = r(off::MAC_10G_STAT_VLANReceivedOK);
        s.octets = r(off::MAC_10G_STAT_etherStatsOctets);
        s.pkts_64 = r(off::MAC_10G_STAT_etherStatsPkts64Octets);
        s.pkts_65_127 = r(off::MAC_10G_STAT_etherStatsPkts65to127Octets);
        s.pkts_128_255 = r(off::MAC_10G_STAT_etherStatsPkts128to255Octets);
        s.pkts_256_511 = r(off::MAC_10G_STAT_etherStatsPkts256to511Octets);
        s.pkts_512_1023 = r(off::MAC_10G_STAT_etherStatsPkts512to1023Octets);
        s.pkts_1024_1518 = r(off::MAC_10G_STAT_etherStatsPkts1024to1518Octets);
        s.pkts_1519_max = r(off::MAC_10G_STAT_etherStatsPkts1519toX);
    }
    s.eee_in = r(off::MAC_STAT_EEE_IN);
    s.eee_out = r(off::MAC_STAT_EEE_OUT);
    s
}

/// Number of counters in [MacStats].
pub const MAC_STATS_N: usize = 36;

impl MacStats {
    /// The counters in field order.
    pub const fn to_array(&self) -> [u64; MAC_STATS_N] {
        [
            self.if_in_ucast_pkts,
            self.if_in_multicast_pkts,
            self.if_in_broadcast_pkts,
            self.ether_stats_pkts,
            self.if_out_ucast_pkts,
            self.if_out_multicast_pkts,
            self.if_out_broadcast_pkts,
            self.if_in_errors,
            self.if_out_errors,
            self.frames_received_ok,
            self.frames_transmitted_ok,
            self.octets_received_ok,
            self.octets_transmitted_ok,
            self.undersize_pkts,
            self.fragments,
            self.jabbers,
            self.oversize_pkts,
            self.fcs_errors,
            self.alignment_errors,
            self.drop_events,
            self.pause_tx,
            self.pause_rx,
            self.frame_too_long_errors,
            self.in_range_length_errors,
            self.vlan_tx_ok,
            self.vlan_rx_ok,
            self.octets,
            self.pkts_64,
            self.pkts_65_127,
            self.pkts_128_255,
            self.pkts_256_511,
            self.pkts_512_1023,
            self.pkts_1024_1518,
            self.pkts_1519_max,
            self.eee_in,
            self.eee_out,
        ]
    }

    /// The inverse of [MacStats::to_array].
    pub const fn from_array(a: &[u64; MAC_STATS_N]) -> Self {
        Self {
            if_in_ucast_pkts: a[0],
            if_in_multicast_pkts: a[1],
            if_in_broadcast_pkts: a[2],
            ether_stats_pkts: a[3],
            if_out_ucast_pkts: a[4],
            if_out_multicast_pkts: a[5],
            if_out_broadcast_pkts: a[6],
            if_in_errors: a[7],
            if_out_errors: a[8],
            frames_received_ok: a[9],
            frames_transmitted_ok: a[10],
            octets_received_ok: a[11],
            octets_transmitted_ok: a[12],
            undersize_pkts: a[13],
            fragments: a[14],
            jabbers: a[15],
            oversize_pkts: a[16],
            fcs_errors: a[17],
            alignment_errors: a[18],
            drop_events: a[19],
            pause_tx: a[20],
            pause_rx: a[21],
            frame_too_long_errors: a[22],
            in_range_length_errors: a[23],
            vlan_tx_ok: a[24],
            vlan_rx_ok: a[25],
            octets: a[26],
            pkts_64: a[27],
            pkts_65_127: a[28],
            pkts_128_255: a[29],
            pkts_256_511: a[30],
            pkts_512_1023: a[31],
            pkts_1024_1518: a[32],
            pkts_1519_max: a[33],
            eee_in: a[34],
            eee_out: a[35],
        }
    }
}

/// 64-bit totals of the MAC's free-running counters (not in the C).
///
/// The MAC counters are 32-bit registers that wrap (aOctetsReceivedOK after
/// 4 GiB: 34 s at line rate) and the C reports them as they are, so its
/// rx_bytes/tx_bytes go down at every wrap (seen 2026-11-23 on the DMC-12 with
/// the Rust driver, which did the same). Each [MacTotals::fold] adds the
/// difference to the previous reading modulo 2^32, which is exact as long as
/// readings are less than one wrap apart. [MacTotals::start] takes the
/// baseline once the port is up (whatever the counters were reset to) and
/// [MacTotals::stop] folds the last reading before it goes down, so the totals
/// survive down/up and resets. The 10G MAC's octet counts are 64-bit values
/// (folded modulo 2^64).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MacTotals {
    last: [u64; MAC_STATS_N],
    total: [u64; MAC_STATS_N],
    live: bool,
}

impl Default for MacTotals {
    fn default() -> Self {
        Self::new()
    }
}

impl MacTotals {
    /// All zero, not live.
    pub const fn new() -> Self {
        Self { last: [0; MAC_STATS_N], total: [0; MAC_STATS_N], live: false }
    }

    /// Counting from this reading on (port up).
    pub fn start(&mut self, raw: &MacStats) {
        self.last = raw.to_array();
        self.live = true;
    }

    /// Adds what the counters advanced since the last reading (no-op unless live).
    pub fn fold(&mut self, raw: &MacStats, mode: MacMode) {
        if !self.live {
            return;
        }
        let r = raw.to_array();
        // octets_received_ok, octets_transmitted_ok
        const WIDE: [usize; 2] = [11, 12];
        for i in 0..MAC_STATS_N {
            let d = if !mode.is_1g() && WIDE.contains(&i) {
                r[i].wrapping_sub(self.last[i])
            } else {
                u64::from((r[i] as u32).wrapping_sub(self.last[i] as u32))
            };
            self.total[i] = self.total[i].wrapping_add(d);
        }
        self.last = r;
    }

    /// The last reading before the port goes down: folded, then not live.
    pub fn stop(&mut self, raw: &MacStats, mode: MacMode) {
        self.fold(raw, mode);
        self.live = false;
    }

    /// Whether [MacTotals::start] ran since the last [MacTotals::stop].
    pub const fn live(&self) -> bool {
        self.live
    }

    /// The totals.
    pub const fn totals(&self) -> MacStats {
        MacStats::from_array(&self.total)
    }
}

#[cfg(test)]
mod totals_tests {
    use super::*;

    fn raw(octets: u64, frames: u64) -> MacStats {
        MacStats { octets_received_ok: octets, frames_received_ok: frames, ..Default::default() }
    }

    #[test]
    fn mac_totals_wrap_and_restart() {
        // the field order WIDE relies on
        let mut s = MacStats::default();
        s.octets_received_ok = 1;
        s.octets_transmitted_ok = 2;
        assert_eq!(s.to_array()[11], 1);
        assert_eq!(s.to_array()[12], 2);
        assert_eq!(MacStats::from_array(&s.to_array()), s);

        let mut t = MacTotals::new();
        t.fold(&raw(5, 5), MacMode::Rgmii); // not live: ignored
        assert_eq!(t.totals(), MacStats::default());
        t.start(&raw(1_672_294_899, 4_800_000));
        let mut prev = 0;
        // 3.3e6 frames of ~1285 bytes, read every 100k frames: wraps once
        for k in 1..=33u64 {
            let o = (1_672_294_899 + k * 100_000 * 1285) & 0xffff_ffff;
            t.fold(&raw(o, 4_800_000 + k * 100_000), MacMode::Rgmii);
            let now = t.totals().octets_received_ok;
            assert!(now > prev, "monotonic");
            prev = now;
        }
        assert_eq!(t.totals().octets_received_ok, 33 * 100_000 * 1285);
        assert_eq!(t.totals().frames_received_ok, 3_300_000);
        // down, the counters reset by the next up, then more traffic
        t.stop(&raw((1_672_294_899 + 33 * 100_000 * 1285 + 1000) & 0xffff_ffff, 4_800_000 + 3_300_001), MacMode::Rgmii);
        t.fold(&raw(0, 0), MacMode::Rgmii); // down: ignored
        t.start(&raw(0, 0));
        t.fold(&raw(500, 1), MacMode::Rgmii);
        assert_eq!(t.totals().octets_received_ok, 33 * 100_000 * 1285 + 1000 + 500);
        assert_eq!(t.totals().frames_received_ok, 3_300_002);
        // 10G octets are 64-bit
        let mut w = MacTotals::new();
        w.start(&raw(0xffff_fff0, 0));
        w.fold(&raw(0x1_0000_0010, 0), MacMode::TenGSerial);
        assert_eq!(w.totals().octets_received_ok, 0x20);
    }
}

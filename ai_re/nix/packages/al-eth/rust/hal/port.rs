// SPDX-License-Identifier: GPL-2.0

//! One Ethernet port as the Linux driver runs it: the hardware side of
//! al_eth_main.c (probe, up, down, Rx mode, MAC address, MTU, coalescing,
//! statistics, the Tx/Rx data path decisions) on top of the HAL modules.
//! Kernel objects (net_device, NAPI, skb, DMA mappings, IRQs) stay outside:
//! buffers are type parameters owned by the rings of [`crate::hal::ring`],
//! the caller does the kernel calls between the steps documented here.
//!
//! Split for locking: [`Port`] is the control state (probe/open/close/ndo
//! calls under RTNL); [`TxQueue`] and [`RxQueue`] are touched only by their
//! xmit path / NAPI context and need only shared access to the registers.

use core::ops::DerefMut;

use crate::hal::board_params::{BoardParams, MdioFreq, MediaType, PhyIf, Speed1g};
use crate::hal::desc::{self, RxPkt, TxMeta, TxPkt, MAX_BUFS};
use crate::hal::ec::{self, FwdCtrlEntry, FwdMacEntry};
use crate::hal::io::{Bars, CDescMem, Delay, DescMem, PciCfg, RegIo};
use crate::hal::iofic::{self, Group};
use crate::hal::mac::{self, LinkPause, MacMode, MacStats};
use crate::hal::regs::{k, off};
use crate::hal::ring::{RxRing, TxRing, TxSlot};
use crate::hal::udma::{Dir, Queue, State, Udma};
use crate::hal::{Error, Result};

pub const NUM_QUEUES: usize = 4;
/// Descriptors per ring (AL_ETH_DEFAULT_{TX,RX}_DESCS) and software slots.
pub const RING_SIZE: usize = 1024;
/// CONFIG_MAX_SKB_FRAGS of our kernels (the default, 17).
pub const MAX_SKB_FRAGS: u32 = 17;
/// Stop a Tx queue below this many free descriptors, wake it above
/// (AL_ETH_TX_WAKEUP_THRESH resolves to MAX_SKB_FRAGS + 2 for 1024 slots).
pub const TX_STOP_THRESH: u32 = MAX_SKB_FRAGS + 2;
/// Rx packets up to this long are copied, their buffer stays posted
/// (AL_ETH_DEFAULT_SMALL_PACKET_LEN, NET_IP_ALIGN = 0 on arm64).
pub const SMALL_COPY_LEN: u32 = 128;
pub const MIN_MTU: u32 = 68;
pub const MAX_MTU: u32 = 9216;
const MAC_TABLE_UC_BASE: usize = 0;
const MAC_TABLE_UC_MAX: usize = 4;
const MAC_TABLE_ALL_MC: usize = MAC_TABLE_UC_BASE + MAC_TABLE_UC_MAX;
const MAC_TABLE_DROP: usize = k::AL_ETH_FWD_MAC_NUM as usize - 1;
const MAC_TABLE_BCAST: usize = MAC_TABLE_DROP - 1;
const FLOW_CTRL_RX_PAUSE: u8 = 1 << 1;
const FLOW_CTRL_TX_PAUSE: u8 = 1 << 2;
/// UDMA mask of UDMA 0 (the integrated port uses only that one).
const UDMA0: u8 = 1;

/// Size of an Rx buffer for an MTU (al_eth_alloc_rx_frag's data_size).
pub const fn rx_buf_size(mtu: u32) -> u32 {
    let s = mtu + 14 + 4 + 4; // ETH_HLEN + FCS + VLAN
    if s > 1536 {
        1536
    } else if s < 600 {
        600
    } else {
        s
    }
}

/// A buffer the device reads or writes: its bus address and length.
pub trait DmaBuf {
    fn dma_addr(&self) -> u64;
    fn dma_len(&self) -> u32;
}

/// What al_eth_board_params_init takes from the board parameters
/// (ALPINE_INTEGRATED), for the media of our boards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoardCfg {
    pub mac_mode: MacMode,
    pub phy_exist: bool,
    pub phy_addr: u8,
    pub autoneg: bool,
    pub force_1000_base_x: bool,
    pub speed: u32,
    pub full_duplex: bool,
    pub clause_45: bool,
    pub mdio_khz: u32,
    pub ref_clk_freq: u32,
}

impl BoardCfg {
    pub fn from_params(p: &BoardParams) -> Result<Self> {
        let mac_mode = match p.media_type {
            MediaType::RGMII if p.sfp_plus_module_exist => MacMode::Sgmii,
            MediaType::RGMII => MacMode::Rgmii,
            MediaType::SGMII => MacMode::Sgmii,
            MediaType::TEN_GBASE_SR => MacMode::TenGSerial,
            // auto-detect, 2.5G, NBASE-T, 25G, 40G: link management, not on our boards
            _ => return Err(Error::Unsupported),
        };
        Ok(BoardCfg {
            mac_mode,
            phy_exist: p.phy_exist,
            phy_addr: p.phy_mdio_addr,
            autoneg: if p.phy_exist { p.an_in_band } else { !p.an_disable },
            force_1000_base_x: p.force_1000_base_x,
            speed: match p.speed {
                Speed1g::M100 => 100,
                Speed1g::M10 => 10,
                _ => 1000,
            },
            full_duplex: !p.half_duplex,
            clause_45: p.phy_if == PhyIf::XMDIO,
            mdio_khz: if p.mdio_freq == MdioFreq::MHZ_1 { 1000 } else { 2500 },
            ref_clk_freq: p.ref_clk_freq.0 as u32,
        })
    }

    /// AL_ETH_REF_CLK_FREQ_TO_HZ / 1000
    const fn sb_clk_khz(&self) -> u32 {
        match self.ref_clk_freq {
            k::AL_ETH_REF_FREQ_375_MHZ => 375_000,
            k::AL_ETH_REF_FREQ_187_5_MHZ => 187_500,
            k::AL_ETH_REF_FREQ_250_MHZ => 250_000,
            k::AL_ETH_REF_FREQ_428_MHZ => 428_000,
            _ => 500_000,
        }
    }

    /// AL_ETH_INTR_MODERATION_RESOLUTION_NSECS (resolution 1)
    const fn moder_res_ns(&self) -> u32 {
        (2 * 256 * 1_000_000) / self.sb_clk_khz()
    }
}

/// One level of the adaptive interrupt moderation table.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Moder {
    pub interval: u32,
    pub pkts: u32,
    pub bytes: u32,
}

/// Adaptive moderation state of an Rx queue (al_eth_ring fields).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RxModer {
    idx: usize,
    smoothed: u32,
    packets: u32,
    bytes: u32,
}

/// The control state of a port.
pub struct Port<R: RegIo> {
    pub bars: Bars<R>,
    pub cfg: BoardCfg,
    mac_addr: [u8; 6],
    tx_udma: Udma,
    rx_udma: Udma,
    reset_requested: bool,
    flow_ctrl_supported: u8,
    flow_ctrl_active: u8,
    tx_usecs: u32,
    rx_usecs: u32,
    pub adaptive: bool,
    modtab: [Moder; 5],
    rss: [u8; k::AL_ETH_RX_THASH_TABLE_SIZE as usize],
    mtu: u32,
    up: bool,
}

/// Board parameter registers in read order (al_eth_board_params_get).
const PARAM_REGS: [usize; 3] = [off::MAC_1G_SCRATCH, off::MAC_10G_SCRATCH, off::MAC_1G_MAC_0];

/// al_eth_board_params_get: none when register 1 is 0 (not read further).
fn read_params<R: RegIo>(mac: &R) -> Option<BoardParams> {
    let r1 = mac.r32(PARAM_REGS[0]);
    if r1 == 0 {
        return None;
    }
    let r2 = mac.r32(PARAM_REGS[1]);
    let r3 = mac.r32(PARAM_REGS[2]);
    BoardParams::decode([r1, r2, r3])
}

impl<R: RegIo> Port<R> {
    /// al_eth_probe, hardware part: board parameters, MAC address, a stop to
    /// clear what the boot loader left running, FLR, adapter init.
    /// `rev` is the PCI revision (4 on Alpine V3 runs as 2).
    pub fn probe<D: Delay, P: PciCfg>(bars: Bars<R>, delay: &D, pci: &P, rev: u8) -> Result<Self> {
        let mut p = Self::new(bars, rev)?;
        p.reset_init(delay, pci)?;
        Ok(p)
    }

    /// [`Port::probe`] in two steps, with the same register accesses in the
    /// same order: [`Port::new`] reads what the boot loader left (board
    /// parameters, MAC address, UDMA revisions) and builds the state; then
    /// [`Port::reset_init`] stops, resets and initialises the hardware. Lets
    /// the kernel side put the state on the heap before the hardware steps.
    pub fn new(bars: Bars<R>, rev: u8) -> Result<Self> {
        if rev as u32 != k::AL_ETH_REV_ID_2 {
            return Err(Error::Unsupported);
        }
        let params = read_params(&bars.mac).ok_or(Error::NoBoardParams)?;
        let cfg = BoardCfg::from_params(&params)?;
        let mac_addr = ec::mac_addr_read(&bars.ec, 0)?;
        let tx_udma = Udma::init(&bars.udma, Dir::Tx);
        let rx_udma = Udma::init(&bars.udma, Dir::Rx);
        let ns = cfg.moder_res_ns();
        let mut p = Port {
            bars,
            cfg,
            mac_addr,
            tx_udma,
            rx_udma,
            reset_requested: false,
            flow_ctrl_supported: 0,
            flow_ctrl_active: 0,
            tx_usecs: 0,
            rx_usecs: 0,
            adaptive: true,
            modtab: [
                Moder { interval: 0, pkts: 3, bytes: 2 * 1524 },
                Moder { interval: 1000 * 32 / ns, pkts: 12, bytes: 16 * 1024 },
                Moder { interval: 1000 * 80 / ns, pkts: 48, bytes: 64 * 1024 },
                Moder { interval: 1000 * 128 / ns, pkts: 96, bytes: 128 * 1024 },
                Moder { interval: 1000 * 192 / ns, pkts: 128, bytes: 192 * 1024 },
            ],
            rss: [0; k::AL_ETH_RX_THASH_TABLE_SIZE as usize],
            mtu: 1500,
            up: false,
        };
        for (i, r) in p.rss.iter_mut().enumerate() {
            *r = (i % NUM_QUEUES) as u8;
        }
        Ok(p)
    }

    /// The hardware half of [`Port::probe`]: hw_stop, function_reset, adapter_init.
    pub fn reset_init<D: Delay, P: PciCfg>(&mut self, delay: &D, pci: &P) -> Result {
        self.hw_stop(delay);
        self.function_reset(delay, pci)?;
        self.adapter_init(delay)
    }

    pub const fn mac_addr(&self) -> [u8; 6] {
        self.mac_addr
    }

    pub const fn mtu(&self) -> u32 {
        self.mtu
    }

    pub const fn is_up(&self) -> bool {
        self.up
    }

    /// al_eth_flow_ctrl_config
    fn flow_ctrl_config(&self) {
        let mut p = LinkPause {
            obey: self.flow_ctrl_active & FLOW_CTRL_RX_PAUSE != 0,
            generate: self.flow_ctrl_active & FLOW_CTRL_TX_PAUSE != 0,
            rx_fifo_th_high: 0x160,
            rx_fifo_th_low: 0x90,
            quanta: 0xffff,
            quanta_th: 0x8000,
            prio_q_map: [[0; 8]; 4],
        };
        for (i, m) in p.prio_q_map[0].iter_mut().enumerate() {
            *m = 1 << (i >> 1);
        }
        mac::link_pause_config(&self.bars.ec, &self.bars.mac, self.cfg.mac_mode, &p);
    }

    /// al_eth_hw_stop (UDMA 0)
    fn hw_stop<D: Delay>(&mut self, delay: &D) {
        let b = &self.bars;
        mac::stop(&b.mac, self.cfg.mac_mode);
        delay.udelay(100);
        for q in 0..NUM_QUEUES {
            let _ = self.tx_udma.queue_reset(&b.udma, delay, q);
        }
        for q in 0..NUM_QUEUES {
            let _ = self.rx_udma.queue_reset(&b.udma, delay, q);
        }
        // al_eth_adapter_stop: Rx is left alone when Tx does not stop
        if self.tx_udma.state_set_wait(&b.udma, delay, State::Disable).is_ok() {
            let _ = self.rx_udma.state_set_wait(&b.udma, delay, State::Disable);
        }
        self.reset_requested = true;
        self.flow_ctrl_active = 0;
        self.flow_ctrl_config();
    }

    /// al_eth_function_reset (UDMA 0): FLR with the board parameters and the
    /// MAC address saved and restored around it.
    pub fn function_reset<D: Delay, P: PciCfg>(&mut self, delay: &D, pci: &P) -> Result {
        let b = &self.bars;
        let params = read_params(&b.mac).ok_or(Error::NoBoardParams)?;
        let saved = params.encode().ok_or(Error::NoBoardParams)?;
        self.mac_addr = ec::mac_addr_read(&b.ec, 0)?;
        flr(&b.mac, delay, pci);
        for (o, v) in PARAM_REGS.iter().zip(saved) {
            b.mac.w32(*o, v);
        }
        ec::mac_addr_store(&b.ec, 0, &self.mac_addr)
    }

    /// al_eth_adapter_init for UDMA 0, revision 2.
    fn adapter_init<D: Delay>(&mut self, delay: &D) -> Result {
        let b = &self.bars;
        self.tx_udma = Udma::init(&b.udma, Dir::Tx);
        self.tx_udma.state_set_wait(&b.udma, delay, State::Normal)?;
        self.rx_udma = Udma::init(&b.udma, Dir::Rx);
        self.rx_udma.state_set_wait(&b.udma, delay, State::Normal)?;
        // the C ignores these return values
        let _ = self.tx_udma.m2s_packet_size_cfg_set(&b.udma, true, 0xfffff);
        let _ = self.tx_udma.max_descs_set(&b.udma, k::AL_ETH_PKT_MAX_BUFS as u8 + 1);
        let _ = self.rx_udma.max_descs_set(&b.udma, k::AL_ETH_PKT_MAX_BUFS as u8);
        let _ = self.rx_udma.s2m_compl_desc_burst_config(&b.udma, 64);
        ec::init(&b.ec);
        // al_eth_ec_error_ints_unmask
        let [a, bm, c, d] = ec::error_int_masks();
        let flags = k::INT_CONTROL_GRP_MASK_MSI_X | k::INT_CONTROL_GRP_CLEAR_ON_READ;
        for g in [Group::A, Group::B, Group::C, Group::D] {
            iofic::EC.config(&b.ec, g, flags);
        }
        iofic::UDMA_PRIMARY.config(&b.udma, Group::D, k::INT_CONTROL_GRP_SET_ON_POSEDGE | flags);
        for (g, m) in [(Group::A, a), (Group::B, bm), (Group::C, c), (Group::D, d)] {
            iofic::EC.clear_cause(&b.ec, g, m);
        }
        for (g, m) in [(Group::A, a), (Group::B, bm), (Group::C, c), (Group::D, d)] {
            iofic::EC.unmask(&b.ec, g, m);
        }
        iofic::UDMA_PRIMARY.unmask(&b.udma, Group::D, k::AL_INT_GROUP_D_APP_EXT_INT);
        Ok(())
    }

    /// al_eth_hw_init: adapter, MAC mode, link (fixed or SGMII), MDIO clock.
    fn hw_init<D: Delay>(&mut self, delay: &D) -> Result {
        self.adapter_init(delay)?;
        let c = self.cfg;
        let m = &self.bars.mac;
        mac::config(m, c.mac_mode);
        if c.mac_mode == MacMode::Sgmii || (c.mac_mode == MacMode::Rgmii && !c.phy_exist) {
            mac::link_config(m, c.mac_mode, c.force_1000_base_x, c.autoneg, c.speed, c.full_duplex)?;
        }
        mac::mdio_config(m, c.clause_45, c.ref_clk_freq, c.mdio_khz)?;
        self.flow_ctrl_supported = FLOW_CTRL_TX_PAUSE | FLOW_CTRL_RX_PAUSE;
        Ok(())
    }

    /// al_eth_set_coalesce: interrupt moderation intervals of all queues, in us.
    pub fn set_coalesce(&mut self, tx_usecs: u32, rx_usecs: u32) -> Result {
        let ns = self.cfg.moder_res_ns();
        let ti = 1000 * tx_usecs / ns;
        let ri = 1000 * rx_usecs / ns;
        if (self.tx_usecs != tx_usecs && ti > 255) || (self.rx_usecs != rx_usecs && ri > 255) {
            return Err(Error::Invalid); // a BUG_ON in C
        }
        let u = &self.bars.udma;
        if self.tx_usecs != tx_usecs {
            self.tx_usecs = ti * ns / 1000;
            for q in 0..NUM_QUEUES {
                iofic::UDMA_PRIMARY.msix_moder_interval_config(u, Group::C, q as u8, ti as u8);
            }
        }
        if self.rx_usecs != rx_usecs {
            self.rx_usecs = ri * ns / 1000;
            for q in 0..NUM_QUEUES {
                iofic::UDMA_PRIMARY.msix_moder_interval_config(u, Group::B, q as u8, ri as u8);
            }
        }
        Ok(())
    }

    fn mac_table(&self, idx: usize, addr: [u8; 6], mask: [u8; 6], udma_mask: u8, filter: bool) {
        mac_table(&self.bars.ec, idx, addr, mask, udma_mask, filter);
    }

    fn promiscuous(&self, on: bool) {
        promiscuous(&self.bars.ec, on);
    }

    /// al_eth_config_rx_fwd: priorities, default control entry, MAC filter,
    /// Toeplitz key (`key`: what get_random_bytes gave), RSS table, FSM.
    fn config_rx_fwd(&self, key: &[u32; 10]) {
        let e = &self.bars.ec;
        for i in 0..k::AL_ETH_FWD_PBITS_TABLE_NUM {
            let _ = ec::fwd_pbits_table_set(e, i, i as u8);
        }
        for i in 0..k::AL_ETH_FWD_PRIO_TABLE_NUM as u8 {
            let _ = ec::fwd_priority_table_set(e, i, i >> 1);
        }
        ec::ctrl_table_def_set(
            e,
            false,
            &FwdCtrlEntry {
                prio_sel: k::AL_ETH_CTRL_TABLE_PRIO_SEL_VAL_0,
                queue_sel_1: k::AL_ETH_CTRL_TABLE_QUEUE_SEL_1_THASH_TABLE,
                queue_sel_2: k::AL_ETH_CTRL_TABLE_QUEUE_SEL_2_NO_PRIO,
                udma_sel: k::AL_ETH_CTRL_TABLE_UDMA_SEL_MAC_TABLE,
                // uninitialised stack in al_eth_main.c; 0 here
                hdr_split_len_sel: k::AL_ETH_CTRL_TABLE_HDR_SPLIT_LEN_SEL_0,
                filter: false,
            },
        );
        self.mac_table(MAC_TABLE_UC_BASE, self.mac_addr, [0xff; 6], UDMA0, false);
        self.mac_table(MAC_TABLE_BCAST, [0xff; 6], [0xff; 6], UDMA0, false);
        self.promiscuous(false);
        for (i, w) in key.iter().enumerate() {
            let _ = ec::hash_key_set(e, i as u32, w.swap_bytes()); // htonl
        }
        for (i, q) in self.rss.iter().enumerate() {
            let _ = ec::thash_table_set(e, i as u32, 0, *q as u32);
        }
        // al_eth_fsm_table_init: 4-tuple hash for TCP/UDP, 2-tuple for other IP
        for i in 0..k::AL_ETH_RX_FSM_TABLE_SIZE {
            let v = match i & 7 {
                k::AL_ETH_FSM_ENTRY_IPV4_TCP | k::AL_ETH_FSM_ENTRY_IPV4_UDP | k::AL_ETH_FSM_ENTRY_IPV6_TCP | k::AL_ETH_FSM_ENTRY_IPV6_UDP => {
                    k::AL_ETH_FSM_DATA_OUTER_4_TUPLE | k::AL_ETH_FSM_DATA_HASH_SEL
                }
                k::AL_ETH_FSM_ENTRY_IPV6_NO_UDP_TCP | k::AL_ETH_FSM_ENTRY_IPV4_NO_UDP_TCP => {
                    k::AL_ETH_FSM_DATA_OUTER_2_TUPLE | k::AL_ETH_FSM_DATA_HASH_SEL
                }
                _ => (UDMA0 as u32) << k::AL_ETH_FSM_DATA_DEFAULT_UDMA_SHIFT,
            };
            let _ = ec::fsm_table_set(e, i, v);
        }
    }

    /// The programmed moderation intervals (tx, rx) in µs, as al_eth_get_coalesce
    /// reports them (rounded to the hardware's resolution).
    pub const fn coalesce(&self) -> (u32, u32) {
        (self.tx_usecs, self.rx_usecs)
    }

    /// The largest interval [Port::apply_coalesce] takes, in µs (255 units).
    pub const fn max_coalesce_usecs(&self) -> u32 {
        255 * self.cfg.moder_res_ns() / 1000
    }

    /// The interval register value (units of the moderation resolution) for
    /// usecs, None if it does not fit the 8-bit field. No overflow for any u32.
    pub fn coalesce_units(&self, usecs: u32) -> Option<u8> {
        let ns = u64::from(self.cfg.moder_res_ns());
        u8::try_from(1000 * u64::from(usecs) / ns).ok()
    }

    /// [Port::set_coalesce] for ethtool: the same interval writes, but to every
    /// queue unconditionally (the C's change detection compares against its
    /// cached value, which adaptive moderation does not update) and an
    /// out-of-range value refused before any write. Returns the (tx, rx)
    /// register values written.
    pub fn apply_coalesce(&mut self, tx_usecs: u32, rx_usecs: u32) -> Result<(u8, u8)> {
        let (Some(ti), Some(ri)) = (self.coalesce_units(tx_usecs), self.coalesce_units(rx_usecs)) else {
            return Err(Error::Invalid);
        };
        let u = &self.bars.udma;
        for q in 0..NUM_QUEUES {
            iofic::UDMA_PRIMARY.msix_moder_interval_config(u, Group::C, q as u8, ti);
        }
        for q in 0..NUM_QUEUES {
            iofic::UDMA_PRIMARY.msix_moder_interval_config(u, Group::B, q as u8, ri);
        }
        let ns = self.cfg.moder_res_ns();
        self.tx_usecs = u32::from(ti) * ns / 1000;
        self.rx_usecs = u32::from(ri) * ns / 1000;
        Ok((ti, ri))
    }

    /// al_eth_change_mtu
    pub fn change_mtu(&mut self, mtu: u32) -> Result {
        if !(MIN_MTU..=MAX_MTU).contains(&mtu) {
            return Err(Error::Invalid);
        }
        mac::rx_pkt_limit_config(&self.bars.ec, &self.bars.mac, self.cfg.mac_mode, k::AL_ETH_MIN_FRAME_LEN, mtu + 14 + 4 + 4)?;
        self.mtu = mtu;
        Ok(())
    }

    /// al_eth_up + al_eth_up_complete with MSI-X per queue (1 management +
    /// 4 Rx + 4 Tx vectors): the queues get fresh rings (`txq`/`rxq`
    /// with zeroed descriptor memory), Rx rings are filled from `alloc`.
    /// The caller requests the IRQs and enables NAPI before, starts the
    /// netdev Tx queues after.
    #[allow(clippy::too_many_arguments)]
    pub fn up<D, P, BT, ST, MT, BR, SR, MR, CR, A>(
        &mut self,
        delay: &D,
        pci: &P,
        key: &[u32; 10],
        mut txq: [&mut TxQueue<BT, ST, MT>; NUM_QUEUES],
        mut rxq: [&mut RxQueue<BR, SR, MR, CR>; NUM_QUEUES],
        mut alloc: A,
    ) -> Result
    where
        D: Delay,
        P: PciCfg,
        ST: DerefMut<Target = [TxSlot<BT>]>,
        MT: DescMem,
        BR: DmaBuf,
        SR: DerefMut<Target = [Option<BR>]>,
        MR: DescMem,
        CR: CDescMem,
        A: FnMut(usize) -> Option<BR>,
    {
        if self.up {
            return Err(Error::Invalid);
        }
        if self.reset_requested {
            self.function_reset(delay, pci)?;
            self.reset_requested = false;
        }
        if let Err(e) = self.hw_init(delay) {
            let _ = self.function_reset(delay, pci); // err_hw_init_open
            return Err(e);
        }
        for (i, t) in txq.iter_mut().enumerate() {
            t.reset(i)?;
        }
        for (i, r) in rxq.iter_mut().enumerate() {
            r.reset(i)?;
        }
        // al_eth_configure_int_mode
        let u = &self.bars.udma;
        iofic::udma_config(u, iofic::Mode::MsixPerQueue, 0x480, 0x480, 0x1e0, 0x1e0);
        iofic::UDMA_PRIMARY.moder_res_config(u, Group::B, 1);
        iofic::UDMA_PRIMARY.moder_res_config(u, Group::C, 1);
        if self.adaptive {
            self.set_coalesce(196, 0)?;
        }
        for r in rxq.iter_mut() {
            r.modtab = self.modtab;
            r.adaptive = self.adaptive;
            r.written = None; // set_coalesce wrote every queue's interval
        }
        self.config_rx_fwd(key);
        let _ = self.change_mtu(self.mtu);
        // al_eth_udma_queues_enable_all
        let u = &self.bars.udma;
        for t in txq.iter_mut() {
            t.q = Some(Queue::init(u, &mut self.tx_udma, t.qid, RING_SIZE as u32, t.mem.dma_addr(), None)?);
        }
        for r in rxq.iter_mut() {
            let q = Queue::init(u, &mut self.rx_udma, r.qid, RING_SIZE as u32, r.mem.dma_addr(), Some((r.cmem.dma_addr(), 16)))?;
            // al_eth_queue_config, Rx: completion coalescing on
            let b = q.base();
            let v = u.r32(b + off::Q_S2M_COMP_CFG);
            u.w32(b + off::Q_S2M_COMP_CFG, v & !k::UDMA_S2M_Q_COMP_CFG_DIS_COMP_COAL);
            u.w32(b + off::Q_S2M_COMP_CFG_2, k::AL_ETH_S2M_UDMA_COMP_COAL_TIMEOUT);
            r.q = Some(q);
        }
        for r in rxq.iter_mut() {
            let qid = r.qid;
            r.refill(&self.bars, RING_SIZE as u32 - 1, &mut || alloc(qid))?;
        }
        // al_eth_interrupts_unmask
        let p = iofic::UDMA_PRIMARY;
        p.unmask(u, Group::A, k::AL_INT_GROUP_A_GROUP_D_SUM);
        p.unmask(u, Group::B, (1 << NUM_QUEUES) - 1);
        p.unmask(u, Group::C, (1 << NUM_QUEUES) - 1);
        p.unmask(u, Group::D, 3 << 8);
        // al_eth_flow_ctrl_enable
        self.flow_ctrl_active = self.flow_ctrl_supported;
        self.flow_ctrl_config();
        // al_eth_restore_ethtool_params
        let (t, r) = (self.tx_usecs, self.rx_usecs);
        self.tx_usecs = 0;
        self.rx_usecs = 0;
        self.set_coalesce(t, r)?;
        for (i, q) in self.rss.iter().enumerate() {
            let _ = ec::thash_table_set(&self.bars.ec, i as u32, 0, *q as u32);
        }
        mac::start(&self.bars.mac, self.cfg.mac_mode);
        self.up = true;
        Ok(())
    }

    /// al_eth_down, hardware part: interrupts masked, MAC and DMA stopped,
    /// then every buffer still in a ring is handed to `free_tx` / `free_rx`
    /// (the device no longer touches them). Before: carrier off, IRQs
    /// synchronised, NAPI disabled, Tx stopped; after: free IRQs.
    pub fn down<D, BT, ST, MT, BR, SR, MR, CR>(
        &mut self,
        delay: &D,
        mut txq: [&mut TxQueue<BT, ST, MT>; NUM_QUEUES],
        mut rxq: [&mut RxQueue<BR, SR, MR, CR>; NUM_QUEUES],
        mut free_tx: impl FnMut(BT),
        mut free_rx: impl FnMut(BR),
    ) -> [(u32, u32); NUM_QUEUES]
    where
        D: Delay,
        ST: DerefMut<Target = [TxSlot<BT>]>,
        MT: DescMem,
        SR: DerefMut<Target = [Option<BR>]>,
        MR: DescMem,
        CR: CDescMem,
    {
        self.up = false;
        let u = &self.bars.udma;
        let p = iofic::UDMA_PRIMARY;
        p.mask(u, Group::A, 0x7);
        p.mask(u, Group::B, 0xf);
        p.mask(u, Group::C, 0xf);
        p.mask(u, Group::D, 0xffff_ffff);
        self.hw_stop(delay);
        let mut freed = [(0, 0); NUM_QUEUES];
        for (i, (t, r)) in txq.iter_mut().zip(rxq.iter_mut()).enumerate() {
            t.q = None;
            r.q = None;
            freed[i] = (t.ring.drain(&mut free_tx), r.ring.drain(&mut free_rx));
        }
        freed
    }

    /// al_eth_set_rx_mode. `uc`: the secondary unicast addresses. (The C
    /// programs the port's own address into each of those entries; this
    /// programs the addresses.)
    pub fn set_rx_mode(&self, promisc: bool, allmulti: bool, mc_count: usize, uc: &[[u8; 6]]) {
        rx_mode(&self.bars, promisc, allmulti, mc_count, uc);
    }

    /// al_eth_set_mac_addr (the caller checked is_valid_ether_addr).
    pub fn set_mac_addr(&mut self, a: [u8; 6]) {
        self.mac_addr = a;
        self.mac_table(MAC_TABLE_UC_BASE, a, [0xff; 6], UDMA0, false);
    }

    /// al_eth_mac_stats_get (al_eth_get_stats64 reads it only while up).
    pub fn stats(&self) -> Option<MacStats> {
        self.up.then(|| mac::stats(&self.bars.mac, self.cfg.mac_mode))
    }

    /// al_eth_link_status_get
    pub fn link_up(&self) -> bool {
        mac::link_up(&self.bars.mac, self.cfg.mac_mode)
    }

    /// al_eth_intr_msix_mgmt: the cause registers it reports (primary D,
    /// secondary A: M2S errors, secondary B: S2M errors).
    pub fn mgmt_causes(&self) -> [u32; 3] {
        let u = &self.bars.udma;
        [
            iofic::UDMA_PRIMARY.read_cause(u, Group::D),
            iofic::UDMA_SECONDARY.read_cause(u, Group::A),
            iofic::UDMA_SECONDARY.read_cause(u, Group::B),
        ]
    }

    /// The MAC mode / link programming on its own (tests, mode changes).
    pub fn mac_config(&self, mode: MacMode, force_1000_base_x: bool, an: bool, speed: u32, full_duplex: bool) -> Result {
        mac::config(&self.bars.mac, mode);
        mac::link_config(&self.bars.mac, mode, force_1000_base_x, an, speed, full_duplex)
    }
}

fn mac_table<R: RegIo>(e: &R, idx: usize, addr: [u8; 6], mask: [u8; 6], udma_mask: u8, filter: bool) {
    let m = FwdMacEntry { addr, mask, rx_valid: true, udma_mask, filter, ..Default::default() };
    let _ = ec::fwd_mac_table_set(e, idx, &m);
}

fn promiscuous<R: RegIo>(e: &R, on: bool) {
    mac_table(e, MAC_TABLE_DROP, [0; 6], [0; 6], if on { UDMA0 } else { 0 }, !on);
}

fn table_clear<R: RegIo>(e: &R, idx: usize) {
    let _ = ec::fwd_mac_table_set(e, idx, &FwdMacEntry::default());
}

/// [`Port::set_rx_mode`] without the port: it touches only the EC's MAC
/// table (entries other than the port's own address), so the kernel side can
/// run it from `ndo_set_rx_mode` (atomic context) while the control state is
/// locked elsewhere; the caller serialises it against `Port::up`/`down`.
pub fn rx_mode<R: RegIo>(bars: &Bars<R>, promisc: bool, allmulti: bool, mc_count: usize, uc: &[[u8; 6]]) {
    let e = &bars.ec;
    if promisc {
        promiscuous(e, true);
        return;
    }
    if allmulti || mc_count != 0 {
        mac_table(e, MAC_TABLE_ALL_MC, [1, 0, 0, 0, 0, 0], [1, 0, 0, 0, 0, 0], UDMA0, false);
    } else {
        table_clear(e, MAC_TABLE_ALL_MC);
    }
    if !uc.is_empty() {
        if uc.len() > MAC_TABLE_UC_MAX {
            promiscuous(e, true);
            return;
        }
        for i in MAC_TABLE_UC_BASE + 1..MAC_TABLE_UC_BASE + MAC_TABLE_UC_MAX {
            table_clear(e, i);
        }
        for (i, a) in uc.iter().enumerate() {
            mac_table(e, MAC_TABLE_UC_BASE + 1 + i, *a, [0xff; 6], UDMA0, false);
        }
    }
    promiscuous(e, false);
}

/// Most secondary unicast addresses the MAC table takes (more: promiscuous).
pub const UC_MAX: usize = MAC_TABLE_UC_MAX;

/// al_eth_flr_rmn
fn flr<R: RegIo, D: Delay, P: PciCfg>(mac: &R, delay: &D, pci: &P) {
    const CFG: [u16; 7] = [k::AL_PCI_COMMAND as u16, 0xc, 0x10, 0x18, 0x20, 0x110, k::AL_ADAPTER_SMCC_CONF_2 as u16];
    let gc0 = k::AL_ADAPTER_GENERIC_CONTROL_0 as u16;
    let mut reg = pci.read32(gc0);
    reg |= k::AL_ADAPTER_GENERIC_CONTROL_0_ETH_RESET_1GMAC;
    pci.write32(gc0, reg);
    delay.udelay(1000);
    reg &= !k::AL_ADAPTER_GENERIC_CONTROL_0_ETH_RESET_1GMAC;
    reg &= !k::AL_ADAPTER_GENERIC_CONTROL_0_ETH_RESET_1GMAC_ON_FLR;
    pci.write32(gc0, reg);
    let mux_sel = mac.r32(off::MAC_GEN_MUX_SEL);
    let saved = CFG.map(|o| pci.read32(o));
    pci.write32((k::AL_PCI_EXP_CAP_BASE + k::AL_PCI_EXP_DEVCTL) as u16, k::AL_PCI_EXP_DEVCTL_BCR_FLR);
    delay.udelay(1000);
    for (o, v) in CFG.iter().zip(saved) {
        pci.write32(*o, v);
    }
    crate::hal::io::masked(mac, off::MAC_GEN_MUX_SEL, k::ETH_MAC_GEN_MUX_SEL_KR_IN_MASK, mux_sel);
    mac.w32(off::MAC_SGMII_CLK_DIV, 0x0332_0501); // SGMII clock 125 MHz
    reg |= k::AL_ADAPTER_GENERIC_CONTROL_0_ETH_RESET_1GMAC;
    pci.write32(gc0, reg);
    delay.udelay(1000);
    reg &= !k::AL_ADAPTER_GENERIC_CONTROL_0_ETH_RESET_1GMAC;
    pci.write32(gc0, reg);
    mac.w32(off::MAC_SGMII_CLK_DIV, 0x0032_0501);
    delay.udelay(1000);
    let r = mac.r32(off::MAC_GEN_SD_FIFO_CTRL);
    mac.w32(off::MAC_GEN_SD_FIFO_CTRL, r | 0xf0);
    let r = mac.r32(off::MAC_GEN_SD_FIFO_CTRL);
    mac.w32(off::MAC_GEN_SD_FIFO_CTRL, r & !0xf0);
    pci.after_flr();
}

/// What al_eth_start_xmit / al_eth_tx_csum read from the skb.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TxInfo {
    pub len: u32,
    /// ip_summed == CHECKSUM_PARTIAL
    pub partial: bool,
    /// skb_shinfo(skb)->gso_size
    pub gso_size: u32,
    /// ip_hdr(skb)->version
    pub ip_version: u8,
    /// ip_hdr(skb)->protocol / ipv6_hdr(skb)->nexthdr
    pub l4proto: u8,
    pub nh_off: u32,
    pub nh_len: u32,
    pub transport_off: u32,
    /// tcp_hdr(skb)->doff (read whatever the L4 header is, as the C does)
    pub doff: u8,
    /// skb->protocol == ETH_P_8021Q
    pub vlan: bool,
}

/// The offload part of a Tx packet (al_eth_tx_csum's result).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TxOffload {
    pub flags: u32,
    pub meta: Option<TxMeta>,
    /// gso_size after al_eth_tx_tso_mss_fix (the C writes it back to the skb)
    pub gso_size: u32,
}

/// al_eth_tx_tso_mss_fix + al_eth_is_mss_valid with the C's integer types
/// (int lengths, u32 MSS, the result stored in the skb's unsigned short):
/// makes the last TSO segment at least 60 bytes. Returns the new gso_size.
fn tso_mss_fix(i: &TxInfo, orig: u32) -> Result<u32> {
    let header_len = i.transport_off.wrapping_add(i.doff as u32 * 4) as i32;
    let total = i.len.wrapping_sub(header_len as u32) as i32;
    let valid = |mss: u32| -> bool {
        if mss == 0 {
            return false; // a division by zero in C; unreachable (mss >= 18 here)
        }
        let res = (total as u32) % mss;
        res == 0 || (header_len as u32).wrapping_add(res) >= 60
    };
    if valid(orig) {
        return Ok(orig);
    }
    if orig < 64 {
        return Err(Error::Invalid);
    }
    let pad = 60 - header_len;
    let new = if orig >= 260 {
        if (total as u32) / orig < pad as u32 {
            orig.wrapping_sub(pad as u32)
        } else {
            orig - 1
        }
    } else {
        let mut n = orig;
        let mut k = 1;
        while k <= pad {
            if valid(orig.wrapping_sub(k as u32)) {
                n = orig.wrapping_sub(k as u32);
                break;
            }
            k += 1;
        }
        n
    };
    Ok(new as u16 as u32)
}

/// A Tx queue: hardware ring, software ring of in-flight buffers.
pub struct TxQueue<B, S: DerefMut<Target = [TxSlot<B>]>, M: DescMem> {
    pub qid: usize,
    q: Option<Queue>,
    mem: M,
    ring: TxRing<B, S>,
    /// tx_ring->hal_meta: shared by the queue's packets, never cleared
    meta: TxMeta,
}

/// What a sent packet did to its queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XmitResult {
    pub descs: u32,
    /// fewer than TX_STOP_THRESH descriptors left: stop the netdev queue
    pub stop: bool,
}

/// What a Tx completion pass found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TxClean {
    pub pkts: u32,
    pub bytes: u32,
    /// free descriptors after it (wake a stopped queue above TX_STOP_THRESH)
    pub available: u32,
}

impl<B, S: DerefMut<Target = [TxSlot<B>]>, M: DescMem> TxQueue<B, S, M> {
    pub fn new(qid: usize, slots: S, mem: M) -> Result<Self> {
        if qid >= NUM_QUEUES || slots.len() != RING_SIZE || mem.len() != RING_SIZE {
            return Err(Error::Invalid);
        }
        Ok(Self { qid, q: None, mem, ring: TxRing::new(slots)?, meta: TxMeta::default() })
    }

    /// Fresh rings for the next up (al_eth_setup_tx_resources); the caller
    /// may give new zeroed descriptor memory with `replace_mem` before.
    fn reset(&mut self, qid: usize) -> Result {
        if self.q.is_some() || qid != self.qid {
            return Err(Error::Invalid);
        }
        self.ring.reset()
    }

    /// Replaces the descriptor memory while the queue is down; hands back the old one.
    pub fn replace_mem(&mut self, mem: M) -> core::result::Result<M, M> {
        if self.q.is_some() || mem.len() != RING_SIZE {
            return Err(mem);
        }
        Ok(core::mem::replace(&mut self.mem, mem))
    }

    pub fn available(&self) -> u32 {
        self.q.as_ref().map_or(0, |q| q.available())
    }

    /// al_eth_tx_csum: the offload flags and meta data. It updates the
    /// fields the next packet of this slot inherits (as the C does) even when
    /// it rejects the packet (a TSO MSS that cannot be fixed: drop it).
    pub fn offload(&mut self, i: &TxInfo) -> Result<TxOffload> {
        let mut flags = k::AL_ETH_TX_FLAGS_INT;
        let mut gso = i.gso_size;
        if !(i.partial || gso != 0) {
            return Ok(TxOffload { flags, meta: None, gso_size: gso });
        }
        let st = self.ring.next_sticky();
        if gso != 0 {
            flags |= k::AL_ETH_TX_FLAGS_TSO | k::AL_ETH_TX_FLAGS_L4_CSUM;
        } else {
            flags |= k::AL_ETH_TX_FLAGS_L4_CSUM | k::AL_ETH_TX_FLAGS_L4_PARTIAL_CSUM;
        }
        let mut l4 = 0;
        match i.ip_version {
            4 => {
                st.l3_proto_idx = k::AL_ETH_PROTO_ID_IPv4;
                if gso != 0 {
                    flags |= k::AL_ETH_TX_FLAGS_IPV4_L3_CSUM;
                }
                l4 = i.l4proto;
            }
            6 => {
                st.l3_proto_idx = k::AL_ETH_PROTO_ID_IPv6;
                l4 = i.l4proto;
            }
            _ => {}
        }
        if l4 == 6 {
            st.l4_proto_idx = k::AL_ETH_PROTO_ID_TCP;
            if gso != 0 {
                gso = tso_mss_fix(i, gso)?;
            }
        } else if l4 == 17 {
            st.l4_proto_idx = k::AL_ETH_PROTO_ID_UDP;
        }
        st.source_vlan_count = i.vlan as u8;
        let meta = TxMeta {
            l3_header_len: i.nh_len as u8,
            l3_header_offset: i.nh_off.wrapping_sub(4 * i.vlan as u32) as u8,
            l4_header_len: i.doff,
            mss_val: (gso & 0x3fff) as u16,
        };
        self.meta = meta;
        Ok(TxOffload { flags, meta: Some(meta), gso_size: gso })
    }

    /// al_eth_start_xmit after the skb was mapped: `segs` are the bus
    /// address and length of the head and of each frag, `buf` owns the skb
    /// and its mappings until the completion gives it back. No room: the
    /// buffer comes back untouched (the caller drops it or retries).
    pub fn xmit<R: RegIo>(&mut self, bars: &Bars<R>, o: &TxOffload, segs: &[(u64, u32)], len: u32, buf: B) -> core::result::Result<XmitResult, B> {
        let Some(descs) = self.prepare(o, segs) else { return Err(buf) };
        self.hand_over(bars, descs, len, buf)
    }

    /// [`TxQueue::xmit`] for a buffer that knows its own mappings
    /// (`segs_of`), so the caller needs no copy of them, and that is handed to
    /// `reject` (not returned) when there is no room: the same descriptors,
    /// ring entries and doorbell. Not inlined (its by-value buffer stays out of
    /// the caller's frame: the kernel's xmit path is stack-sensitive).
    #[inline(never)]
    pub fn xmit_with<R: RegIo>(
        &mut self,
        bars: &Bars<R>,
        o: &TxOffload,
        len: u32,
        buf: B,
        segs_of: impl for<'x> Fn(&'x B) -> &'x [(u64, u32)],
        reject: impl FnOnce(B),
    ) -> Option<XmitResult> {
        let Some(descs) = self.prepare(o, segs_of(&buf)) else {
            reject(buf);
            return None;
        };
        match self.hand_over(bars, descs, len, buf) {
            Ok(r) => Some(r),
            Err(b) => {
                reject(b);
                None
            }
        }
    }

    /// The descriptors of a packet (none written: None).
    fn prepare(&mut self, o: &TxOffload, segs: &[(u64, u32)]) -> Option<u32> {
        let q = self.q.as_mut()?;
        if !self.ring.has_room() || segs.is_empty() || segs.len() > MAX_SKB_FRAGS as usize + 1 {
            return None;
        }
        let st = *self.ring.next_sticky();
        let pkt = TxPkt {
            flags: o.flags,
            l3_proto_idx: st.l3_proto_idx,
            l4_proto_idx: st.l4_proto_idx,
            source_vlan_count: st.source_vlan_count,
            meta: o.meta.map(|_| self.meta),
            bufs: segs,
        };
        match desc::tx_prepare(q, &mut self.mem, &pkt) {
            Ok(d) if d > 0 => Some(d),
            _ => None,
        }
    }

    /// The buffer into its slot, then the doorbell.
    fn hand_over<R: RegIo>(&mut self, bars: &Bars<R>, descs: u32, len: u32, buf: B) -> core::result::Result<XmitResult, B> {
        let Some(q) = self.q.as_mut() else { return Err(buf) };
        if let Err(b) = self.ring.push(buf, descs, len) {
            // cannot happen: room was checked; the descriptors are not yet
            // handed over, so no doorbell
            return Err(b);
        }
        let stop = q.available() < TX_STOP_THRESH;
        let _ = desc::tx_action(&bars.udma, q, &self.mem, descs);
        Ok(XmitResult { descs, stop })
    }

    /// al_eth_tx_poll before napi_complete: hands every fully completed
    /// packet's buffer to `free` (with its length).
    pub fn clean<R: RegIo>(&mut self, bars: &Bars<R>, mut free: impl FnMut(B, u32)) -> TxClean {
        let Some(q) = self.q.as_mut() else { return TxClean { pkts: 0, bytes: 0, available: 0 } };
        let mut done = desc::tx_completed(&bars.udma, q);
        let (mut pkts, mut bytes) = (0, 0);
        while let Some((b, len)) = self.ring.complete(&mut done) {
            free(b, len);
            pkts += 1;
            bytes += len;
        }
        TxClean { pkts, bytes, available: q.available() }
    }

    /// After napi_complete: unmask this queue's interrupt.
    pub fn unmask<R: RegIo>(&self, bars: &Bars<R>) {
        bars.udma.w32(iofic::UDMA_PRIMARY.unmask_offset(Group::C), !(1u32 << self.qid));
    }
}

/// A received packet handed to the stack.
pub enum RxEvent<'a, B> {
    /// Up to SMALL_COPY_LEN bytes: copy them out of `buf`, which stays in
    /// the ring (sync for the CPU before, for the device after).
    Small { buf: &'a mut B, info: RxInfo },
    /// The buffers now belong to the packet (frags).
    Frags { bufs: RxFrags<'a, B>, info: RxInfo },
    /// A packet with an Ethernet or DMA error: dropped, its buffers stay
    /// posted (the C drops it silently).
    Dropped { flags: u32, descs: u32 },
}

/// Checksum and hash of a received packet (al_eth_rx_checksum, RXHASH).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RxInfo {
    pub len: u32,
    /// CHECKSUM_UNNECESSARY
    pub csum_ok: bool,
    pub hash: u32,
    /// PKT_HASH_TYPE_L4 (else L3)
    pub l4_hash: bool,
    pub pkt: RxPkt,
}

pub use crate::hal::ring::RxFrags;

/// What an Rx poll did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RxPoll {
    pub work: u32,
    /// refill came short: reschedule NAPI
    pub resched: bool,
    /// all done within the budget: napi_complete, then `RxQueue::complete`
    pub complete: bool,
}

/// Refill came short (allocation failure) with at least this many buffers
/// still posted: complete anyway, and let the next packet's interrupt retry
/// the refill; below it keep polling (a starved ring raises no interrupt).
/// Bounds the softirq spinning under memory pressure (not in the C, whose
/// napi_schedule from inside its own poll is a no-op).
pub const RX_LOW_WATER: u32 = 128;

impl RxPoll {
    /// The NAPI decision after this poll: true = return the full budget and
    /// stay on the poll list (the C: return work_done == budget). Always so
    /// when the budget was used up (more packets may be waiting, the refill
    /// has run, nothing was completed); also when the refill came short and
    /// fewer than [RX_LOW_WATER] buffers are posted. Otherwise napi_complete
    /// + [RxQueue::complete].
    pub const fn repoll(&self, budget: u32, posted: u32) -> bool {
        self.work >= budget || (self.resched && posted < RX_LOW_WATER)
    }
}

/// An Rx queue's hardware view, for diagnostics (ethtool -S): raw values of
/// the S2M queue's descriptor ring head/tail pointers and packet counter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RxQueueRegs {
    pub head: u32,
    pub tail: u32,
    pub rx_pkt: u32,
}

/// Reads [RxQueueRegs] of queue qid (no side effects known; not in the C's datapath).
pub fn rx_queue_regs<R: RegIo>(udma: &R, qid: usize) -> RxQueueRegs {
    let b = off::U_S2M_Q0 + qid * off::U_S2M_Q_STRIDE;
    RxQueueRegs { head: udma.r32(b + off::Q_DRHP), tail: udma.r32(b + off::Q_DRTP), rx_pkt: udma.r32(b + off::Q_S2M_RX_PKT) }
}

/// The EC's receive-path counters (al_eth_ec_stats_get, al_eth_ec_stat_udma_get
/// for UDMA 0), in [EC_RX_STAT_NAMES] order: where a frame the MAC took can be
/// dropped before it reaches a queue. Raw 32-bit register values.
pub const EC_RX_STATS: [usize; 21] = [
    off::EC_STAT_faf_in_rx_pkt,
    off::EC_STAT_faf_out_drop,
    off::EC_STAT_rxf_in_rx_pkt,
    off::EC_STAT_rxf_in_fifo_err,
    off::EC_STAT_rxf_out_rx_1_pkt,
    off::EC_STAT_rxf_out_drop_1_pkt,
    off::EC_STAT_rfw_in_rx_pkt,
    off::EC_STAT_rfw_in_vlan_drop,
    off::EC_STAT_rfw_in_parse_drop,
    off::EC_STAT_rfw_in_mac_drop,
    off::EC_STAT_rfw_in_mac_ndet_drop,
    off::EC_STAT_rfw_in_ctrl_drop,
    off::EC_STAT_rfw_in_prot_i_drop,
    off::EC_STAT_UDMA0_rfw_out_rx_pkt,
    off::EC_STAT_UDMA0_rfw_out_drop,
    off::EC_STAT_UDMA0_msw_in_rx_pkt,
    off::EC_STAT_UDMA0_msw_drop_q_full,
    off::EC_STAT_UDMA0_msw_drop_sop,
    off::EC_STAT_UDMA0_msw_drop_eop,
    off::EC_STAT_UDMA0_msw_wr_eop,
    off::EC_STAT_UDMA0_msw_out_rx_pkt,
];
/// ethtool -S names of [EC_RX_STATS].
pub const EC_RX_STAT_NAMES: [&str; 21] = [
    "ec_faf_in_rx_pkt", "ec_faf_out_drop", "ec_rxf_in_rx_pkt", "ec_rxf_in_fifo_err", "ec_rxf_out_rx_1_pkt",
    "ec_rxf_out_drop_1_pkt", "ec_rfw_in_rx_pkt", "ec_rfw_in_vlan_drop", "ec_rfw_in_parse_drop", "ec_rfw_in_mac_drop",
    "ec_rfw_in_mac_ndet_drop", "ec_rfw_in_ctrl_drop", "ec_rfw_in_prot_i_drop", "ec_udma0_rfw_out_rx_pkt",
    "ec_udma0_rfw_out_drop", "ec_udma0_msw_in_rx_pkt", "ec_udma0_msw_drop_q_full", "ec_udma0_msw_drop_sop",
    "ec_udma0_msw_drop_eop", "ec_udma0_msw_wr_eop", "ec_udma0_msw_out_rx_pkt",
];

/// An Rx queue: hardware rings, posted buffers, adaptive moderation.
pub struct RxQueue<B, S: DerefMut<Target = [Option<B>]>, M: DescMem, C: CDescMem> {
    pub qid: usize,
    q: Option<Queue>,
    mem: M,
    cmem: C,
    ring: RxRing<B, S>,
    moder: RxModer,
    modtab: [Moder; 5],
    adaptive: bool,
    /// The interval while adaptive moderation is off (ethtool adaptive-rx off).
    fixed: Option<u8>,
    /// The interval this queue last wrote in [RxQueue::complete] (None: not
    /// known, someone else wrote it since).
    written: Option<u8>,
}

impl<B: DmaBuf, S: DerefMut<Target = [Option<B>]>, M: DescMem, C: CDescMem> RxQueue<B, S, M, C> {
    pub fn new(qid: usize, slots: S, mem: M, cmem: C) -> Result<Self> {
        if qid >= NUM_QUEUES || slots.len() != RING_SIZE || mem.len() != RING_SIZE || cmem.len() != RING_SIZE {
            return Err(Error::Invalid);
        }
        Ok(Self { qid, q: None, mem, cmem, ring: RxRing::new(slots)?, moder: RxModer::default(), modtab: [Moder::default(); 5], adaptive: false, fixed: None, written: None })
    }

    fn reset(&mut self, qid: usize) -> Result {
        if self.q.is_some() || qid != self.qid || self.ring.filled() != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Replaces both rings' memory while the queue is down.
    pub fn replace_mem(&mut self, mem: M, cmem: C) -> core::result::Result<(M, C), (M, C)> {
        if self.q.is_some() || mem.len() != RING_SIZE || cmem.len() != RING_SIZE {
            return Err((mem, cmem));
        }
        Ok((core::mem::replace(&mut self.mem, mem), core::mem::replace(&mut self.cmem, cmem)))
    }

    /// al_eth_refill_rx_bufs: post up to `num` buffers, one doorbell.
    fn refill<R: RegIo>(&mut self, bars: &Bars<R>, num: u32, alloc: &mut impl FnMut() -> Option<B>) -> Result<u32> {
        let q = self.q.as_mut().ok_or(Error::Invalid)?;
        let mem = &mut self.mem;
        let mut n = 0;
        while n < num {
            let mut post = |b: &B| desc::rx_buffer_add(q, mem, b.dma_addr(), b.dma_len(), k::AL_ETH_RX_FLAGS_INT);
            if !self.ring.refill_one(alloc, &mut post) {
                break;
            }
            n += 1;
        }
        if n != 0 {
            desc::rx_action(&bars.udma, q, mem, n)?;
        }
        Ok(n)
    }

    /// al_eth_rx_poll: up to `budget` packets to `deliver`, then the ring
    /// refilled from `alloc`. Error packets are dropped (their buffers stay).
    pub fn poll<R: RegIo>(&mut self, bars: &Bars<R>, budget: u32, mut deliver: impl FnMut(RxEvent<'_, B>), mut alloc: impl FnMut() -> Option<B>) -> Result<RxPoll> {
        let mut budget = budget;
        let mut work = 0;
        loop {
            let q = self.q.as_mut().ok_or(Error::Invalid)?;
            let Some(p) = desc::rx_packet(q, &self.cmem)? else { break };
            self.moder.packets = self.moder.packets.wrapping_add(1);
            if p.flags & (k::AL_ETH_RX_ERROR | k::AL_UDMA_CDESC_ERROR) == 0 {
                let l4 = p.l4_proto_idx == k::AL_ETH_PROTO_ID_TCP || p.l4_proto_idx == k::AL_ETH_PROTO_ID_UDP;
                let l3_bad = p.l3_proto_idx == k::AL_ETH_PROTO_ID_IPv4 && p.flags & k::AL_ETH_RX_FLAGS_L3_CSUM_ERR != 0;
                let mut info = RxInfo {
                    len: 0,
                    csum_ok: !l3_bad && l4 && p.flags & k::AL_ETH_RX_FLAGS_L4_CSUM_ERR == 0,
                    hash: ((p.rxhash as u32) << 16) ^ p.rxhash as u32,
                    l4_hash: l4,
                    pkt: p,
                };
                if p.lens[0] <= SMALL_COPY_LEN {
                    info.len = p.lens[0];
                    self.moder.bytes = self.moder.bytes.wrapping_add(p.lens[0]);
                    let buf = self.ring.head_mut().ok_or(Error::Hardware)?;
                    deliver(RxEvent::Small { buf, info });
                    self.ring.skip(1);
                } else {
                    // The packet's buffers, taken from the ring one by one by the
                    // consumer (the rest on drop): no array of them on the stack.
                    let n = (p.descs as usize).min(MAX_BUFS);
                    for &l in &p.lens[..n] {
                        info.len += l;
                        self.moder.bytes = self.moder.bytes.wrapping_add(l);
                    }
                    match self.ring.take_frags(n, &p.lens) {
                        Ok(bufs) => deliver(RxEvent::Frags { bufs, info }),
                        Err(taken) => {
                            // a buffer missing (impossible hardware): what the C-like
                            // walk had added before it stopped, then the error
                            let extra: u32 = p.lens[taken..n].iter().fold(0u32, |a, &l| a.wrapping_add(l));
                            self.moder.bytes = self.moder.bytes.wrapping_sub(extra);
                            return Err(Error::Hardware);
                        }
                    }
                }
            } else {
                self.ring.skip(p.descs as usize);
                deliver(RxEvent::Dropped { flags: p.flags, descs: p.descs });
            }
            budget = budget.saturating_sub(1);
            work += 1;
            if budget == 0 {
                break;
            }
        }
        let need = self.q.as_ref().map_or(0, |q| q.available());
        let got = self.refill(bars, need, &mut alloc)?;
        Ok(RxPoll { work, resched: got < need, complete: got >= need && budget > 0 })
    }

    /// Interrupt moderation of this queue (ethtool -C), taking effect at the
    /// next [RxQueue::complete]: None = adaptive (the C's table, as set by
    /// up), Some(units) = that fixed interval. A fixed interval is (re)written
    /// by complete whenever this queue's last write was another value, so an
    /// adaptive step that raced with ethtool switching it off is corrected at
    /// the next completion.
    pub fn set_moderation(&mut self, fixed: Option<u8>) {
        self.adaptive = fixed.is_none();
        self.fixed = fixed;
    }

    /// Descriptors posted to the device and not yet completed (0 while down).
    pub fn posted(&self) -> u32 {
        self.q.as_ref().map_or(0, |q| q.size() - 1 - q.available())
    }

    /// After napi_complete: adaptive moderation step, unmask the interrupt.
    pub fn complete<R: RegIo>(&mut self, bars: &Bars<R>) {
        if self.adaptive {
            self.update_moderation(bars);
        } else if let Some(u) = self.fixed {
            if self.written != Some(u) {
                iofic::UDMA_PRIMARY.msix_moder_interval_config(&bars.udma, Group::B, self.qid as u8, u);
                self.written = Some(u);
            }
            self.moder.packets = 0;
            self.moder.bytes = 0;
        }
        bars.udma.w32(iofic::UDMA_PRIMARY.unmask_offset(Group::B), !(1u32 << self.qid));
    }

    /// al_eth_update_intr_moderation, Rx
    fn update_moderation<R: RegIo>(&mut self, bars: &Bars<R>) {
        let m = &mut self.moder;
        if m.packets == 0 || m.bytes == 0 {
            return;
        }
        let t = &self.modtab;
        let cur = m.idx;
        let mut new = cur;
        if cur == 0 {
            if m.packets > t[0].pkts || m.bytes > t[0].bytes {
                new = 1;
            }
        } else if m.packets <= t[cur - 1].pkts || m.bytes <= t[cur - 1].bytes {
            new = cur - 1;
        } else if (m.packets > t[cur].pkts || m.bytes > t[cur].bytes) && cur != 4 {
            new = cur + 1;
        }
        m.packets = 0;
        m.bytes = 0;
        m.smoothed = (t[new].interval * 4 + 6 * m.smoothed + 5) / 10;
        m.idx = new;
        iofic::UDMA_PRIMARY.msix_moder_interval_config(&bars.udma, Group::B, self.qid as u8, m.smoothed as u8);
        self.written = None;
    }
}

#[cfg(test)]
#[path = "rx_model.rs"]
mod rx_model;

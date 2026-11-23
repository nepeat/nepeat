// SPDX-License-Identifier: GPL-2.0

//! The UDMA engine: per-direction state (al_hal_udma_main.c) and queues
//! (submission ring + completion ring bookkeeping, al_hal_udma.h inlines).

use crate::hal::io::{field_get, masked, CDescMem, DescMem, Delay, RegIo};
use crate::hal::regs::{k, off};
use crate::hal::{Error, Result};

/// M2S (memory to stream: Tx) or S2M (stream to memory: Rx).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Tx,
    Rx,
}

/// al_udma_state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Disable,
    Idle,
    Normal,
    Abort,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QStatus {
    NotInitialized,
    Disabled,
    Enabled,
}

pub const NUM_QUEUES: usize = k::DMA_MAX_Q as usize;

/// One direction of the UDMA (struct al_udma).
pub struct Udma {
    pub dir: Dir,
    /// Programming id from gen.dma_misc.revision.
    pub rev: u32,
    state: State,
    q_status: [QStatus; NUM_QUEUES],
}

const fn change_state_off(dir: Dir) -> usize {
    match dir {
        Dir::Tx => off::U_M2S_CHANGE_STATE,
        Dir::Rx => off::U_S2M_CHANGE_STATE,
    }
}

const fn state_off(dir: Dir) -> usize {
    match dir {
        Dir::Tx => off::U_M2S_STATE,
        Dir::Rx => off::U_S2M_STATE,
    }
}

/// Register offset of queue `qid` of a direction.
pub const fn queue_base(dir: Dir, qid: usize) -> usize {
    match dir {
        Dir::Tx => off::U_M2S_Q0 + qid * off::U_M2S_Q_STRIDE,
        Dir::Rx => off::U_S2M_Q0 + qid * off::U_S2M_Q_STRIDE,
    }
}

impl Udma {
    /// al_udma_init (4 queues) and al_udma_set_defaults.
    pub fn init<R: RegIo>(io: &R, dir: Dir) -> Self {
        let rev = field_get(
            io.r32(off::U_GEN_REVISION),
            k::UDMA_GEN_DMA_MISC_REVISION_PROGRAMMING_ID_MASK,
            k::UDMA_GEN_DMA_MISC_REVISION_PROGRAMMING_ID_SHIFT,
        );
        match dir {
            Dir::Tx => {
                if rev >= k::AL_UDMA_REV_ID_2 {
                    masked(io, off::U_M2S_RD_DATA_CFG, k::UDMA_M2S_RD_DATA_CFG_DATA_FIFO_DEPTH_MASK, 256 << k::UDMA_M2S_RD_DATA_CFG_DATA_FIFO_DEPTH_SHIFT);
                }
                io.w32(off::U_GEN_AXI_CFG_1, 1_000_000);
                io.w32(off::U_M2S_COMP_CFG_APP_ACK, 0);
            }
            Dir::Rx => io.w32(off::U_S2M_COMP_CFG_APP_ACK, 0),
        }
        Udma { dir, rev, state: State::Disable, q_status: [QStatus::NotInitialized; NUM_QUEUES] }
    }

    /// al_udma_state_set
    pub fn state_set<R: RegIo>(&mut self, io: &R, state: State) -> Result {
        let reg = match state {
            State::Disable => k::UDMA_M2S_CHANGE_STATE_DIS,
            State::Normal => k::UDMA_M2S_CHANGE_STATE_NORMAL,
            State::Abort => k::UDMA_M2S_CHANGE_STATE_ABORT,
            State::Idle => return Err(Error::Invalid),
        };
        io.w32(change_state_off(self.dir), reg);
        self.state = state;
        Ok(())
    }

    /// al_udma_state_get. A reserved state field is an al_assert in C.
    pub fn state_get<R: RegIo>(&self, io: &R) -> Result<State> {
        let r = io.r32(state_off(self.dir));
        let f = [
            field_get(r, k::UDMA_M2S_STATE_COMP_CTRL_MASK, k::UDMA_M2S_STATE_COMP_CTRL_SHIFT),
            field_get(r, k::UDMA_M2S_STATE_STREAM_IF_MASK, k::UDMA_M2S_STATE_STREAM_IF_SHIFT),
            field_get(r, k::UDMA_M2S_STATE_DATA_RD_CTRL_MASK, k::UDMA_M2S_STATE_DATA_RD_CTRL_SHIFT),
            field_get(r, k::UDMA_M2S_STATE_DESC_PREF_MASK, k::UDMA_M2S_STATE_DESC_PREF_SHIFT),
        ];
        if f.contains(&k::UDMA_STATE_RESERVED) {
            return Err(Error::Hardware);
        }
        if f.contains(&k::UDMA_STATE_ABORT) {
            return Ok(State::Abort);
        }
        if f.contains(&k::UDMA_STATE_NORMAL) {
            return Ok(State::Normal);
        }
        Ok(State::Idle)
    }

    /// al_udma_state_set_wait (al_hal_eth_main.c): NORMAL and DISABLE are
    /// reached when the engine reports idle. 1001 polls, 1 us apart.
    pub fn state_set_wait<R: RegIo, D: Delay>(&mut self, io: &R, delay: &D, state: State) -> Result {
        self.state_set(io, state)?;
        let expected = match state {
            State::Normal | State::Disable => State::Idle,
            s => s,
        };
        let mut count: u32 = 1000;
        loop {
            if self.state_get(io)? == expected {
                return Ok(());
            }
            delay.udelay(1);
            if count == 0 {
                return Err(Error::Timeout);
            }
            count -= 1;
        }
    }

    /// al_udma_m2s_packet_size_cfg_set
    pub fn m2s_packet_size_cfg_set<R: RegIo>(&self, io: &R, encode_64k_as_zero: bool, max_pkt_size: u32) -> Result {
        if self.dir != Dir::Tx {
            return Err(Error::Invalid);
        }
        let mut reg = io.r32(off::U_M2S_CFG_LEN);
        let mut max_supported = k::UDMA_M2S_CFG_LEN_MAX_PKT_SIZE_MASK;
        if encode_64k_as_zero {
            max_supported += 1;
        }
        if max_pkt_size > max_supported {
            return Err(Error::Invalid);
        }
        reg &= !k::UDMA_M2S_CFG_LEN_ENCODE_64K;
        if encode_64k_as_zero {
            reg |= k::UDMA_M2S_CFG_LEN_ENCODE_64K;
        }
        reg &= !k::UDMA_M2S_CFG_LEN_MAX_PKT_SIZE_MASK;
        reg |= max_pkt_size;
        io.w32(off::U_M2S_CFG_LEN, reg);
        Ok(())
    }

    /// al_udma_m2s_max_descs_set / al_udma_s2m_max_descs_set
    pub fn max_descs_set<R: RegIo>(&self, io: &R, max_descs: u8) -> Result {
        let allowed = match self.dir {
            Dir::Tx => k::AL_UDMA_M2S_MAX_ALLOWED_DESCS_PER_PACKET,
            Dir::Rx => k::AL_UDMA_S2M_MAX_ALLOWED_DESCS_PER_PACKET,
        };
        if max_descs as u32 > allowed || max_descs == 0 {
            return Err(Error::Invalid);
        }
        let mut pref_thr = max_descs as u32;
        let mut min_burst_above_thr = 4;
        if pref_thr >= 8 {
            min_burst_above_thr = 8;
        } else {
            pref_thr = 4;
        }
        match self.dir {
            Dir::Tx => {
                masked(
                    io,
                    off::U_M2S_RD_DESC_PREF_CFG_2,
                    k::UDMA_M2S_RD_DESC_PREF_CFG_2_MAX_DESC_PER_PKT_MASK,
                    (max_descs as u32) << k::UDMA_M2S_RD_DESC_PREF_CFG_2_MAX_DESC_PER_PKT_SHIFT,
                );
                masked(
                    io,
                    off::U_M2S_RD_DESC_PREF_CFG_3,
                    k::UDMA_M2S_RD_DESC_PREF_CFG_3_PREF_THR_MASK | k::UDMA_M2S_RD_DESC_PREF_CFG_3_MIN_BURST_ABOVE_THR_MASK,
                    (pref_thr << k::UDMA_M2S_RD_DESC_PREF_CFG_3_PREF_THR_SHIFT)
                        | (min_burst_above_thr << k::UDMA_M2S_RD_DESC_PREF_CFG_3_MIN_BURST_ABOVE_THR_SHIFT),
                );
            }
            Dir::Rx => masked(
                io,
                off::U_S2M_RD_DESC_PREF_CFG_3,
                k::UDMA_S2M_RD_DESC_PREF_CFG_3_PREF_THR_MASK | k::UDMA_S2M_RD_DESC_PREF_CFG_3_MIN_BURST_ABOVE_THR_MASK,
                (pref_thr << k::UDMA_S2M_RD_DESC_PREF_CFG_3_PREF_THR_SHIFT)
                    | (min_burst_above_thr << k::UDMA_S2M_RD_DESC_PREF_CFG_3_MIN_BURST_ABOVE_THR_SHIFT),
            ),
        }
        Ok(())
    }

    /// al_udma_s2m_compl_desc_burst_config (64, 128 or 256 bytes)
    pub fn s2m_compl_desc_burst_config<R: RegIo>(&self, io: &R, burst_size: u16) -> Result {
        if !matches!(burst_size, 64 | 128 | 256) {
            return Err(Error::Invalid);
        }
        let beats = (burst_size / 16) as u32;
        masked(
            io,
            off::U_S2M_AXI_DESC_WR_CFG_1,
            k::UDMA_AXI_S2M_DESC_WR_CFG_1_MIN_AXI_BEATS_MASK | k::UDMA_AXI_S2M_DESC_WR_CFG_1_MAX_AXI_BEATS_MASK,
            (beats << k::UDMA_AXI_S2M_DESC_WR_CFG_1_MIN_AXI_BEATS_SHIFT) | (beats << k::UDMA_AXI_S2M_DESC_WR_CFG_1_MAX_AXI_BEATS_SHIFT),
        );
        Ok(())
    }

    /// al_udma_q_enable(q, 0) + the scheduling/prefetch/completion drain and
    /// reset of al_udma_q_reset. Works on any of the 4 queues, initialised or
    /// not (the driver resets all of them at stop).
    pub fn queue_reset<R: RegIo, D: Delay>(&mut self, io: &R, delay: &D, qid: usize) -> Result {
        if qid >= NUM_QUEUES {
            return Err(Error::Invalid);
        }
        let b = queue_base(self.dir, qid);
        queue_enable(io, b, false);
        self.q_status[qid] = QStatus::Disabled;
        let mut remaining = k::AL_UDMA_Q_RST_TOUT;
        while remaining != 0 {
            let status = io.r32(b + off::Q_STATUS);
            if status & (k::UDMA_M2S_Q_STATUS_PREFETCH | k::UDMA_M2S_Q_STATUS_SCHEDULER) == 0 {
                break;
            }
            remaining -= 1;
            delay.udelay(1);
        }
        if remaining == 0 {
            return Err(Error::Timeout);
        }
        while remaining != 0 {
            let dcp = io.r32(b + off::Q_DCP);
            let crhp = io.r32(b + off::Q_CRHP);
            if dcp == crhp {
                break;
            }
            remaining -= 1;
            delay.udelay(1);
        }
        if remaining == 0 {
            return Err(Error::Timeout);
        }
        let sw_ctrl = match self.dir {
            Dir::Tx => off::Q_M2S_SW_CTRL,
            Dir::Rx => off::Q_S2M_SW_CTRL,
        };
        io.w32(b + sw_ctrl, k::UDMA_M2S_Q_SW_CTRL_RST_Q);
        Ok(())
    }
}

fn queue_enable<R: RegIo>(io: &R, base: usize, enable: bool) {
    let mut reg = io.r32(base + off::Q_CFG);
    if enable {
        reg |= k::UDMA_M2S_Q_CFG_EN_PREF | k::UDMA_M2S_Q_CFG_EN_SCHEDULING;
    } else {
        reg &= !(k::UDMA_M2S_Q_CFG_EN_PREF | k::UDMA_M2S_Q_CFG_EN_SCHEDULING);
    }
    io.w32(base + off::Q_CFG, reg);
}

/// One queue's ring bookkeeping (struct al_udma_q): indices into the
/// submission ring and the completion ring, and the ring ids (2-bit phase)
/// that tell a fresh completion descriptor from a stale one.
pub struct Queue {
    pub dir: Dir,
    pub qid: usize,
    base: usize,
    size_mask: u32,
    next_desc_idx: u32,
    next_cdesc_idx: u32,
    desc_ring_id: u32,
    comp_ring_id: u32,
    pkt_crnt_descs: u32,
    comp_head: u32,
}

impl Queue {
    /// al_udma_q_init: `size` descriptors (power of two, 4..65536) at
    /// `desc_dma`, completion ring at `cdesc_dma` of `cdesc_size`-byte
    /// entries or none (Tx: completions come from the head pointer).
    pub fn init<R: RegIo>(io: &R, udma: &mut Udma, qid: usize, size: u32, desc_dma: u64, cdesc: Option<(u64, u32)>) -> Result<Self> {
        if qid >= NUM_QUEUES || udma.q_status[qid] == QStatus::Enabled {
            return Err(Error::Invalid);
        }
        if !(k::AL_UDMA_MIN_Q_SIZE..=k::AL_UDMA_MAX_Q_SIZE).contains(&size) || !size.is_power_of_two() {
            return Err(Error::Invalid);
        }
        // al_assert()s of al_udma_q_set_pointers, checked before any write
        if (desc_dma as u32) & !k::UDMA_M2S_Q_TDRBP_LOW_ADDR_MASK != 0 {
            return Err(Error::Invalid);
        }
        if let Some((c, _)) = cdesc {
            if (c as u32) & !k::UDMA_M2S_Q_TCRBP_LOW_ADDR_MASK != 0 {
                return Err(Error::Invalid);
            }
        }
        let dir = udma.dir;
        let b = queue_base(dir, qid);
        // al_udma_q_config
        if dir == Dir::Tx {
            let mut v = io.r32(b + off::Q_M2S_RLIMIT_MASK);
            v &= !k::UDMA_M2S_Q_RATE_LIMIT_MASK_INTERNAL_PAUSE_DMB;
            io.w32(b + off::Q_M2S_RLIMIT_MASK, v);
        }
        // al_udma_q_set_pointers
        io.w32(b + off::Q_DRBP_LOW, desc_dma as u32);
        io.w32(b + off::Q_DRBP_HIGH, (desc_dma >> 32) as u32);
        io.w32(b + off::Q_DRL, size);
        if let Some((c, _)) = cdesc {
            io.w32(b + off::Q_CRBP_LOW, c as u32);
            io.w32(b + off::Q_CRBP_HIGH, (c >> 32) as u32);
        }
        // al_udma_q_config_compl
        let comp_cfg = b + if dir == Dir::Tx { off::Q_M2S_COMP_CFG } else { off::Q_S2M_COMP_CFG };
        let mut v = io.r32(comp_cfg);
        if cdesc.is_none() {
            v &= !k::UDMA_M2S_Q_COMP_CFG_EN_COMP_RING_UPDATE;
        } else {
            v |= k::UDMA_M2S_Q_COMP_CFG_EN_COMP_RING_UPDATE;
        }
        v |= k::UDMA_M2S_Q_COMP_CFG_DIS_COMP_COAL;
        io.w32(comp_cfg, v);
        if dir == Dir::Rx {
            // the C programs the size of a NULL ring too (cdesc_size from q_params)
            let cdesc_size = cdesc.map_or(0, |(_, s)| s);
            let mut v = io.r32(off::U_S2M_COMP_CFG_1C);
            v &= !k::UDMA_S2M_COMP_CFG_1C_DESC_SIZE_MASK;
            v |= (cdesc_size >> 2) & k::UDMA_S2M_COMP_CFG_1C_DESC_SIZE_MASK;
            io.w32(off::U_S2M_COMP_CFG_1C, v);
        }
        queue_enable(io, b, true);
        udma.q_status[qid] = QStatus::Enabled;
        Ok(Queue {
            dir,
            qid,
            base: b,
            size_mask: size - 1,
            next_desc_idx: 0,
            next_cdesc_idx: 0,
            desc_ring_id: k::AL_UDMA_INITIAL_RING_ID,
            comp_ring_id: k::AL_UDMA_INITIAL_RING_ID,
            pkt_crnt_descs: 0,
            comp_head: 0,
        })
    }

    pub const fn base(&self) -> usize {
        self.base
    }

    pub const fn size(&self) -> u32 {
        self.size_mask + 1
    }

    /// Index of the next descriptor the driver will fill.
    pub const fn next_desc_idx(&self) -> u32 {
        self.next_desc_idx
    }

    /// al_udma_available_get: free descriptors, one kept empty.
    pub const fn available(&self) -> u32 {
        (self.next_cdesc_idx.wrapping_sub(self.next_desc_idx.wrapping_add(1)) & 0xffff) & self.size_mask
    }

    /// al_udma_desc_get + al_udma_ring_id_get (always called as a pair):
    /// the index of the next free descriptor and the ring id to put into it.
    pub(crate) fn desc_take(&mut self) -> (usize, u32) {
        let idx = self.next_desc_idx;
        self.next_desc_idx = (idx + 1) & self.size_mask;
        let ring_id = self.desc_ring_id;
        if self.next_desc_idx == 0 {
            self.desc_ring_id = (self.desc_ring_id + 1) & k::DMA_RING_ID_MASK;
        }
        (idx as usize, ring_id)
    }

    /// al_udma_desc_action_add: make the descriptors visible, then the
    /// doorbell (tail pointer increment).
    pub fn action_add<R: RegIo, M: DescMem>(&self, io: &R, mem: &M, num: u32) -> Result {
        if num == 0 || num > self.size() {
            return Err(Error::Invalid);
        }
        mem.publish();
        io.w32(self.base + off::Q_DRTP_INC, num);
        Ok(())
    }

    /// al_udma_cdesc_get_all (completions from the head pointer register).
    pub fn cdesc_get_all<R: RegIo>(&mut self, io: &R) -> u32 {
        let head = io.r32(self.base + off::Q_CRHP) & 0xffff;
        head.wrapping_sub(self.next_cdesc_idx) & 0xffff & self.size_mask
    }

    /// al_udma_cdesc_ack
    pub fn cdesc_ack(&mut self, num: u32) {
        self.next_cdesc_idx = self.next_cdesc_idx.wrapping_add(num) & self.size_mask;
    }

    const fn new_cdesc(&self, flags: u32) -> bool {
        (flags & k::AL_M2S_DESC_RING_ID_MASK) >> k::AL_M2S_DESC_RING_ID_SHIFT == self.comp_ring_id
    }

    /// al_cdesc_next_update
    fn cdesc_next_update(&mut self, cur: u32) -> u32 {
        if cur == self.size_mask {
            self.comp_ring_id = (self.comp_ring_id + 1) & k::DMA_RING_ID_MASK;
            0
        } else {
            cur + 1
        }
    }

    /// al_udma_cdesc_packet_get: the descriptors of the next complete packet
    /// in the completion ring, as (first index, count); None while the
    /// device has not finished one. A partial packet is remembered.
    pub fn cdesc_packet_get<C: CDescMem>(&mut self, cmem: &C) -> Result<Option<(usize, u32)>> {
        if cmem.len() != self.size() as usize {
            return Err(Error::Invalid);
        }
        let mut cur = self.comp_head;
        let mut flags = cmem.ctrl(cur as usize);
        if !self.new_cdesc(flags) {
            return Ok(None);
        }
        let mut count = self.pkt_crnt_descs + 1;
        while flags & k::AL_UDMA_CDESC_LAST == 0 {
            cur = self.cdesc_next_update(cur);
            flags = cmem.ctrl(cur as usize);
            if !self.new_cdesc(flags) {
                self.pkt_crnt_descs = count;
                self.comp_head = cur;
                return Ok(None);
            }
            count += 1;
            if count > self.size() {
                return Err(Error::Hardware);
            }
        }
        let first = self.next_cdesc_idx as usize;
        self.pkt_crnt_descs = 0;
        self.comp_head = self.cdesc_next_update(cur);
        Ok(Some((first, count)))
    }
}

// SPDX-License-Identifier: GPL-2.0

//! Streaming DMA for packet buffers (`include/linux/dma-mapping.h`), RX page
//! fragments and descriptor rings in coherent memory.
//!
//! A mapped buffer is a value that owns both the buffer and its mapping:
//! - [RxBuf]: an [RxFrag] mapped DMA_FROM_DEVICE. The CPU reads its bytes only
//!   through [RxBuf::sync_for_cpu] (a guard that hands the bytes back to the
//!   device when dropped), or after [RxBuf::unmap], which consumes it and
//!   returns the fragment.
//! - [TxMapped]: an [SkBuff] whose head and fragments are mapped DMA_TO_DEVICE,
//!   with no access to the skb at all until [TxMapped::unmap].
//!
//! Both unmap in Drop, so safe code can neither leak a mapping nor touch a buffer
//! that is still mapped. The device's own DMA is outside what types can see: a
//! buffer handed to hardware may only be dropped once the hardware is done with
//! it or stopped (knet/mod.rs).

use super::napi::Bh;
use super::skb::{SkBuff, TxMeta};
use core::{mem::ManuallyDrop, ptr::NonNull};
use kernel::{
    bindings,
    device::{Bound, Device},
    dma::{Coherent, DmaAddress},
    prelude::*,
};

fn raw_dev(dev: &Device<Bound>) -> *mut bindings::device {
    // device::Device is a transparent wrapper of struct device.
    core::ptr::from_ref(dev).cast::<bindings::device>().cast_mut()
}

const fn data_align(x: u32) -> u32 {
    let a = bindings::SMP_CACHE_BYTES;
    (x + a - 1) & !(a - 1)
}

/// dma_rmb(): orders reads of descriptor memory (after seeing a completion's
/// ownership/phase bit, before reading its other words).
pub(crate) fn dma_rmb() {
    // SAFETY: a barrier; no memory is accessed.
    unsafe { bindings::dma_rmb() }
}

/// dma_wmb(): orders writes to descriptor memory (all words before the one that
/// hands the descriptor to the device).
pub(crate) fn dma_wmb() {
    // SAFETY: a barrier; no memory is accessed.
    unsafe { bindings::dma_wmb() }
}

/// An RX buffer: one page fragment from netdev_alloc_frag, laid out so that it
/// can become an skb head (build_skb): headroom, len data bytes, then room for
/// struct skb_shared_info.
///
/// # Invariants
///
/// ptr is the start of a fragment of frag_size bytes from netdev_alloc_frag
/// that this value owns (one page reference), and the layout above fits in it.
pub(crate) struct RxFrag {
    ptr: NonNull<u8>,
    frag_size: u32,
    headroom: u32,
    len: u32,
}

// SAFETY: a page fragment can be freed from any context or CPU.
unsafe impl Send for RxFrag {}

impl RxFrag {
    /// Size of a fragment with len data bytes after headroom.
    fn frag_size(len: u32, headroom: u32) -> Result<u32> {
        let shinfo = core::mem::size_of::<bindings::skb_shared_info>() as u32;
        let frag_size = data_align(headroom.checked_add(len).ok_or(EINVAL)?) + data_align(shinfo);
        if frag_size as usize > kernel::page::PAGE_SIZE {
            // build_skb on a page fragment needs it to fit a page.
            return Err(EINVAL);
        }
        Ok(frag_size)
    }

    /// Allocates a fragment with len data bytes after headroom (any context).
    pub(crate) fn alloc(len: u32, headroom: u32) -> Result<Self> {
        let frag_size = Self::frag_size(len, headroom)?;
        // SAFETY: plain allocation, result checked.
        let p = unsafe { bindings::netdev_alloc_frag(frag_size) }.cast::<u8>();
        let ptr = NonNull::new(p).ok_or(ENOMEM)?;
        Ok(Self { ptr, frag_size, headroom, len })
    }

    /// [RxFrag::alloc] from a NAPI poll (napi_alloc_frag: this CPU's NAPI
    /// fragment cache, without the local_bh_disable/enable pair
    /// netdev_alloc_frag wraps around it outside hard IRQ context).
    pub(crate) fn alloc_bh(len: u32, headroom: u32, _bh: Bh<'_>) -> Result<Self> {
        let frag_size = Self::frag_size(len, headroom)?;
        // SAFETY: plain allocation, result checked; BH is disabled and we are
        // not in hard IRQ context (what Bh proves), as napi_alloc_frag requires.
        let p = unsafe { bindings::__napi_alloc_frag_align(frag_size, !0) }.cast::<u8>();
        let ptr = NonNull::new(p).ok_or(ENOMEM)?;
        Ok(Self { ptr, frag_size, headroom, len })
    }

    /// Data bytes (excluding headroom and the shared-info room).
    pub(crate) fn size(&self) -> u32 {
        self.len
    }

    /// (head page, offset of the data area in it, truesize) for skb_add_rx_frag.
    pub(super) fn page_parts(&self) -> (*mut bindings::page, u32, u32) {
        // SAFETY: ptr points into a live fragment (invariant), in the kernel's
        // linear map, where virt_to_head_page and page_address are valid.
        let page = unsafe { bindings::virt_to_head_page(self.ptr.as_ptr().cast()) };
        // SAFETY: as above.
        let base = unsafe { bindings::page_address(page) } as usize;
        let off = (self.ptr.as_ptr() as usize - base) as u32 + self.headroom;
        (page, off, self.frag_size)
    }

    /// Gives up the page reference (to an skb that took it).
    pub(super) fn into_raw(self) -> *mut u8 {
        ManuallyDrop::new(self).ptr.as_ptr()
    }

    /// Turns the fragment into an skb whose linear part is its first len data
    /// bytes (build_skb, skb_reserve, skb_put). The fragment comes back if len is
    /// too large or the skb head cannot be allocated.
    pub(crate) fn into_skb(self, len: u32) -> core::result::Result<SkBuff, Self> {
        if len > self.len {
            return Err(self);
        }
        // SAFETY: ptr is a page fragment of frag_size bytes with room for the
        // shared info at its end (layout of alloc), as build_skb requires.
        let raw = unsafe { bindings::build_skb(self.ptr.as_ptr().cast(), self.frag_size) };
        if raw.is_null() {
            return Err(self);
        }
        let headroom = self.headroom;
        // The skb owns the fragment's page reference now.
        self.into_raw();
        // SAFETY: build_skb returned a new skb that we own.
        let mut skb = unsafe { SkBuff::from_raw(raw) };
        // The room was laid out by alloc, so these cannot fail; if they did, the
        // skb (and the fragment with it) would be dropped by the caller.
        if skb.reserve(headroom).is_err() || skb.put(len).is_err() {
            pr_warn!("knet: rx skb layout mismatch\n");
        }
        Ok(skb)
    }
}

impl Drop for RxFrag {
    fn drop(&mut self) {
        // SAFETY: we own the fragment (invariant).
        unsafe { bindings::skb_free_frag(self.ptr.as_ptr().cast()) }
    }
}

/// An [RxFrag]'s data area mapped for the device to write (DMA_FROM_DEVICE).
///
/// # Invariants
///
/// addr is a live mapping by dev of frag's data area (frag.size() bytes), made
/// by dma_map_single and not yet unmapped; while it exists the CPU does not touch
/// those bytes except inside an [RxSync] window.
pub(crate) struct RxBuf<'d> {
    frag: ManuallyDrop<RxFrag>,
    addr: DmaAddress,
    dev: &'d Device<Bound>,
}

impl<'d> RxBuf<'d> {
    /// Maps frag for dev to write into. Gives it back on a mapping error.
    pub(crate) fn map(dev: &'d Device<Bound>, frag: RxFrag) -> core::result::Result<Self, (Error, RxFrag)> {
        // SAFETY: the data area is len bytes inside the live fragment, in the
        // linear map (not vmalloc); dev is a bound device. Checked below.
        let addr = unsafe {
            bindings::dma_map_single_attrs(
                raw_dev(dev),
                frag.ptr.as_ptr().add(frag.headroom as usize).cast(),
                frag.len as usize,
                bindings::dma_data_direction::DMA_FROM_DEVICE,
                0,
            )
        };
        // SAFETY: checks the handle just returned for dev.
        if unsafe { bindings::dma_mapping_error(raw_dev(dev), addr) } != 0 {
            return Err((EIO, frag));
        }
        Ok(Self { frag: ManuallyDrop::new(frag), addr, dev })
    }

    /// The bus address for the descriptor.
    pub(crate) fn dma_addr(&self) -> DmaAddress {
        self.addr
    }

    /// Mapped length.
    pub(crate) fn len(&self) -> u32 {
        self.frag.len
    }

    /// Gives the first len bytes to the CPU (dma_sync_single_for_cpu) for as long
    /// as the guard lives, then back to the device: the copy-small-packets path,
    /// which keeps the buffer mapped and on the ring.
    pub(crate) fn sync_for_cpu(&mut self, len: u32) -> RxSync<'_, 'd> {
        let len = len.min(self.frag.len);
        // SAFETY: addr is a live mapping by dev (invariant), len within it.
        unsafe {
            bindings::dma_sync_single_for_cpu(
                raw_dev(self.dev),
                self.addr,
                len as usize,
                bindings::dma_data_direction::DMA_FROM_DEVICE,
            )
        };
        RxSync { buf: self, len }
    }

    /// Unmaps and returns the fragment, now the CPU's.
    pub(crate) fn unmap(self) -> RxFrag {
        let mut me = ManuallyDrop::new(self);
        me.unmap_in_place();
        // SAFETY: me is never used or dropped again: the fragment moves out once.
        unsafe { ManuallyDrop::take(&mut me.frag) }
    }

    fn unmap_in_place(&mut self) {
        // SAFETY: live mapping (invariant); called once, by unmap or drop.
        unsafe {
            bindings::dma_unmap_single_attrs(
                raw_dev(self.dev),
                self.addr,
                self.frag.len as usize,
                bindings::dma_data_direction::DMA_FROM_DEVICE,
                0,
            )
        }
    }
}

impl Drop for RxBuf<'_> {
    fn drop(&mut self) {
        self.unmap_in_place();
        // SAFETY: dropped once, here, after the mapping is gone.
        unsafe { ManuallyDrop::drop(&mut self.frag) }
    }
}

/// CPU access window to a mapped RX buffer (see [RxBuf::sync_for_cpu]).
pub(crate) struct RxSync<'b, 'd> {
    buf: &'b mut RxBuf<'d>,
    len: u32,
}

impl RxSync<'_, '_> {
    /// The synced bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: the first len data bytes were synced for the CPU, and the device
        // does not own them until this guard drops (the driver holds &mut RxBuf,
        // so it cannot hand the descriptor back meanwhile).
        unsafe {
            core::slice::from_raw_parts(
                self.buf.frag.ptr.as_ptr().add(self.buf.frag.headroom as usize),
                self.len as usize,
            )
        }
    }
}

impl Drop for RxSync<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: live mapping, len within it: hands the bytes back to the device.
        unsafe {
            bindings::dma_sync_single_for_device(
                raw_dev(self.buf.dev),
                self.buf.addr,
                self.len as usize,
                bindings::dma_data_direction::DMA_FROM_DEVICE,
            )
        }
    }
}

/// One mapped piece of a TX packet: (bus address, length), in the form a
/// descriptor writer takes directly (no copy of the list per packet).
pub(crate) type Seg = (DmaAddress, u32);

const MAX_SEGS: usize = 1 + bindings::MAX_SKB_FRAGS as usize;
const _: () = assert!(MAX_SEGS <= 32); // the page bitmask

/// An skb mapped for the device to read: its head (if any) and every fragment.
///
/// # Invariants
///
/// segs[..n] are live mappings by dev of the skb's head and fragments, in order
/// (bit i of pages set: segment i was mapped with dma_map_page, else
/// dma_map_single); the skb is not reachable while they exist.
pub(crate) struct TxMapped<'d> {
    skb: ManuallyDrop<SkBuff>,
    meta: TxMeta,
    segs: [Seg; MAX_SEGS],
    pages: u32,
    n: usize,
    dev: &'d Device<Bound>,
}

impl<'d> TxMapped<'d> {
    /// Maps skb for dev. Gives it back on error, with every partial mapping undone.
    /// Not inlined: its working copy stays out of the caller's (xmit) frame.
    #[inline(never)]
    pub(crate) fn map(dev: &'d Device<Bound>, skb: SkBuff) -> core::result::Result<Self, (Error, SkBuff)> {
        let meta = skb.tx_meta();
        let mut me = Self {
            skb: ManuallyDrop::new(skb),
            meta,
            segs: [(0, 0); MAX_SEGS],
            pages: 0,
            n: 0,
            dev,
        };
        let head = me.skb.headlen();
        if head > 0 {
            // SAFETY: data .. data + headlen is the skb's linear area, in the linear
            // map; dev is bound. Checked in push.
            let addr = unsafe {
                bindings::dma_map_single_attrs(
                    raw_dev(dev),
                    (*me.skb.as_raw()).data.cast(),
                    head as usize,
                    bindings::dma_data_direction::DMA_TO_DEVICE,
                    0,
                )
            };
            if !me.push(addr, head, false) {
                return Err((EIO, me.into_skb()));
            }
        }
        for i in 0..me.skb.nr_frags() {
            let Some((page, off, len)) = me.skb.frag(i) else { break };
            // SAFETY: page/off/len describe fragment i of the skb we hold.
            let addr = unsafe {
                bindings::dma_map_page_attrs(
                    raw_dev(dev),
                    page,
                    off as usize,
                    len as usize,
                    bindings::dma_data_direction::DMA_TO_DEVICE,
                    0,
                )
            };
            if !me.push(addr, len, true) {
                return Err((EIO, me.into_skb()));
            }
        }
        Ok(me)
    }

    /// Records a mapping; false (nothing kept) on a mapping error or overflow.
    fn push(&mut self, addr: DmaAddress, len: u32, page: bool) -> bool {
        // SAFETY: checks a handle just returned for dev.
        if unsafe { bindings::dma_mapping_error(raw_dev(self.dev), addr) } != 0 {
            return false;
        }
        match self.segs.get_mut(self.n) {
            Some(slot) => {
                *slot = (addr, len);
                if page {
                    self.pages |= 1 << self.n;
                }
                self.n += 1;
                true
            }
            None => {
                // Cannot happen (nr_frags <= MAX_SKB_FRAGS); undo this one mapping.
                self.unmap_seg((addr, len), page);
                false
            }
        }
    }

    fn unmap_seg(&self, (addr, len): Seg, page: bool) {
        let dir = bindings::dma_data_direction::DMA_TO_DEVICE;
        // SAFETY: (addr, len) is a live mapping by dev, made by map with this kind;
        // each is unmapped exactly once (unmap_all clears the count).
        unsafe {
            if page {
                bindings::dma_unmap_page_attrs(raw_dev(self.dev), addr, len as usize, dir, 0)
            } else {
                bindings::dma_unmap_single_attrs(raw_dev(self.dev), addr, len as usize, dir, 0)
            }
        }
    }

    fn unmap_all(&mut self) {
        for i in 0..self.n {
            self.unmap_seg(self.segs[i], self.pages & (1 << i) != 0);
        }
        self.n = 0;
        self.pages = 0;
    }

    fn into_skb(self) -> SkBuff {
        let mut me = ManuallyDrop::new(self);
        me.unmap_all();
        // SAFETY: me is not used or dropped again: the skb moves out once.
        unsafe { ManuallyDrop::take(&mut me.skb) }
    }

    /// The mapped pieces in packet order (for the descriptors): (address, length).
    pub(crate) fn segs(&self) -> &[Seg] {
        &self.segs[..self.n]
    }

    /// The offload request, read before mapping.
    pub(crate) fn meta(&self) -> &TxMeta {
        &self.meta
    }

    /// Unmaps everything (the device is done with the packet), returns the skb.
    pub(crate) fn unmap(self) -> SkBuff {
        self.into_skb()
    }
}

impl Drop for TxMapped<'_> {
    fn drop(&mut self) {
        self.unmap_all();
        // SAFETY: dropped once, here, after every mapping is gone.
        unsafe { ManuallyDrop::drop(&mut self.skb) }
    }
}

/// A descriptor ring in coherent memory: n descriptors of four 32-bit words,
/// read and written with bounds-checked volatile 32-bit accesses (no unsafe).
pub(crate) struct DescRing {
    mem: Coherent<[u32]>,
    n: usize,
}

impl DescRing {
    /// A zeroed ring of n descriptors for dev.
    pub(crate) fn new(dev: &Device<Bound>, n: usize) -> Result<Self> {
        let mem = Coherent::<u32>::zeroed_slice(dev, n.checked_mul(4).ok_or(EINVAL)?, GFP_KERNEL)?;
        Ok(Self { mem, n })
    }
    /// Number of descriptors.
    pub(crate) fn len(&self) -> usize {
        self.n
    }
    /// Bus address of descriptor 0.
    pub(crate) fn dma_addr(&self) -> DmaAddress {
        self.mem.dma_address()
    }
    /// Writes descriptor idx, word 0 first.
    pub(crate) fn write(&self, idx: usize, words: [u32; 4]) -> Result {
        if idx >= self.n {
            return Err(EINVAL);
        }
        for (i, w) in words.iter().enumerate() {
            kernel::io::io_write!(&self.mem, [try: idx * 4 + i], *w);
        }
        Ok(())
    }
    /// Reads word w of descriptor idx only (coherent memory is uncached on a
    /// non-coherent device: one bus read instead of four).
    pub(crate) fn read_word(&self, idx: usize, w: usize) -> Result<u32> {
        if idx >= self.n || w >= 4 {
            return Err(EINVAL);
        }
        Ok(kernel::io::io_read!(&self.mem, [try: idx * 4 + w]))
    }
    /// Reads descriptor idx.
    pub(crate) fn read(&self, idx: usize) -> Result<[u32; 4]> {
        if idx >= self.n {
            return Err(EINVAL);
        }
        let mut w = [0u32; 4];
        for (i, o) in w.iter_mut().enumerate() {
            *o = kernel::io::io_read!(&self.mem, [try: idx * 4 + i]);
        }
        Ok(w)
    }
}

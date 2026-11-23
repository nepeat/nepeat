// SPDX-License-Identifier: GPL-2.0

//! An owned `struct sk_buff` (`include/linux/skbuff.h`).

use super::{dma::RxFrag, napi::NapiCtx, netdev::NetDev};
use core::{mem::ManuallyDrop, ptr::NonNull};
use kernel::{bindings, prelude::*};

/// One owned reference to an `sk_buff`.
///
/// Dropping it frees the packet as a drop (`dev_kfree_skb_any`, any context);
/// [`SkBuff::consume`] frees it as delivered. Handing it to the stack
/// ([`NapiCtx::gro_receive`]) or to a TX mapping moves it.
///
/// # Invariants
///
/// `self.0` is a valid `sk_buff` and this value owns one reference to it.
pub(crate) struct SkBuff(NonNull<bindings::sk_buff>);

// SAFETY: an skb reference may be passed between contexts and CPUs (the stack
// does so all the time); this type gives no shared access.
unsafe impl Send for SkBuff {}

/// The TX offload request of a packet, read before it is mapped.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TxMeta {
    /// `skb->len`.
    pub(crate) len: u32,
    /// `ip_summed == CHECKSUM_PARTIAL`: the device must fill in an L4 checksum.
    pub(crate) csum_partial: bool,
    /// Where the checksummed area starts, from `skb->data`.
    pub(crate) csum_start: u32,
    /// Offset of the checksum field from `csum_start`.
    pub(crate) csum_offset: u16,
    /// `gso_size` (MSS) when a GSO packet, else 0.
    pub(crate) gso_size: u16,
    /// `gso_type` (`SKB_GSO_*`).
    pub(crate) gso_type: u32,
    /// L3 header offset from `skb->data`.
    pub(crate) network_offset: u32,
    /// L4 header offset from `skb->data`.
    pub(crate) transport_offset: u32,
    /// TCP header length (only meaningful for TCP GSO).
    pub(crate) tcp_hdrlen: u32,
    /// `skb->protocol`, host order (`ETH_P_*`).
    pub(crate) protocol: u16,
    /// The VLAN tag to insert, if any.
    pub(crate) vlan_tci: Option<u16>,
}

impl SkBuff {
    /// # Safety
    ///
    /// `ptr` is a valid `sk_buff` whose reference the caller gives to the result.
    pub(crate) unsafe fn from_raw(ptr: *mut bindings::sk_buff) -> Self {
        // SAFETY: valid, hence non-null, by the caller's promise.
        Self(unsafe { NonNull::new_unchecked(ptr) })
    }

    /// Gives up ownership without freeing (to the core, or to a C call that takes it).
    pub(crate) fn into_raw(self) -> *mut bindings::sk_buff {
        ManuallyDrop::new(self).0.as_ptr()
    }

    pub(crate) fn as_raw(&self) -> *mut bindings::sk_buff {
        self.0.as_ptr()
    }

    /// Frees it as a successfully handled packet (`dev_consume_skb_any`).
    pub(crate) fn consume(self) {
        // SAFETY: we own the reference (invariant), given up here.
        unsafe { bindings::dev_consume_skb_any(self.into_raw()) }
    }

    /// Total length (`skb->len`).
    pub(crate) fn len(&self) -> u32 {
        // SAFETY: valid skb (invariant), owned: no concurrent writer of `len`.
        unsafe { (*self.as_raw()).len }
    }

    /// Length of the linear part (`skb_headlen`).
    pub(crate) fn headlen(&self) -> u32 {
        // SAFETY: valid skb.
        unsafe { bindings::skb_headlen(self.as_raw()) }
    }

    /// The linear part's bytes (`skb->data`, `headlen` bytes).
    pub(crate) fn head(&self) -> &[u8] {
        let n = self.headlen() as usize;
        // SAFETY: `data .. data + headlen` is the skb's linear area, valid for reads
        // while we hold the reference; the borrow of `self` keeps it alive and no
        // `&mut` access to the bytes exists meanwhile (only `put`, which needs `&mut self`).
        unsafe { core::slice::from_raw_parts((*self.as_raw()).data, n) }
    }

    fn shinfo(&self) -> &bindings::skb_shared_info {
        // SAFETY: every skb has a valid shared info block at its end.
        unsafe { &*bindings::skb_shinfo(self.as_raw()) }
    }

    /// Number of paged fragments.
    pub(crate) fn nr_frags(&self) -> usize {
        usize::from(self.shinfo().nr_frags)
    }

    /// Fragment `i`: (page, offset in page, length).
    pub(crate) fn frag(&self, i: usize) -> Option<(*mut bindings::page, u32, u32)> {
        if i >= self.nr_frags() {
            return None;
        }
        let f = &self.shinfo().frags[i];
        // SAFETY: `f` is one of the skb's `nr_frags` initialised fragments.
        let page = unsafe { bindings::skb_frag_page(f) };
        // SAFETY: as above.
        let (off, len) = unsafe { (bindings::skb_frag_off(f), bindings::skb_frag_size(f)) };
        Some((page, off, len))
    }

    /// The TX queue the stack picked.
    pub(crate) fn queue_mapping(&self) -> u16 {
        // SAFETY: valid skb.
        unsafe { bindings::skb_get_queue_mapping(self.as_raw()) }
    }

    /// The offload request, for building TX descriptors.
    pub(crate) fn tx_meta(&self) -> TxMeta {
        let s = self.as_raw();
        let sh = self.shinfo();
        // SAFETY (all reads below): valid, owned skb; the accessors only read
        // header fields and offsets.
        unsafe {
            let csum_partial = bindings::skb_ip_summed(s) == bindings::CHECKSUM_PARTIAL as u8;
            let gso = bindings::skb_is_gso(s);
            TxMeta {
                len: (*s).len,
                csum_partial,
                csum_start: if csum_partial {
                    bindings::skb_checksum_start_offset(s) as u32
                } else {
                    0
                },
                csum_offset: if csum_partial { bindings::skb_csum_offset(s) } else { 0 },
                gso_size: if gso { sh.gso_size } else { 0 },
                gso_type: if gso { sh.gso_type } else { 0 },
                network_offset: bindings::skb_network_offset(s) as u32,
                transport_offset: if csum_partial || gso {
                    bindings::skb_transport_offset(s) as u32
                } else {
                    0
                },
                tcp_hdrlen: if gso && sh.gso_type & (bindings::SKB_GSO_TCPV4 | bindings::SKB_GSO_TCPV6) as u32 != 0 {
                    bindings::tcp_hdrlen(s)
                } else {
                    0
                },
                protocol: u16::from_be(bindings::skb_protocol(s)),
                vlan_tci: if bindings::skb_vlan_tag_present(s) {
                    Some(bindings::skb_vlan_tag_get(s))
                } else {
                    None
                },
            }
        }
    }

    /// `skb_checksum_help()`: fill in a pending (`CHECKSUM_PARTIAL`) L4 checksum in
    /// software, for a device that cannot. No-op for other packets.
    pub(crate) fn checksum_help(&mut self) -> Result {
        // SAFETY: valid, owned skb.
        if unsafe { bindings::skb_ip_summed(self.as_raw()) } != bindings::CHECKSUM_PARTIAL as u8 {
            return Ok(());
        }
        // SAFETY: owned skb, process or softirq context (xmit).
        kernel::error::to_result(unsafe { bindings::skb_checksum_help(self.as_raw()) })
    }

    /// `skb_tx_timestamp()`: call just before handing the packet to hardware.
    pub(crate) fn tx_timestamp(&mut self) {
        // SAFETY: valid, owned skb.
        unsafe { bindings::skb_tx_timestamp(self.as_raw()) }
    }

    /// Appends `n` bytes to the linear part and returns them (`skb_put`);
    /// `ENOSPC` if the tailroom is smaller (where `skb_put` would panic).
    pub(crate) fn put(&mut self, n: u32) -> Result<&mut [u8]> {
        // SAFETY: valid, owned skb.
        if unsafe { bindings::skb_tailroom(self.as_raw()) } < n {
            return Err(ENOSPC);
        }
        // SAFETY: tailroom checked: `skb_put` extends the linear area by `n` bytes
        // that we own exclusively through `&mut self`.
        let p = unsafe { bindings::skb_put(self.as_raw(), n) }.cast::<u8>();
        // SAFETY: `p .. p + n` was just added to the skb; the slice borrows `self`.
        Ok(unsafe { core::slice::from_raw_parts_mut(p, n as usize) })
    }

    /// `skb_reserve()`: headroom of `n` bytes in an empty skb.
    pub(crate) fn reserve(&mut self, n: u32) -> Result {
        // SAFETY: valid, owned skb.
        if self.len() != 0 || unsafe { bindings::skb_tailroom(self.as_raw()) } < n {
            return Err(EINVAL);
        }
        // SAFETY: empty skb with at least `n` bytes of tailroom.
        unsafe { bindings::skb_reserve(self.as_raw(), n as i32) };
        Ok(())
    }

    /// `skb->protocol = eth_type_trans(skb, dev)` (also pulls the Ethernet header
    /// and sets `pkt_type`). `EINVAL` for a packet shorter than an Ethernet header.
    pub(crate) fn eth_type_trans<T>(&mut self, dev: &NetDev<T>) -> Result {
        if self.headlen() < bindings::ETH_HLEN {
            return Err(EINVAL);
        }
        // SAFETY: owned skb with a full Ethernet header in its linear part; `dev`
        // is a registered device.
        unsafe {
            let p = bindings::eth_type_trans(self.as_raw(), dev.as_raw());
            bindings::skb_set_protocol(self.as_raw(), p);
        }
        Ok(())
    }

    /// The hardware verified the L4 checksum (`CHECKSUM_UNNECESSARY`).
    pub(crate) fn set_csum_unnecessary(&mut self) {
        // SAFETY: valid, owned skb.
        unsafe { bindings::skb_set_ip_summed(self.as_raw(), bindings::CHECKSUM_UNNECESSARY as u8) }
    }

    /// `skb_set_hash()`: an L4 (ports included) or L3 hash.
    pub(crate) fn set_hash(&mut self, hash: u32, l4: bool) {
        let t = if l4 { bindings::pkt_hash_types_PKT_HASH_TYPE_L4 } else { bindings::pkt_hash_types_PKT_HASH_TYPE_L3 };
        // SAFETY: valid, owned skb.
        unsafe { bindings::skb_set_hash(self.as_raw(), hash, t) }
    }

    /// `skb_record_rx_queue()`.
    pub(crate) fn record_rx_queue(&mut self, q: u16) {
        // SAFETY: valid, owned skb.
        unsafe { bindings::skb_record_rx_queue(self.as_raw(), q) }
    }

    /// `__vlan_hwaccel_put_tag()`: the hardware stripped this tag (`proto`: host order).
    pub(crate) fn put_vlan_tag(&mut self, proto: u16, tci: u16) {
        // SAFETY: valid, owned skb.
        unsafe { bindings::__vlan_hwaccel_put_tag(self.as_raw(), proto.to_be(), tci) }
    }

    /// Appends a received buffer as a paged fragment (`skb_add_rx_frag`): the
    /// skb takes over the fragment's page reference. Gives the fragment back if
    /// the skb has no fragment slot left or the range is outside the fragment.
    pub(crate) fn add_rx_frag(&mut self, frag: RxFrag, off: u32, len: u32) -> core::result::Result<(), RxFrag> {
        let i = self.nr_frags();
        if i >= bindings::MAX_SKB_FRAGS as usize || off.checked_add(len).is_none_or(|e| e > frag.size()) {
            return Err(frag);
        }
        let (page, page_off, truesize) = frag.page_parts();
        frag.into_raw();
        // SAFETY: owned skb with a free fragment slot `i`; `page`/`page_off` name
        // the fragment's memory and its one page reference, which `into_raw` gave
        // up and the skb now owns (freed with the skb); `off + len` is inside it.
        unsafe {
            bindings::skb_add_rx_frag(self.as_raw(), i as i32, page, (page_off + off) as i32, len as i32, truesize)
        };
        Ok(())
    }

    /// Hands the packet to GRO (`napi_gro_receive`).
    pub(crate) fn gro_receive(self, napi: &mut NapiCtx<'_>) {
        napi.gro_receive(self)
    }
}

impl Drop for SkBuff {
    fn drop(&mut self) {
        // SAFETY: we own the reference (invariant); any context.
        unsafe { bindings::dev_kfree_skb_any(self.as_raw()) }
    }
}

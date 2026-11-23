// SPDX-License-Identifier: GPL-2.0

//! knet: the kernel networking interfaces a NIC driver needs that the 7.3 kernel
//! crate has no abstraction for (net_device, NAPI, sk_buff, streaming DMA, the TX
//! queue lock, ethtool), as small safe types. All of the driver's `unsafe` lives
//! here; every block has a SAFETY comment and the per-file count is in README.md.
//!
//! Bindings: the opt-in Rust kernel (packages/kernel-rust, `rustNet`) carries a patch
//! that adds linux/netdevice.h, etherdevice.h, skbuff.h, if_vlan.h to
//! rust/bindings/bindings_helper.h and C helpers for the static inlines used
//! here, so bindgen checks every signature (no hand-written `extern "C"`).
//!
//! Ownership model (who owns what, when):
//! - [`netdev::Registration<T>`] owns the `net_device`; the driver's private
//!   data `T` lives in its private area from `Registration::new` until drop
//!   (unregister, drop `T`, free: in that order, so no callback sees a dead `T`).
//! - [`napi::Napi<P>`] owns the NAPI context *and* the state `P` its poll
//!   function works on (the RX/TX ring). `P` is reachable mutably only from
//!   the poll function or through a [`napi::NapiOff`] guard, which exists only
//!   while NAPI is disabled: never both.
//! - [`netdev::TxLocked<S>`] is data protected by a TX queue's own
//!   `_xmit_lock`: `start_xmit` gets the guard from the core, NAPI and process
//!   context take it; no second lock.
//! - [`skb::SkBuff`] is an owned `sk_buff`: moved into the stack (GRO, xmit
//!   return), into a TX mapping, or freed on drop. Never two owners.
//! - [`dma::TxMapped`] / [`dma::RxBuf`] own a buffer *and* its streaming
//!   mapping. While they exist the CPU has no access to the bytes (no accessor);
//!   unmapping consumes them and gives the buffer back. Dropping one unmaps
//!   first, so a mapping can neither leak nor outlive its buffer. What Rust cannot
//!   see is the device's own DMA: a buffer handed to hardware must only be
//!   dropped after the hardware is done with it (completion) or the DMA engine is
//!   stopped; that is the driver's ring logic (hal), stated where rings are torn down.
#![allow(dead_code)]

pub(crate) mod dma;
pub(crate) mod ether;
pub(crate) mod ethtool;
pub(crate) mod napi;
pub(crate) mod netdev;
pub(crate) mod skb;

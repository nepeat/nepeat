//! The OS rim of the CS8210-32X platform tools, and the only crate of this workspace
//! with `unsafe` code: a mapped PCI BAR with 32-bit volatile accesses ([`mmio`]) and
//! i2c-dev `SMBus` byte transfers ([`i2cdev`]). Everything above it is
//! `#![forbid(unsafe_code)]`.
//!
//! Each unsafe block holds one operation and a `// SAFETY:` contract (clippy
//! `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` are errors).
//! Inventory: README.md "Unsafe".
#![deny(unsafe_code)]

pub mod i2cdev;
pub mod mmio;

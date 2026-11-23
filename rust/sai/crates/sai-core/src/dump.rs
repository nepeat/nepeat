//! `sai_dbg_generate_dump`: a text summary of the model (backend, switch, ports, VLANs, counts per
//! type, LAGs, host interfaces). Ports the dump of `sai_api.cpp`.
//!
//! Backend: [`crate::backend::Base`] (`name`, `hardware_info`); otherwise the store.
//!
//! Registration: none in the dispatch table; the rim's global function calls it (today a fixed
//! `NOT_IMPLEMENTED` in `sai-ffi/src/global.rs`).

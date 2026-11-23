//! The external PHY runtime hook (AS7772 retimers and `MACsec` PHYs): link gate per port
//! ([`crate::backend::types::LinkGate`]: down at one bad poll, up after two good, 500 ms polls,
//! 3000 ms hold-off after diag MDIO), admin up / down sequences around
//! [`crate::backend::Backend::port_admin_set`], counters and status. Ports the runtime half of
//! `sai_extphy.cpp` (591 lines); the table parser and executor are `phy-seq` already.
//!
//! Backend: [`crate::backend::Mdio`] (clause 45 only), [`crate::backend::PortOps::port_link_gate`].
//! The gate closure must not take the SAI lock: it keeps its verdict behind its own lock and does
//! its MDIO through its own bus handle (on the virtual chip:
//! [`crate::virt::VirtualBackend::mdio`]).
//!
//! Registration: no object type. The port area calls the hooks from `set ADMIN_STATE`; switch
//! create loads the table and sets the gates (C++ `ext_phy_load` / `attach` / `detach` /
//! `admin`). The runtime itself is planned as its own crate (`sai-extphy`); this module is the
//! seam on the core's side.

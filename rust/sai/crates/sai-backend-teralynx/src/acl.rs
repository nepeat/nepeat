//! ACLs, policers and the `CoPP` table (`tl_acl.{h,cpp}`). Ported in G1: `forget` (the shutdown
//! drops the state; the IFCS objects go with `ifcs_deinit`). Everything else is G2, as the diag
//! shell's `acl`.
#![forbid(unsafe_code)]

/// The ACL area's state (nothing exists in G1).
#[derive(Debug, Default)]
pub struct AclArea {}

impl AclArea {
    /// `AclArea::forget`.
    pub const fn forget(&mut self) {}
}

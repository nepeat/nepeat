//! The owner's fixes of the C++ (sai-macsec's crate documentation, D9 and D10; decision of
//! 2026-11-23) written as an edit of `sai_macsec.cpp`: the reference the differential tests
//! hold the Rust model to wherever it differs from the unmodified C++. Where a script reaches
//! one of the two, the Rust output must equal, to the end of the script, the output of the C++
//! with exactly these edits; nothing else may differ. (D1 is not here: the scripts never move
//! a flow between ports.)
//!
//! Shared by `tests/differential.rs`, `build.rs` (the prebuilt drivers of a cross build) and
//! sai-macsec-phy's `tests/e2e.rs`. The source tree is not touched: the edited copy is written
//! next to the objects. An anchor that is not found exactly once fails the build of the driver.

/// (what, anchor in `sai_macsec.cpp`, its replacement)
const EDITS: &[(&str, &str, &str)] = &[
    (
        // reconcile (:287): when the bound flow is another flow than an installed SA's SC's, that
        // SA comes out first (the gate closes before the last), as the Rust `reconcile` does
        "D9",
        "    sai_object_id_t flow = bound_flow(port, dir);\n    // pending SAs of the SC(s) on the bound flow, oldest first\n",
        "    sai_object_id_t flow = bound_flow(port, dir);
    if (flow) {   // sai-macsec D9 (Q7 fixed): the SAs of every other SC come out of the PHY
        std::vector<sai_object_id_t> stale;
        for (auto& e : s.sas) {
            MsSa& sa = e.second;
            if (sa.dir != dir || !sa.installed || h.sa[sa.an] != e.first) continue;
            auto sc = s.scs.find(sa.sc);
            if (sc == s.scs.end() || sc->second.flow == flow) continue;
            stale.push_back(e.first);
        }
        for (sai_object_id_t id : stale) {
            OB_NOTICE(SAI_API_MACSEC, \"port %s: %s SA AN %u of another flow's SC taken out\", pit->second.name.c_str(), kDir[dir],
                      s.sas[id].an);
            sa_uninstall(hw, s.sas[id], id);
        }
    }
    // pending SAs of the SC(s) on the bound flow, oldest first
",
    ),
    (
        // set_macsec_sc_attribute (:806): no cipher suite change under waiting SAs either
        "D10",
        "            if (has_sa && attr->value.s32 != sc.cipher) return SAI_STATUS_OBJECT_IN_USE;\n",
        "            {   // sai-macsec D10 (Q10 fixed)
                bool waiting = false;
                for (auto& sa : S().sas) waiting = waiting || (sa.second.sc == id && !sa.second.installed);
                if ((has_sa || waiting) && attr->value.s32 != sc.cipher) return SAI_STATUS_OBJECT_IN_USE;
            }
",
    ),
];

/// `sai_macsec.cpp` with D9 and D10; `Err` names an anchor that is not there exactly once
/// (the C++ changed: this file must follow it).
pub(crate) fn apply(source: &str) -> Result<String, String> {
    let mut out = source.to_owned();
    for (what, anchor, replacement) in EDITS {
        let n = out.matches(anchor).count();
        if n != 1 {
            return Err(format!(
                "sai_macsec.cpp: the anchor of {what} is there {n} times, not once (tests/cpp/listed_fixes.rs must follow the C++)"
            ));
        }
        out = out.replacen(anchor, replacement, 1);
    }
    Ok(out)
}

/// Writes the edited copy of `src/sai_macsec.cpp` to `dir` and returns its path (compile it with
/// `-I src`: its includes are the library's).
pub(crate) fn write_fixed(src: &std::path::Path, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let from = src.join("sai_macsec.cpp");
    let text = std::fs::read_to_string(&from).map_err(|e| format!("{}: {e}", from.display()))?;
    let to = dir.join("sai_macsec_fixed.cpp");
    std::fs::write(&to, apply(&text)?).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to)
}

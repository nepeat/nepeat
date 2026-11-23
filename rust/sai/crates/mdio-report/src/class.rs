//! Failure classes: what kind of thing went wrong, and what to do about it in general terms.

use mdio_script::{E_ABORT, E_FILE, E_IO, E_POLL, E_SYNTAX, E_VERIFY};

use crate::report::{PhyState, Report};

/// The class of a failed run (the JSON's `failure.class`, the word in `FAILED (...)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailClass {
    /// `script-missing`: the file is not there or unreadable.
    ScriptMissing,
    /// `script-malformed`: truncated or not a script.
    ScriptMalformed,
    /// `sdk-busy`: another process owns the switch chip.
    SdkBusy,
    /// `sdk-config-missing`
    SdkConfig,
    /// `sdk-init`: attach / init soc / init misc failed.
    SdkInit,
    /// `mdio-clock`: MDIO rate or hold delay on the chip not as configured.
    MdioClock,
    /// `mdio-io`: the MDIO master reported an error.
    MdioIo,
    /// `phy-not-answering`: a PHY reads one constant everywhere.
    PhyConstant,
    /// `sequence-diverged`: a poll never saw its value; the PHY answers.
    Diverged,
    /// `verify-mismatch`: ran to the end, read-back differs.
    Verify,
    /// `interrupted`
    Interrupted,
}

impl FailClass {
    /// Every class.
    pub const ALL: [Self; 11] = [
        Self::ScriptMissing,
        Self::ScriptMalformed,
        Self::SdkBusy,
        Self::SdkConfig,
        Self::SdkInit,
        Self::MdioClock,
        Self::MdioIo,
        Self::PhyConstant,
        Self::Diverged,
        Self::Verify,
        Self::Interrupted,
    ];

    /// The class as written in the status files and given to `--hint`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScriptMissing => "script-missing",
            Self::ScriptMalformed => "script-malformed",
            Self::SdkBusy => "sdk-busy",
            Self::SdkConfig => "sdk-config-missing",
            Self::SdkInit => "sdk-init",
            Self::MdioClock => "mdio-clock",
            Self::MdioIo => "mdio-io",
            Self::PhyConstant => "phy-not-answering",
            Self::Diverged => "sequence-diverged",
            Self::Verify => "verify-mismatch",
            Self::Interrupted => "interrupted",
        }
    }

    /// What to do about it, in general terms (boards add their own).
    pub const fn hint(self) -> &'static str {
        match self {
            Self::ScriptMissing => "install the script file (private data, copied by hand), then start the unit again",
            Self::ScriptMalformed => "the file is truncated or not an MDIO script: install it again",
            Self::SdkBusy => "stop the process that owns the switch chip, then start the unit again",
            Self::SdkConfig => "the SDK config file given with --config is missing: a build or deployment problem",
            Self::SdkInit => "the switch chip did not initialise: see the SDK messages before this line in the journal",
            Self::MdioClock => {
                "the SDK config must set rate_ext_mdio_divisor and mdio_output_delay; no MDIO frame was sent"
            }
            Self::MdioIo => "the switch chip's MDIO master failed (not a PHY): see the SDK messages in the journal",
            Self::PhyConstant => {
                "a PHY that reads one constant on every register does not recover by itself: \
                 power-cycle the switch (cold), then let the unit run again"
            }
            Self::Diverged => {
                "start the unit again; if it stops at the same step, the script does not match this \
                 hardware or its firmware"
            }
            Self::Verify => {
                "start the unit again; a PHY that keeps differing is not in the state the script was \
                 recorded for"
            }
            Self::Interrupted => "the PHYs may be half-programmed: start the unit again",
        }
    }
}

/// The class of a failed script run. `code`: the `mdio_script::E_*` code; `fail_phy`: the PHY of
/// its failure record (`None`: no record); `post`: the verification reads made at the end or
/// after the failure, classified (`None` if none could be made). `None` for a code that is not
/// a failure.
pub fn classify_run(code: i32, fail_phy: Option<u16>, post: Option<&Report>) -> Option<FailClass> {
    let constant = || post.is_some_and(|r| r.count(PhyState::Constant) > 0);
    Some(match code {
        E_FILE => FailClass::ScriptMissing,
        E_SYNTAX => FailClass::ScriptMalformed,
        E_IO => FailClass::MdioIo,
        E_ABORT => FailClass::Interrupted,
        E_POLL => {
            // The PHY the script stopped at decides; when it is not in the verification list,
            // any PHY that reads a constant does.
            let stopped_at = fail_phy.and_then(|phy| post.and_then(|r| r.find(phy)));
            let not_answering = stopped_at.map_or_else(constant, |p| p.state == PhyState::Constant);
            if not_answering {
                FailClass::PhyConstant
            } else {
                FailClass::Diverged
            }
        }
        E_VERIFY => {
            if constant() {
                FailClass::PhyConstant
            } else if post.is_some_and(|r| r.count(PhyState::Io) > 0) {
                FailClass::MdioIo
            } else {
                FailClass::Verify
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_without_reads() {
        assert_eq!(classify_run(E_POLL, Some(1), None), Some(FailClass::Diverged));
        assert_eq!(classify_run(E_FILE, None, None), Some(FailClass::ScriptMissing));
        assert_eq!(classify_run(E_SYNTAX, None, None), Some(FailClass::ScriptMalformed));
        assert_eq!(classify_run(E_IO, None, None), Some(FailClass::MdioIo));
        assert_eq!(classify_run(E_ABORT, None, None), Some(FailClass::Interrupted));
        assert_eq!(classify_run(E_VERIFY, None, None), Some(FailClass::Verify));
        assert_eq!(classify_run(0, None, None), None);
    }

    #[test]
    fn every_class_has_a_name_and_a_hint() {
        let mut names: Vec<&str> = FailClass::ALL.iter().map(|c| c.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), FailClass::ALL.len());
        assert!(
            FailClass::ALL
                .iter()
                .all(|c| !c.hint().is_empty() && c.hint().len() < 400)
        );
        assert!(FailClass::PhyConstant.hint().contains("power-cycle"));
    }
}

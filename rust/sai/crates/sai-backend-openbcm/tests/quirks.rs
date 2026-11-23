//! The port settings of the profile: the accepted values are exactly the C++ ones.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use proptest::prelude::*;
use sai_backend_openbcm::quirks::{AUTONEG_KEY, FEC_KEY, IFG_KEY, INTERFACE_KEY, PortQuirks};
use sai_core::Error;
use sai_core::backend::Profile;
use sdk_glue::Fec;

fn parse(pairs: &[(&str, &str)]) -> Result<PortQuirks, Error> {
    PortQuirks::parse(&Profile::from_pairs(pairs.iter().copied()))
}

#[test]
fn the_as7772_profile() {
    let q = parse(&[
        (INTERFACE_KEY, "caui"),
        (AUTONEG_KEY, "off"),
        (FEC_KEY, "none"),
        (IFG_KEY, "128"),
    ])
    .unwrap();
    assert_eq!(q.interface.as_deref(), Some(c"caui"));
    assert_eq!((q.autoneg, q.fec, q.ifg), (Some(false), Some(Fec::None), Some(128)));
    assert!(!q.is_empty());
    assert_eq!(
        q.to_string(),
        "interface caui, ASIC FEC fixed none, autoneg fixed off, IFG 128 bits"
    );
}

#[test]
fn absent_and_empty_values_fix_nothing() {
    assert!(parse(&[]).unwrap().is_empty());
    let q = parse(&[(INTERFACE_KEY, ""), (AUTONEG_KEY, ""), (FEC_KEY, ""), (IFG_KEY, "")]).unwrap();
    assert!(q.is_empty());
    assert_eq!(
        q.to_string(),
        "interface (SDK), ASIC FEC (SDK), autoneg (SDK), IFG (SDK)"
    );
}

#[test]
fn every_allowed_value() {
    for name in sdk_glue::port::INTERFACE_NAMES {
        assert_eq!(
            parse(&[(INTERFACE_KEY, name)])
                .unwrap()
                .interface
                .unwrap()
                .to_str()
                .unwrap(),
            name
        );
    }
    assert_eq!(parse(&[(FEC_KEY, "rs")]).unwrap().fec, Some(Fec::Rs));
    assert_eq!(parse(&[(FEC_KEY, "fc")]).unwrap().fec, Some(Fec::Fc));
    assert_eq!(parse(&[(AUTONEG_KEY, "on")]).unwrap().autoneg, Some(true));
    assert_eq!(parse(&[(IFG_KEY, "64")]).unwrap().ifg, Some(64));
    assert_eq!(parse(&[(IFG_KEY, "504")]).unwrap().ifg, Some(504));
    for (key, value) in [
        (INTERFACE_KEY, "Caui"),
        (INTERFACE_KEY, "caui "),
        (FEC_KEY, "RS"),
        (FEC_KEY, "off"),
        (AUTONEG_KEY, "true"),
        (IFG_KEY, "63"),
        (IFG_KEY, "65"),
        (IFG_KEY, "505"),
        (IFG_KEY, "0x80"),
        (IFG_KEY, "+128"),
        (IFG_KEY, " 128"),
        (IFG_KEY, "128\0"),
        (IFG_KEY, "99999999999999999999"),
    ] {
        assert_eq!(parse(&[(key, value)]), Err(Error::InvalidParameter), "{key}={value:?}");
    }
}

proptest! {
    /// Any four strings: never a panic, and accepted exactly when each one is empty or one of
    /// the listed values.
    #[test]
    fn accepts_exactly_the_listed_values(interface in ".{0,12}", fec in ".{0,6}", autoneg in ".{0,6}", ifg in "[0-9 +x-]{0,6}") {
        let result = parse(&[(INTERFACE_KEY, &interface), (FEC_KEY, &fec), (AUTONEG_KEY, &autoneg), (IFG_KEY, &ifg)]);
        let ifg_ok = ifg.is_empty()
            || (ifg.bytes().all(|b| b.is_ascii_digit()) && ifg.parse::<u32>().is_ok_and(|n| (64..=504).contains(&n) && n % 8 == 0));
        let ok = (interface.is_empty() || sdk_glue::port::INTERFACE_NAMES.contains(&interface.as_str()))
            && ["", "none", "rs", "fc"].contains(&fec.as_str())
            && ["", "off", "on"].contains(&autoneg.as_str())
            && ifg_ok;
        prop_assert_eq!(result.is_ok(), ok);
        if let Ok(q) = result {
            prop_assert_eq!(q.ifg, ifg.parse::<u32>().ok());
            prop_assert_eq!(q.is_empty(), interface.is_empty() && fec.is_empty() && autoneg.is_empty() && ifg.is_empty());
        }
    }

    /// Every multiple of 8 in range is accepted, nothing else.
    #[test]
    fn ifg_range(n in 0u32..1000) {
        let text = n.to_string();
        let result = parse(&[(IFG_KEY, &text)]);
        prop_assert_eq!(result.is_ok(), (64..=504).contains(&n) && n % 8 == 0);
    }
}

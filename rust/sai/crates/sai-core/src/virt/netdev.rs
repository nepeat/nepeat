//! The kernel side of the virtual backend's host interfaces, as the C++ fake backend has it
//! (`fake_backend.cpp`: `SAI_OPENBCM_FAKE_NETDEV`, `SAI_OPENBCM_FAKE_WIRE`, `tap_open`,
//! `netif_carrier_set`, `pump`):
//!
//! - mode `tap`: every host interface of type NETDEV is also a Linux TAP netdev of its name,
//!   with the switch's MAC address, created with its carrier off; its carrier is the host
//!   interface's `OPER_STATUS` (which orchagent sets from the port's oper status), like a KNET
//!   netif. Admin state and MTU are the kernel's business (portmgrd sets them with `ip`).
//!   Removing the host interface (or the switch) removes the netdev.
//! - a wire `<netif>:<ifname>`: frames the TAP sends go out of the host's interface
//!   `<ifname>`, and what `<ifname>` receives comes up the TAP, while the port is admin up
//!   with link: the CPU path of that port to a peer VM. No switching between TAPs.
//!
//! The system calls are in the `tap-netdev` crate; this module is safe code.
use crate::backend::HwPort;
use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tap_netdev::{IfName, PacketSocket, Tap, Wire};

/// `SAI_RS_VIRTUAL_NETDEV`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetdevMode {
    /// `none` (default): host interfaces exist only in the model.
    #[default]
    None,
    /// `tap`: each is also a TAP netdev (needs `CAP_NET_ADMIN` and `/dev/net/tun`).
    Tap,
}

/// A line on stderr, where the rim's log goes too (the C++ logs these as errors / notices).
pub(crate) fn note(message: core::fmt::Arguments<'_>) {
    // A closed stderr must not take the SAI user down.
    let _ = writeln!(std::io::stderr().lock(), "sai-rs virtual: {message}");
}

/// Parses `SAI_RS_VIRTUAL_WIRE`: `<netif>:<ifname>` items separated by commas, as the C++
/// `parse_wire` does (the same accepted language: each side 1..=15 bytes, the netif before
/// the first colon; no netif twice; no host interface for two netifs; the empty string and a
/// trailing comma are fine). `None`: malformed.
pub fn parse_wire(text: &str) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (item, next) = match rest.split_once(',') {
            Some((item, next)) => (item, Some(next)),
            None => (rest, None),
        };
        let colon = item.find(':')?;
        let (netif, ifname) = (item.get(..colon)?, item.get(colon + 1..)?);
        if netif.is_empty() || ifname.is_empty() || netif.len() > 15 || ifname.len() > 15 {
            return None;
        }
        if out.contains_key(netif) || out.values().any(|v: &String| v == ifname) {
            return None;
        }
        out.insert(netif.to_owned(), ifname.to_owned());
        match next {
            Some(next) => rest = next,
            None => break,
        }
    }
    Some(out)
}

/// A host interface's netdev, owned: dropping it stops the wire (joins its thread) and then
/// closes the TAP, which removes the netdev. Field order is drop order.
#[derive(Debug)]
pub(crate) struct Kernel {
    wire: Option<Wire>,
    tap: Arc<Tap>,
}

impl Kernel {
    /// Creates the TAP `name` for `port` (carrier off, MAC `mac`), wired to `wire_to` if
    /// given; `gate`: the port is up. Nothing is left behind on error.
    pub(crate) fn open(
        name: &str,
        port: HwPort,
        mac: [u8; 6],
        wire_to: Option<&str>,
        gate: &Arc<AtomicBool>,
    ) -> Result<Self> {
        let fail = |what: &str, e: &dyn core::fmt::Display| {
            note(format_args!("TAP {name} (port {}): {what}: {e}", port.0));
            Error::Failure
        };
        let if_name = IfName::new(name).map_err(|e| fail("name", &e))?;
        let tap = Arc::new(Tap::create(if_name, mac).map_err(|e| fail("create", &e))?);
        let wire = match wire_to {
            None => None,
            Some(host) => {
                let host_name = IfName::new(host).map_err(|e| fail("wire interface", &e))?;
                let socket = PacketSocket::bind(host_name).map_err(|e| fail(&format!("wire interface {host}"), &e))?;
                let wire = Wire::start(Arc::clone(&tap), socket, Arc::clone(gate)).map_err(|e| fail("wire", &e))?;
                note(format_args!("TAP {name} wired to {host}"));
                Some(wire)
            }
        };
        Ok(Self { wire, tap })
    }

    /// The netdev's carrier.
    pub(crate) fn set_carrier(&self, up: bool) -> Result<()> {
        self.tap.set_carrier(up).map_err(|e| {
            note(format_args!("TAP {} carrier: {e}", self.tap.name()));
            Error::Failure
        })
    }

    /// Whether it has a wire whose thread still runs.
    pub(crate) fn wired(&self) -> bool {
        self.wire.as_ref().is_some_and(Wire::is_running)
    }
}

/// Whether this process can make TAP netdevs (`/dev/net/tun` opens read-write; creates nothing).
pub(crate) fn tun_available() -> bool {
    Tap::available().is_ok()
}

/// A name the kernel accepts for a netdev.
pub(crate) fn valid_name(name: &str) -> bool {
    IfName::new(name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
    }

    #[test]
    fn wire_lists_as_the_cpp_parses_them() {
        assert_eq!(parse_wire(""), Some(map(&[])));
        assert_eq!(
            parse_wire("Ethernet16:eth1,Ethernet20:eth2"),
            Some(map(&[("Ethernet16", "eth1"), ("Ethernet20", "eth2")]))
        );
        assert_eq!(parse_wire("a:b,"), Some(map(&[("a", "b")])), "trailing comma");
        assert_eq!(parse_wire("a:b:c"), Some(map(&[("a", "b:c")])), "first colon splits");
        for bad in [
            ",",
            ",a:b",
            "a:b,,c:d",
            "a",
            ":b",
            "a:",
            "a:b,a:c",
            "a:b,c:b",
            "1234567890123456:b",
            "a:1234567890123456",
        ] {
            assert_eq!(parse_wire(bad), None, "{bad:?}");
        }
        assert!(parse_wire("123456789012345:123456789012345").is_some());
    }

    proptest! {
        #[test]
        fn parsing_never_panics(text in "\\PC{0,64}") {
            let _ = parse_wire(&text);
        }

        #[test]
        fn a_list_written_from_a_map_parses_back(
            pairs in proptest::collection::btree_map("[a-zA-Z0-9]{1,15}", "[a-z0-9]{1,15}", 0..6),
            trailing in any::<bool>(),
        ) {
            // host interfaces distinct, as the parser requires
            let mut seen = std::collections::BTreeSet::new();
            let pairs: BTreeMap<String, String> = pairs.into_iter().filter(|(_, v)| seen.insert(v.clone())).collect();
            let mut text = pairs.iter().map(|(a, b)| format!("{a}:{b}")).collect::<Vec<_>>().join(",");
            if trailing && !text.is_empty() {
                text.push(',');
            }
            prop_assert_eq!(parse_wire(&text), Some(pairs));
        }
    }
}

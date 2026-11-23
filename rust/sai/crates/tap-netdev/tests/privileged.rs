//! Real TAP netdevs and a real wire. Needs `CAP_NET_ADMIN` and `/dev/net/tun`, and `ip` (iproute2)
//! to set links up: runs only with `SAI_RS_PRIVILEGED_TESTS=1`, which only the NixOS VM tests
//! set (or by hand in a throw-away network namespace:
//! `unshare -rnm sh -c 'mount -t sysfs sysfs /sys && SAI_RS_PRIVILEGED_TESTS=1 <test binary>'`).
//! Otherwise every test says why it did nothing and passes: the test bundle also runs on a
//! real switch, where creating netdevs is not wanted.
use std::io::{Read, Write};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tap_netdev::{IfName, PacketSocket, Tap, Wire};

fn enabled(test: &str) -> bool {
    let on = std::env::var_os("SAI_RS_PRIVILEGED_TESTS").is_some_and(|v| v == "1");
    if !on {
        eprintln!("{test}: skipped (creates netdevs; set SAI_RS_PRIVILEGED_TESTS=1 where that is wanted)");
    }
    on
}

fn sysfs(name: &str, attr: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{name}/{attr}"))
        .ok()
        .map(|s| s.trim().to_owned())
}

fn ip(args: &[&str]) {
    let status = Command::new("ip").args(args).status();
    assert!(
        status.as_ref().is_ok_and(std::process::ExitStatus::success),
        "ip {args:?}: {status:?}"
    );
}

fn up(name: &str) {
    // No IPv6 autoconfiguration noise on the test links.
    let _ = std::fs::write(format!("/proc/sys/net/ipv6/conf/{name}/disable_ipv6"), "1");
    ip(&["link", "set", "dev", name, "up"]);
}

#[test]
fn a_tap_netdev_lives_as_long_as_its_handle() {
    if !enabled("a_tap_netdev_lives_as_long_as_its_handle") {
        return;
    }
    let tap = Tap::create(IfName::new("sairs-t0").unwrap(), [0x02, 0x00, 0x5a, 0x11, 0x22, 0x01]).unwrap();
    assert_eq!(tap.name().as_str(), "sairs-t0");
    assert_eq!(sysfs("sairs-t0", "address").as_deref(), Some("02:00:5a:11:22:01"));
    assert_eq!(sysfs("sairs-t0", "tun_flags").as_deref(), Some("0x1002")); // IFF_TAP | IFF_NO_PI
    assert_eq!(sysfs("sairs-t0", "mtu").as_deref(), Some("1500"));
    // a second device of the same name cannot be made while the first exists
    assert!(Tap::create(IfName::new("sairs-t0").unwrap(), [0x02, 0, 0, 0, 0, 2]).is_err());

    up("sairs-t0");
    assert_eq!(
        sysfs("sairs-t0", "carrier").as_deref(),
        Some("0"),
        "created with the carrier off"
    );
    tap.set_carrier(true).unwrap();
    assert_eq!(sysfs("sairs-t0", "carrier").as_deref(), Some("1"));
    tap.set_carrier(false).unwrap();
    assert_eq!(sysfs("sairs-t0", "carrier").as_deref(), Some("0"));

    drop(tap);
    assert!(
        sysfs("sairs-t0", "ifindex").is_none(),
        "the netdev went with the handle"
    );
}

#[test]
fn an_unknown_wire_interface_is_an_error() {
    if !enabled("an_unknown_wire_interface_is_an_error") {
        return;
    }
    assert!(PacketSocket::bind(IfName::new("sairs-none").unwrap()).is_err());
}

const ETHERTYPE: [u8; 2] = [0x88, 0xb5]; // IEEE local experimental

fn frame(tag: u8) -> Vec<u8> {
    let mut f = vec![0xff; 6];
    f.extend([0x02, 0x00, 0x5a, 0x00, 0x00, tag]);
    f.extend(ETHERTYPE);
    f.extend(b"sai-rs wire test");
    f.push(tag);
    f.resize(60, 0);
    f
}

/// Reads frames from `source` on a thread; the receiver gets ours (by `EtherType`) only.
fn reader(mut source: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![0; 2048];
        while let Ok(n) = source.read(&mut buf) {
            let Some(frame) = buf.get(..n) else { return };
            if frame.get(12..14) == Some(&ETHERTYPE[..]) && tx.send(frame.to_vec()).is_err() {
                return;
            }
        }
    });
    rx
}

struct ArcFile(Arc<Tap>);
impl Read for ArcFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.file().read(buf)
    }
}

#[test]
fn a_wire_carries_frames_both_ways_while_its_gate_is_open() {
    if !enabled("a_wire_carries_frames_both_ways_while_its_gate_is_open") {
        return;
    }
    // "sairs-a" stands for a port's netdev, "sairs-b" for the host interface (eth1 in the VM
    // test) its CPU path is wired to.
    let port = Arc::new(Tap::create(IfName::new("sairs-a").unwrap(), [0x02, 0, 0, 0, 0xa, 1]).unwrap());
    let host = Arc::new(Tap::create(IfName::new("sairs-b").unwrap(), [0x02, 0, 0, 0, 0xb, 1]).unwrap());
    for (tap, name) in [(&port, "sairs-a"), (&host, "sairs-b")] {
        up(name);
        tap.set_carrier(true).unwrap();
    }
    let gate = Arc::new(AtomicBool::new(true));
    let wire = Wire::start(
        Arc::clone(&port),
        PacketSocket::bind(IfName::new("sairs-b").unwrap()).unwrap(),
        Arc::clone(&gate),
    )
    .unwrap();
    assert!(wire.is_running());
    // what the port's netdev receives / what the host interface transmits
    let on_port = PacketSocket::bind(IfName::new("sairs-a").unwrap()).unwrap();
    let port_rx = reader(on_port.file().try_clone().unwrap());
    let host_tx = reader(ArcFile(Arc::clone(&host)));
    let wait = Duration::from_secs(3);

    // host interface receives -> up the port's netdev
    host.file().write_all(&frame(1)).unwrap();
    assert_eq!(port_rx.recv_timeout(wait).unwrap(), frame(1));
    // the port's netdev sends -> out of the host interface
    on_port.file().write_all(&frame(2)).unwrap();
    assert_eq!(host_tx.recv_timeout(wait).unwrap(), frame(2));

    // port down: nothing passes, either way
    gate.store(false, Ordering::Release);
    host.file().write_all(&frame(3)).unwrap();
    on_port.file().write_all(&frame(4)).unwrap();
    assert!(port_rx.recv_timeout(Duration::from_millis(500)).is_err());
    assert!(host_tx.recv_timeout(Duration::from_millis(500)).is_err());
    gate.store(true, Ordering::Release);
    host.file().write_all(&frame(5)).unwrap();
    assert_eq!(port_rx.recv_timeout(wait).unwrap(), frame(5));

    let stopping = Instant::now();
    drop(wire);
    assert!(stopping.elapsed() < Duration::from_secs(2), "the pump stops promptly");
    host.file().write_all(&frame(6)).unwrap();
    assert!(
        port_rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "no pump, no frames"
    );
    // the pump held a clone of the port's TAP: it is ours alone again
    assert_eq!(Arc::strong_count(&port), 1);
}

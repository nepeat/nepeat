//! `l3-frames`: the port of `tools/l3_frames.c`.
//!
//! `l3-frames tx=IF rx=IF[,IF..] dst=IP [src=IP] dmac=MAC [smac=MAC] [count=N] [pps=R] [ttl=N]
//! [size=BYTES] [flows=N] [dst_n=N] [dport=N] [tos=N] [opts=1] [wait_ms=MS]`: sends tagged
//! IPv4 / IPv6 UDP frames and counts what comes back on other netdevs, grouped by how the ASIC
//! changed them (VLAN tag, DMAC, SMAC, TTL, DSCP, length).

use crate::bytes::{array, at, ne16, put, put_be16, put_u8};
use crate::common::{Clock, complain, rx_socket, say, send_frame, tx_socket, up, usleep};
use crate::cstd::{atof, atoi, atou, fmt_f, parse_mac};
use crate::csum::{fold, sum16};
use crate::os::{PACKET_OUTGOING, Sock, poll_readable};
use std::fmt::Write as _;
use std::net::{Ipv4Addr, Ipv6Addr};

/// The tag in every frame.
pub const TAG: [u8; 8] = *b"L3FRAMES";
/// Receive interfaces at most.
pub const MAX_RX: usize = 4;
/// Distinct (interface, header) groups at most.
pub const MAX_GROUPS: usize = 64;
/// Distinct flow numbers tracked per group.
pub const MAX_FLOWS: usize = 4096;

/// What to send.
#[derive(Debug, Clone)]
pub struct Cfg {
    /// IPv6 instead of IPv4.
    pub v6: bool,
    /// Destination MAC.
    pub dmac: [u8; 6],
    /// Source MAC.
    pub smac: [u8; 6],
    /// Source address (the first 4 bytes for IPv4).
    pub src: [u8; 16],
    /// Destination address.
    pub dst: [u8; 16],
    /// TTL / hop limit.
    pub ttl: i32,
    /// Frame length without FCS.
    pub size: i32,
    /// UDP destination port.
    pub dport: i32,
    /// IPv4 TOS / IPv6 traffic class.
    pub tos: i32,
    /// IPv4 with 4 bytes of NOP options.
    pub opts: bool,
    /// UDP source ports cycle through this many values.
    pub flows: u32,
    /// The destination address cycles through this many consecutive addresses (0: one).
    pub dst_n: u32,
}

/// The low 16 bits of a C `int` (`(uint16_t)v`).
fn low16(v: i32) -> u16 {
    let b = v.to_le_bytes();
    u16::from_le_bytes([b[0], b[1]])
}

/// Frame `i` of the run `run_id`.
pub fn build(c: &Cfg, run_id: u32, i: u32) -> Vec<u8> {
    let l3 = 14;
    let ipl: usize = if c.v6 {
        40
    } else if c.opts {
        24
    } else {
        20
    };
    let len = usize::try_from(c.size).map_or(64, |s| s.max(64));
    // the tag and counters are written past a short IPv6 frame (see the C tool); only `len` goes out
    let mut f = vec![0u8; len.max(l3 + ipl + 26)];
    put(&mut f, 0, &c.dmac);
    put(&mut f, 6, &c.smac);
    put(&mut f, 12, if c.v6 { &[0x86, 0xdd] } else { &[0x08, 0x00] });
    let ip = l3;
    let udp = ip + ipl;
    let udpl = len - l3 - ipl;
    let mut dst = c.dst;
    let mut add = if c.dst_n > 0 { i % c.dst_n } else { 0 };
    let last = if c.v6 { 15 } else { 3 };
    for slot in dst.iter_mut().take(last + 1).rev() {
        if add == 0 {
            break;
        }
        let v = u32::from(*slot) + (add & 0xff);
        *slot = v.to_le_bytes()[0];
        add = (add >> 8) + (v >> 8);
    }
    let flow = if c.flows > 0 { i % c.flows } else { 0 };
    let sport = u16::try_from((10000 + flow) & 0xffff).unwrap_or(0);
    put_be16(&mut f, udp, sport);
    put_be16(&mut f, udp + 2, low16(c.dport));
    let ul = u16::try_from(udpl & 0xffff).unwrap_or(0);
    put_be16(&mut f, udp + 4, ul);
    put(&mut f, udp + 8, &TAG);
    put(&mut f, udp + 16, &run_id.to_ne_bytes());
    put(&mut f, udp + 20, &i.to_ne_bytes());
    put(
        &mut f,
        udp + 24,
        &u16::try_from(flow & 0xffff).unwrap_or(0).to_ne_bytes(),
    );
    if c.v6 {
        put_u8(&mut f, ip, 0x60 | ((c.tos >> 4) & 0x0f).to_le_bytes()[0]);
        put_u8(&mut f, ip + 1, ((c.tos & 0x0f) << 4).to_le_bytes()[0]);
        put_be16(&mut f, ip + 4, ul);
        put_u8(&mut f, ip + 6, 17);
        put_u8(&mut f, ip + 7, c.ttl.to_le_bytes()[0]);
        put(&mut f, ip + 8, &c.src);
        put(&mut f, ip + 24, &dst);
        let mut ph = [0u8; 40];
        put(&mut ph, 0, f.get(ip + 8..ip + 40).unwrap_or_default());
        put(&mut ph, 32, &u32::try_from(udpl).unwrap_or(0).to_be_bytes());
        put_u8(&mut ph, 39, 17);
        let seg = f.get(udp..udp + udpl).unwrap_or_default();
        let mut cs = fold(sum16(seg, sum16(&ph, 0)));
        if cs == 0 {
            cs = 0xffff;
        }
        put_be16(&mut f, udp + 6, cs);
    } else {
        put_u8(&mut f, ip, if c.opts { 0x46 } else { 0x45 });
        put_u8(&mut f, ip + 1, c.tos.to_le_bytes()[0]);
        if c.opts {
            put(&mut f, ip + 20, &[1, 1, 1, 1]);
        }
        put_be16(&mut f, ip + 2, u16::try_from((len - l3) & 0xffff).unwrap_or(0));
        put_u8(&mut f, ip + 8, c.ttl.to_le_bytes()[0]);
        put_u8(&mut f, ip + 9, 17);
        put(&mut f, ip + 12, &array::<4>(&c.src, 0));
        put(&mut f, ip + 16, &array::<4>(&dst, 0));
        let ic = fold(sum16(f.get(ip..ip + ipl).unwrap_or_default(), 0));
        put_be16(&mut f, ip + 10, ic);
    }
    f.truncate(len);
    f
}

/// One (interface, VLAN, DMAC, SMAC, TTL, DSCP, length) group of received frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Receive interface index in the list.
    pub rx: usize,
    /// VLAN id, -1 untagged.
    pub vlan: i32,
    /// TTL / hop limit.
    pub ttl: i32,
    /// TOS / traffic class.
    pub tos: i32,
    /// Frame length.
    pub len: usize,
    /// Destination MAC.
    pub dmac: [u8; 6],
    /// Source MAC.
    pub smac: [u8; 6],
    /// Frames in the group.
    pub count: u64,
    /// Bit set of the flow numbers seen.
    pub flows: Vec<u8>,
}

impl Group {
    /// The number of distinct flows.
    pub fn distinct_flows(&self) -> u32 {
        self.flows.iter().map(|b| b.count_ones()).sum()
    }
}

/// What the receive side counted.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    /// The groups in order of first appearance.
    pub groups: Vec<Group>,
    /// Frames with our tag and run id.
    pub ours: u64,
    /// Everything else.
    pub other: u64,
    /// Frames of groups beyond the 64.
    pub dropped_groups: u64,
}

impl Stats {
    /// `account()`: one frame received on interface `rx`.
    pub fn account(&mut self, rx: usize, frame: &[u8], run_id: u32) {
        let n = frame.len();
        let b = frame;
        if n < 14 {
            self.other += 1;
            return;
        }
        let (vlan, o) = if at(b, 12) == 0x81 && at(b, 13) == 0x00 {
            (i32::from(at(b, 14) & 0x0f) << 8 | i32::from(at(b, 15)), 16)
        } else {
            (-1, 12)
        };
        let v6 = at(b, o) == 0x86 && at(b, o + 1) == 0xdd;
        let v4 = at(b, o) == 0x08 && at(b, o + 1) == 0x00;
        let ip = o + 2;
        let udp = ip
            + if v6 {
                40
            } else if n > ip {
                usize::from(at(b, ip) & 0x0f) * 4
            } else {
                20
            };
        if (!v4 && !v6)
            || n < udp + 26
            || b.get(udp + 8..udp + 16) != Some(&TAG[..])
            || b.get(udp + 16..udp + 20) != Some(&run_id.to_ne_bytes()[..])
        {
            self.other += 1;
            return;
        }
        self.ours += 1;
        let ttl = i32::from(if v6 { at(b, ip + 7) } else { at(b, ip + 8) });
        let tos = if v6 {
            i32::from((at(b, ip) & 0x0f) << 4 | at(b, ip + 1) >> 4)
        } else {
            i32::from(at(b, ip + 1))
        };
        let fl = usize::from(ne16(b, udp + 24));
        let dmac: [u8; 6] = array(b, 0);
        let smac: [u8; 6] = array(b, 6);
        let found = self.groups.iter().position(|g| {
            g.rx == rx
                && g.vlan == vlan
                && g.ttl == ttl
                && g.len == n
                && g.tos == tos
                && g.dmac == dmac
                && g.smac == smac
        });
        let idx = if let Some(i) = found {
            i
        } else {
            if self.groups.len() == MAX_GROUPS {
                self.dropped_groups += 1;
                return;
            }
            self.groups.push(Group {
                rx,
                vlan,
                ttl,
                tos,
                len: n,
                dmac,
                smac,
                count: 0,
                flows: vec![0; MAX_FLOWS / 8],
            });
            self.groups.len() - 1
        };
        if let Some(g) = self.groups.get_mut(idx) {
            g.count += 1;
            if fl < MAX_FLOWS {
                if let Some(byte) = g.flows.get_mut(fl / 8) {
                    *byte |= 1 << (fl % 8);
                }
            }
        }
    }
}

fn mac_text(m: [u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[0], m[1], m[2], m[3], m[4], m[5]
    )
}

/// The `RESULT` line and one `RX` line per group.
#[allow(clippy::too_many_arguments)]
pub fn report(
    tx: &str,
    dst: &str,
    c: &Cfg,
    sent: u32,
    failed: u32,
    seconds: f64,
    stats: &Stats,
    names: &[String],
) -> String {
    let mut s = format!(
        "RESULT tx={tx} dst={dst} ttl={} size={} flows={} sent={sent} send_errors={failed} seconds={} rx_ours={} rx_other={}{}\n",
        c.ttl,
        c.size,
        c.flows,
        fmt_f(seconds, 3),
        stats.ours,
        stats.other,
        if stats.dropped_groups > 0 {
            " (groups overflowed)"
        } else {
            ""
        }
    );
    for g in &stats.groups {
        let vl = if g.vlan < 0 { "-".to_owned() } else { g.vlan.to_string() };
        let _ = writeln!(
            s,
            "RX if={} vlan={vl} dmac={} smac={} ttl={} dscp={} len={} count={} flows={}",
            names.get(g.rx).map_or("", String::as_str),
            mac_text(g.dmac),
            mac_text(g.smac),
            g.ttl,
            g.tos >> 2,
            g.len,
            g.count,
            g.distinct_flows()
        );
    }
    s
}

/// The `drain()` of the C tool: reads what is waiting on the sockets.
fn drain(socks: &[Sock], stats: &mut Stats, run_id: u32, buf: &mut [u8], timeout_ms: i32) {
    let refs: Vec<&Sock> = socks.iter().collect();
    let mut timeout = timeout_ms;
    while let Ok(Some(ready)) = poll_readable(&refs, timeout) {
        for (k, (sock, is_ready)) in socks.iter().zip(ready).enumerate() {
            if !is_ready {
                continue;
            }
            let Ok((m, pkttype)) = sock.recv_from_packet(buf, true) else {
                continue;
            };
            if m == 0 || pkttype == PACKET_OUTGOING {
                continue;
            }
            stats.account(k, buf.get(..m).unwrap_or_default(), run_id);
        }
        timeout = 0;
    }
}

/// The value of `key=value` among the arguments.
fn kv<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter()
        .skip(1)
        .find_map(|a| a.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
}

/// `main`: the exit status.
pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

struct Opts<'a> {
    tx: &'a str,
    rx: &'a str,
    dst: &'a str,
    cfg: Cfg,
    count: u32,
    pps: f64,
    wait_ms: i32,
}

fn addr_bytes(text: &str, v6: bool) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    if v6 {
        out = text.parse::<Ipv6Addr>().ok()?.octets();
    } else {
        put(&mut out, 0, &text.parse::<Ipv4Addr>().ok()?.octets());
    }
    Some(out)
}

fn parse(args: &[String]) -> Result<Opts<'_>, i32> {
    let (tx, rx, dst, dmac) = (kv(args, "tx"), kv(args, "rx"), kv(args, "dst"), kv(args, "dmac"));
    let (Some(tx), Some(rx), Some(dst), Some(dmac)) = (tx, rx, dst, dmac) else {
        complain(&format!(
            "usage: {} tx=IF rx=IF[,IF] dst=IP dmac=MAC [src=IP] [smac=MAC] [count=N] [pps=R] [ttl=N] [size=N] [flows=N] [dst_n=N] [wait_ms=MS]",
            args.first().map_or("", String::as_str)
        ));
        return Err(2);
    };
    let v6 = dst.contains(':');
    let src = kv(args, "src").unwrap_or(if v6 { "fc00::99" } else { "10.250.0.99" });
    let (Some(dst_b), Some(src_b)) = (addr_bytes(dst, v6), addr_bytes(src, v6)) else {
        complain("bad src/dst (same family)");
        return Err(2);
    };
    let Some(dmac_b) = parse_mac(dmac) else {
        complain("bad dmac");
        return Err(2);
    };
    let get = |k: &str, d: &str| atoi(kv(args, k).unwrap_or(d));
    let opts = get("opts", "0") != 0 && !v6;
    let cfg = Cfg {
        v6,
        dmac: dmac_b,
        smac: [0; 6],
        src: src_b,
        dst: dst_b,
        ttl: get("ttl", "64"),
        size: get("size", "128").min(9600),
        dport: get("dport", "7777"),
        tos: get("tos", "0"),
        opts,
        flows: atou(kv(args, "flows").unwrap_or("1")).min(u32::try_from(MAX_FLOWS).unwrap_or(4096)),
        dst_n: atou(kv(args, "dst_n").unwrap_or("0")),
    };
    Ok(Opts {
        tx,
        rx,
        dst,
        cfg,
        count: atou(kv(args, "count").unwrap_or("100")),
        pps: atof(kv(args, "pps").unwrap_or("1000")),
        wait_ms: get("wait_ms", "1500"),
    })
}

/// `srand(time ^ pid)`; two draws combined as the C tool does (the generator differs, the id is
/// random either way).
fn make_run_id() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()) ^ d.as_secs());
    let mut x = (secs ^ u64::from(std::process::id())).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        u32::try_from(x >> 33).unwrap_or(0)
    };
    next() ^ (next() << 16)
}

fn run(args: &[String]) -> Result<(), i32> {
    let mut o = parse(args)?;
    let run_id = make_run_id();
    let (txi, txmac) = up(o.tx)?;
    o.cfg.smac = txmac;
    if let Some(s) = kv(args, "smac") {
        let Some(m) = parse_mac(s) else {
            complain("bad smac");
            return Err(2);
        };
        o.cfg.smac = m;
    }
    let list: String = o.rx.chars().take(255).collect();
    let names: Vec<String> = crate::cstd::tokens(&list)
        .take(MAX_RX)
        .map(|t| String::from_utf8_lossy(t.as_bytes().get(..15.min(t.len())).unwrap_or_default()).into_owned())
        .collect();
    let mut socks = Vec::new();
    for t in crate::cstd::tokens(&list).take(MAX_RX) {
        let (ix, _) = up(t)?;
        let s = rx_socket(ix)?;
        let _ = s.set_recv_buffer(8 << 20);
        socks.push(s);
    }
    usleep(200_000);
    let mut stats = Stats::default();
    let mut buf = vec![0u8; 16384];
    drain(&socks, &mut stats, run_id, &mut buf, 0);
    let tx = tx_socket();
    let clock = Clock::default();
    let (mut sent, mut failed) = (0u32, 0u32);
    let t0 = clock.now();
    for i in 0..o.count {
        let frame = build(&o.cfg, run_id, i);
        if send_frame(tx.as_ref(), txi, o.cfg.dmac, &frame) {
            sent += 1;
        } else {
            failed += 1;
        }
        if o.pps > 0.0 {
            let due = t0 + f64::from(i + 1) / o.pps;
            while clock.now() < due {
                drain(&socks, &mut stats, run_id, &mut buf, 0);
                usleep(20);
            }
        }
        if i % 64 == 0 {
            drain(&socks, &mut stats, run_id, &mut buf, 0);
        }
    }
    let t1 = clock.now();
    let end = clock.now() + f64::from(o.wait_ms) / 1000.0;
    while clock.now() < end {
        drain(&socks, &mut stats, run_id, &mut buf, 50);
    }
    say(&report(o.tx, o.dst, &o.cfg, sent, failed, t1 - t0, &stats, &names));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(v6: bool) -> Cfg {
        let mut src = [0u8; 16];
        let mut dst = [0u8; 16];
        if v6 {
            src = "fc00:1::2".parse::<Ipv6Addr>().unwrap().octets();
            dst = "fc00:2::2".parse::<Ipv6Addr>().unwrap().octets();
        } else {
            src[..4].copy_from_slice(&[10, 1, 0, 2]);
            dst[..4].copy_from_slice(&[10, 30, 0, 1]);
        }
        Cfg {
            v6,
            dmac: [2, 0, 0, 0, 1, 0x0a],
            smac: [2, 0, 0, 0, 0x0a, 2],
            src,
            dst,
            ttl: 64,
            size: 128,
            dport: 7777,
            tos: 0xb8,
            opts: false,
            flows: 256,
            dst_n: 0,
        }
    }

    #[test]
    fn v4_frame() {
        let f = build(&cfg(false), 0xdead_beef, 300);
        assert_eq!(f.len(), 128);
        assert_eq!(&f[12..14], &[8, 0]);
        assert_eq!(f[14], 0x45);
        assert_eq!(f[15], 0xb8);
        assert_eq!(&f[16..18], &114u16.to_be_bytes());
        assert_eq!(crate::csum::csum(&f[14..34]), 0);
        assert_eq!(&f[34..36], &(10000u16 + 300 % 256).to_be_bytes());
        assert_eq!(&f[36..38], &7777u16.to_be_bytes());
        assert_eq!(&f[42..50], b"L3FRAMES");
        assert_eq!(&f[50..54], &0xdead_beefu32.to_ne_bytes());
        assert_eq!(&f[54..58], &300u32.to_ne_bytes());
        assert_eq!(&f[58..60], &44u16.to_ne_bytes());
    }

    #[test]
    fn v4_options_and_destination_cycling() {
        let mut c = cfg(false);
        c.opts = true;
        c.dst_n = 300;
        c.dst[..4].copy_from_slice(&[10, 0, 0, 250]);
        let f = build(&c, 1, 299);
        assert_eq!(f[14], 0x46);
        assert_eq!(&f[34..38], &[1, 1, 1, 1]);
        // 250 + 299 carries into the third byte: 10.0.2.37
        assert_eq!(&f[30..34], &[10, 0, 2, 37]);
        assert_eq!(crate::csum::csum(&f[14..38]), 0);
    }

    #[test]
    fn v6_frame_checksum_verifies() {
        let c = cfg(true);
        let f = build(&c, 7, 1);
        assert_eq!(&f[12..14], &[0x86, 0xdd]);
        assert_eq!(f[14], 0x6b);
        assert_eq!(f[15], 0x80);
        let udp = &f[54..];
        let mut ph = f[22..54].to_vec();
        ph.extend_from_slice(&u32::try_from(udp.len()).unwrap().to_be_bytes());
        ph.extend_from_slice(&[0, 0, 0, 17]);
        ph.extend_from_slice(udp);
        assert_eq!(crate::csum::csum(&ph), 0);
    }

    #[test]
    fn short_v6_frame_cuts_the_tag() {
        let mut c = cfg(true);
        c.size = 64;
        let f = build(&c, 7, 1);
        assert_eq!(f.len(), 64);
        assert_eq!(&f[62..64], b"L3");
    }

    #[test]
    fn grouping_and_flow_counting() {
        let c = cfg(false);
        let mut st = Stats::default();
        for i in 0..300 {
            st.account(1, &build(&c, 9, i), 9);
        }
        st.account(0, &build(&c, 8, 0), 9);
        st.account(0, &[0u8; 10], 9);
        assert_eq!((st.ours, st.other), (300, 2));
        assert_eq!(st.groups.len(), 1);
        assert_eq!(st.groups[0].count, 300);
        assert_eq!(st.groups[0].distinct_flows(), 256);
        let text = report(
            "a",
            "10.30.0.1",
            &c,
            300,
            0,
            1.0,
            &st,
            &["x".into(), "Ethernet0".into()],
        );
        assert!(text.starts_with("RESULT tx=a dst=10.30.0.1 ttl=64 size=128 flows=256 sent=300 send_errors=0 seconds=1.000 rx_ours=300 rx_other=2\n"));
        assert!(text.ends_with("RX if=Ethernet0 vlan=- dmac=02:00:00:00:01:0a smac=02:00:00:00:0a:02 ttl=64 dscp=46 len=128 count=300 flows=256\n"));
    }

    #[test]
    fn vlan_tagged_frames_are_parsed() {
        let c = cfg(false);
        let f = build(&c, 9, 0);
        let mut t = f[..12].to_vec();
        t.extend_from_slice(&[0x81, 0, 0x00, 0x64]);
        t.extend_from_slice(&f[12..]);
        let mut st = Stats::default();
        st.account(0, &t, 9);
        assert_eq!(st.groups[0].vlan, 100);
    }

    #[test]
    fn key_value_arguments() {
        let a: Vec<String> = ["l3-frames", "tx=a", "txx=b", "count=5"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(kv(&a, "tx"), Some("a"));
        assert_eq!(kv(&a, "count"), Some("5"));
        assert_eq!(kv(&a, "rx"), None);
    }
}

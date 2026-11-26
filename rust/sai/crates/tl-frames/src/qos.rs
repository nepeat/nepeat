//! `qos-frames`: the port of `tools/qos_frames.c`.
//!
//! `qos-frames TX_IF RX_IF[,RX_IF..] COUNT PPS SIZE DMAC SRC_IP DST_IP DSCP[,DSCP..]`: sends
//! tagged IPv4 / UDP frames with chosen DSCPs (or PFC frames, DMAC `pfc`) and counts them on
//! one or more netdevs: per DSCP, CE-marked, GRE-encapsulated copies, received rate.

use crate::bytes::{at, find, put, put_be16, put_u8};
use crate::common::{Clock, complain, rx_socket, say, send_frame, tx_socket, up, usleep, wait_until};
use crate::cstd::{atof, atoi, atou, fmt_f, parse_mac, tokens};
use crate::csum::csum;
use crate::os::{PACKET_OUTGOING, Sock};
use std::fmt::Write as _;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The tag in every frame.
pub const TAG: [u8; 8] = *b"QOSFRAME";
/// Receive interfaces at most.
pub const MAXRX: usize = 4;
/// DSCP values at most.
pub const MAXD: usize = 16;

/// The frame size argument as the C tool clamps it (`size_t`: a negative number is huge).
pub fn clamp_size(arg: i32) -> usize {
    match usize::try_from(arg) {
        Ok(v) => v.clamp(64, 9000),
        Err(_) => 9000,
    }
}

/// The low byte of a C `int` (`(uint8_t)v`).
fn low_byte(v: i32) -> u8 {
    v.to_le_bytes()[0]
}

/// IPv4 / UDP frame `i` of `size` bytes with the DSCP at index `di` (`dscp` is its value).
#[allow(clippy::too_many_arguments)]
pub fn build_udp(
    size: usize,
    dmac: [u8; 6],
    smac: [u8; 6],
    dscp: i32,
    ecn: i32,
    sip: [u8; 4],
    dip: [u8; 4],
    di: u8,
    seq: u32,
) -> Vec<u8> {
    let mut f = vec![0u8; size];
    put(&mut f, 0, &dmac);
    put(&mut f, 6, &smac);
    put(&mut f, 12, &[0x08, 0x00]);
    put_u8(&mut f, 14, 0x45);
    put_u8(&mut f, 15, low_byte(dscp << 2 | ecn));
    put_be16(&mut f, 16, u16::try_from((size - 14) & 0xffff).unwrap_or(0));
    put_u8(&mut f, 22, 64);
    put_u8(&mut f, 23, 17);
    put(&mut f, 26, &sip);
    put(&mut f, 30, &dip);
    let ic = csum(f.get(14..34).unwrap_or_default());
    put_be16(&mut f, 24, ic);
    put_be16(&mut f, 34, 40000);
    put_be16(&mut f, 36, 9999);
    put_be16(&mut f, 38, u16::try_from((size - 34) & 0xffff).unwrap_or(0));
    put(&mut f, 42, &TAG);
    put_u8(&mut f, 50, di);
    put(&mut f, 51, &seq.to_ne_bytes());
    f
}

/// The 802.1Qbb PFC frame for the priorities `dscps & 7` (64 bytes).
pub fn build_pfc(dmac: [u8; 6], smac: [u8; 6], dscps: &[i32]) -> Vec<u8> {
    let mut f = vec![0u8; 64];
    put(&mut f, 0, &dmac);
    put(&mut f, 6, &smac);
    put(&mut f, 12, &[0x88, 0x08, 0x01, 0x01]);
    let cev = dscps.iter().fold(0u16, |v, d| v | 1u16 << (d & 7));
    put_be16(&mut f, 16, cev);
    for k in 0..8 {
        if cev & (1 << k) != 0 {
            put(&mut f, 18 + 2 * k, &[0xff, 0xff]);
        }
    }
    put(&mut f, 34, &TAG);
    f
}

/// What one receive interface counted.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RxStats {
    /// Our frames (not GRE-encapsulated).
    pub ours: u64,
    /// GRE-encapsulated copies.
    pub gre: u64,
    /// Frames without our tag.
    pub other: u64,
    /// Bytes of ours and GRE frames.
    pub bytes: u64,
    /// Our frames per DSCP index.
    pub per: [u64; MAXD],
    /// CE-marked frames per DSCP index.
    pub ce: [u64; MAXD],
    /// Time of the first and last tagged frame.
    pub first: f64,
    /// See `first`.
    pub last: f64,
    /// The outer header of the first GRE copy.
    pub gre_info: Option<String>,
}

impl RxStats {
    /// Counts one received frame of `n` bytes in `b`, received at time `ts`.
    pub fn account(&mut self, b: &[u8], n: usize, ts: f64) {
        let frame = b.get(..n).unwrap_or(b);
        let Some(t) = find(frame, &TAG).filter(|t| t + TAG.len() < n) else {
            self.other += 1;
            return;
        };
        if self.ours == 0 && self.gre == 0 {
            self.first = ts;
        }
        self.last = ts;
        self.bytes += u64::try_from(n).unwrap_or(0);
        if n > 38 && at(b, 12) == 0x08 && at(b, 13) == 0x00 && at(b, 14 + 9) == 47 && t > 34 + 4 {
            self.gre += 1;
            if self.gre_info.is_none() {
                self.gre_info = Some(gre_info(b, n));
            }
            return;
        }
        self.ours += 1;
        let d = usize::from(at(b, t + TAG.len()));
        if d < MAXD {
            if let Some(c) = self.per.get_mut(d) {
                *c += 1;
            }
            if n > 16 && at(b, 12) == 0x08 && at(b, 13) == 0x00 && at(b, 15) & 3 == 3 {
                if let Some(c) = self.ce.get_mut(d) {
                    *c += 1;
                }
            }
        }
    }
}

fn gre_info(b: &[u8], n: usize) -> String {
    let ip = 14;
    let g = ip + usize::from(at(b, ip) & 15) * 4;
    let ipb = |k: usize| at(b, ip + k);
    format!(
        "outer {}.{}.{}.{} > {}.{}.{}.{} tos=0x{:02x} ttl={} gre_flags=0x{:02x}{:02x} gre_proto=0x{:02x}{:02x} len={}",
        ipb(12),
        ipb(13),
        ipb(14),
        ipb(15),
        ipb(16),
        ipb(17),
        ipb(18),
        ipb(19),
        ipb(1),
        ipb(8),
        at(b, g),
        at(b, g + 1),
        at(b, g + 2),
        at(b, g + 3),
        n
    )
}

/// The `SENT` line.
pub fn sent_line(tx: &str, sent: u32, failed: u32, seconds: f64, size: usize, t1: f64, t0: f64) -> String {
    let mbps = if t1 > t0 {
        f64::from(sent) * crate::usize_f64(size) * 8.0 / (t1 - t0) / 1e6
    } else {
        0.0
    };
    format!(
        "SENT tx={tx} sent={sent} send_errors={failed} seconds={} tx_mbps={} size={size}\n",
        fmt_f(seconds, 3),
        fmt_f(mbps, 1)
    )
}

/// The `RESULT` (and `GRE`) lines of one receive interface.
pub fn result_lines(name: &str, sent: u32, r: &RxStats, dscps: &[i32]) -> String {
    let span = r.last - r.first;
    let mut s = format!(
        "RESULT if={name} sent={sent} rx={} gre={} other={}",
        r.ours, r.gre, r.other
    );
    for (d, v) in dscps.iter().zip(r.per) {
        let _ = write!(s, " dscp{d}={v}");
    }
    for (d, v) in dscps.iter().zip(r.ce) {
        if v != 0 {
            let _ = write!(s, " ce{d}={v}");
        }
    }
    let mbps = if span > 0.0 {
        crate::u64_f64(r.bytes) * 8.0 / span / 1e6
    } else {
        0.0
    };
    let _ = writeln!(s, " rx_seconds={} rx_mbps={}", fmt_f(span, 3), fmt_f(mbps, 1));
    if let Some(info) = &r.gre_info {
        let _ = writeln!(s, "GRE if={name} {info}");
    }
    s
}

fn rx_loop(sock: &Sock, stop: &AtomicBool, clock: &Clock) -> RxStats {
    let mut stats = RxStats::default();
    let mut buf = vec![0u8; 10000];
    while !stop.load(Ordering::Relaxed) {
        let Ok((n, pkttype)) = sock.recv_from_packet(&mut buf, false) else {
            continue;
        };
        if n == 0 || pkttype == PACKET_OUTGOING {
            continue;
        }
        stats.account(&buf, n, clock.now());
    }
    stats
}

/// `main`: the exit status.
pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

struct Opts {
    count: u32,
    pps: f64,
    size: usize,
    dmac: [u8; 6],
    pfc: bool,
    ecn: i32,
    sip: [u8; 4],
    dip: [u8; 4],
    dscps: Vec<i32>,
}

fn parse(args: &[String]) -> Result<Opts, i32> {
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    if args.len() < 10 {
        complain(&format!(
            "usage: {} TX_IF RX_IF[,RX_IF..] COUNT PPS SIZE DMAC SRC_IP DST_IP DSCP[,DSCP..]",
            arg(0)
        ));
        return Err(2);
    }
    let pfc = arg(6) == "pfc";
    let dmac = if pfc {
        [0x01, 0x80, 0xc2, 0, 0, 0x01]
    } else {
        parse_mac(arg(6)).ok_or(2)?
    };
    let ecn = std::env::var("ECN").map_or(0, |v| atoi(&v) & 3);
    let sip = arg(7).parse::<Ipv4Addr>().map_err(|_| 2)?.octets();
    let dip = arg(8).parse::<Ipv4Addr>().map_err(|_| 2)?.octets();
    let dscps: Vec<i32> = tokens(arg(9)).take(MAXD).map(atoi).collect();
    if dscps.is_empty() {
        // the C tool divides by zero (SIGFPE) at the first frame
        complain("no DSCP values");
        return Err(2);
    }
    Ok(Opts {
        count: atou(arg(3)),
        pps: atof(arg(4)),
        size: clamp_size(atoi(arg(5))),
        dmac,
        pfc,
        ecn,
        sip,
        dip,
        dscps,
    })
}

fn open_rx(names: &[&str]) -> Result<Vec<Sock>, i32> {
    let mut socks = Vec::new();
    for name in names {
        let (ix, _) = up(name)?;
        let s = rx_socket(ix)?;
        let _ = s.force_recv_buffer(64 << 20);
        let _ = s.set_recv_timeout(Duration::from_millis(100));
        socks.push(s);
    }
    Ok(socks)
}

fn run(args: &[String]) -> Result<(), i32> {
    let o = parse(args)?;
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    let tx = arg(1);
    let names: Vec<&str> = tokens(arg(2)).take(MAXRX).collect();
    let rxs = open_rx(&names)?;
    let (txi, txmac) = up(tx)?;
    let txs = tx_socket();
    let clock = Clock::default();
    let stop = AtomicBool::new(false);
    let (sent, failed, t0, t1, stats) = std::thread::scope(|scope| {
        let readers: Vec<_> = rxs.iter().map(|s| scope.spawn(|| rx_loop(s, &stop, &clock))).collect();
        usleep(200_000);
        let (mut sent, mut failed) = (0u32, 0u32);
        let t0 = clock.now();
        let nd = u32::try_from(o.dscps.len()).unwrap_or(1);
        for i in 0..o.count {
            let di = i % nd;
            let frame = if o.pfc {
                build_pfc(o.dmac, txmac, &o.dscps)
            } else {
                let dscp = o.dscps.get(usize::try_from(di).unwrap_or(0)).copied().unwrap_or(0);
                build_udp(
                    o.size,
                    o.dmac,
                    txmac,
                    dscp,
                    o.ecn,
                    o.sip,
                    o.dip,
                    u8::try_from(di).unwrap_or(0),
                    i,
                )
            };
            if send_frame(txs.as_ref(), txi, o.dmac, &frame) {
                sent += 1;
            } else {
                failed += 1;
            }
            if o.pps > 0.0 {
                wait_until(&clock, t0 + f64::from(i + 1) / o.pps, 20);
            }
        }
        let t1 = clock.now();
        usleep(1_500_000);
        stop.store(true, Ordering::Relaxed);
        let stats: Vec<RxStats> = readers.into_iter().map(|h| h.join().unwrap_or_default()).collect();
        (sent, failed, t0, t1, stats)
    });
    say(&sent_line(tx, sent, failed, t1 - t0, o.size, t1, t0));
    for (name, r) in names.iter().zip(&stats) {
        say(&result_lines(name, sent, r, &o.dscps));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_clamp_like_size_t() {
        assert_eq!(clamp_size(10), 64);
        assert_eq!(clamp_size(256), 256);
        assert_eq!(clamp_size(100_000), 9000);
        assert_eq!(clamp_size(-5), 9000);
    }

    #[test]
    fn udp_frame_layout() {
        let f = build_udp(
            256,
            [2, 0x99, 0x99, 0x99, 0x99, 0x99],
            [2, 0, 0, 0, 0, 1],
            46,
            1,
            [10, 99, 9, 1],
            [10, 99, 9, 2],
            2,
            77,
        );
        assert_eq!(f.len(), 256);
        assert_eq!(f[15], 46 << 2 | 1);
        assert_eq!(&f[16..18], &242u16.to_be_bytes());
        assert_eq!(csum(&f[14..34]), 0);
        assert_eq!(&f[38..40], &222u16.to_be_bytes());
        assert_eq!(&f[42..50], b"QOSFRAME");
        assert_eq!(f[50], 2);
        assert_eq!(&f[51..55], &77u32.to_ne_bytes());
    }

    #[test]
    fn pfc_frame_layout() {
        let f = build_pfc([1, 0x80, 0xc2, 0, 0, 1], [2, 0, 0, 0, 0, 1], &[3, 11]);
        assert_eq!(&f[12..16], &[0x88, 0x08, 0x01, 0x01]);
        assert_eq!(&f[16..18], &[0x00, 0x08]);
        assert_eq!(&f[18 + 6..18 + 8], &[0xff, 0xff]);
        assert_eq!(&f[18..20], &[0, 0]);
        assert_eq!(&f[34..42], b"QOSFRAME");
    }

    #[test]
    fn counting_per_dscp_ce_and_gre() {
        let mut s = RxStats::default();
        let mut f = build_udp(128, [0; 6], [0; 6], 10, 3, [1, 2, 3, 4], [5, 6, 7, 8], 1, 0);
        s.account(&f, 128, 1.0);
        f[15] = 10 << 2;
        s.account(&f, 128, 2.0);
        s.account(&[0u8; 60], 60, 3.0);
        assert_eq!((s.ours, s.other, s.per[1], s.ce[1], s.bytes), (2, 1, 2, 1, 256));
        assert!((s.first - 1.0).abs() < 1e-9 && (s.last - 2.0).abs() < 1e-9);
        // an outer IPv4 / GRE header in front
        let mut outer = vec![0u8; 14 + 20 + 4];
        outer[12] = 8;
        outer[14] = 0x45;
        outer[15] = 0xb8;
        outer[22] = 63;
        outer[23] = 47;
        outer[26..30].copy_from_slice(&[10, 0, 0, 1]);
        outer[30..34].copy_from_slice(&[10, 0, 0, 2]);
        outer[36..38].copy_from_slice(&[0x88, 0xbe]);
        outer.extend_from_slice(&f);
        s.account(&outer, outer.len(), 4.0);
        assert_eq!((s.ours, s.gre), (2, 1));
        assert_eq!(
            s.gre_info.as_deref(),
            Some("outer 10.0.0.1 > 10.0.0.2 tos=0xb8 ttl=63 gre_flags=0x0000 gre_proto=0x88be len=166")
        );
    }

    #[test]
    fn result_lines_format() {
        let mut r = RxStats {
            ours: 3,
            first: 1.0,
            last: 2.0,
            bytes: 125_000,
            ..RxStats::default()
        };
        r.per[0] = 2;
        r.per[1] = 1;
        r.ce[1] = 1;
        assert_eq!(
            result_lines("Ethernet0", 3, &r, &[0, 46]),
            "RESULT if=Ethernet0 sent=3 rx=3 gre=0 other=0 dscp0=2 dscp46=1 ce46=1 rx_seconds=1.000 rx_mbps=1.0\n"
        );
    }
}

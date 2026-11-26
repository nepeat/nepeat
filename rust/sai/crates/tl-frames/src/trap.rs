//! `trap-frames`: the port of `tools/trap_frames.c`.
//!
//! `trap-frames TX_IF RX_IF KIND COUNT PPS [DMAC] [DST_IP]`: sends tagged frames of a kind out
//! one netdev and counts the ones carrying our tag on another.

use crate::bytes::{at, find, put, put_be16, put_u8};
use crate::common::{
    Clock, complain, dest_of, rx_socket, say, send_frame, short_timeout, tx_socket, up, usleep, wait_until,
};
use crate::cstd::{atof, atou, fmt_f, parse_mac};
use crate::csum::csum;
use crate::os::{PACKET_OUTGOING, Sock};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};

/// The tag every frame carries.
pub const TAG: [u8; 8] = *b"TRAPTEST";
/// The frame kinds; the kind byte in the tag is `0x40 +` the index.
pub const KINDS: [&str; 7] = ["plain", "lldp", "lacp", "eapol", "arp", "bgp", "ip2me"];

/// The kind byte of `kind`.
pub fn kind_id(kind: &str) -> Option<u8> {
    KINDS
        .iter()
        .position(|k| *k == kind)
        .and_then(|i| u8::try_from(i).ok())
        .map(|i| 0x40 + i)
}

/// `build()`: one frame of `kind`, sequence number `seq`.
pub fn build(kind: &str, id: u8, smac: [u8; 6], dmac_arg: [u8; 6], dip: u32, seq: u32) -> Vec<u8> {
    const LLDP: [u8; 6] = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x0e];
    const SLOW: [u8; 6] = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x02];
    const PAE: [u8; 6] = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x03];
    const BC: [u8; 6] = [0xff; 6];
    const UC: [u8; 6] = [0x02, 0x99, 0x99, 0x99, 0x99, 0x99];
    let mut f = vec![0u8; 256];
    let mut dst = UC;
    let mut ethertype: u16 = 0x88b5;
    let mut o = 14;
    let tcp_kind = kind == "bgp" || kind == "ip2me";
    match kind {
        "lldp" => {
            dst = LLDP;
            ethertype = 0x88cc;
        }
        "lacp" => {
            dst = SLOW;
            ethertype = 0x8809;
            put(&mut f, o, &[0x01, 0x01]);
            o += 2;
        }
        "eapol" => {
            dst = PAE;
            ethertype = 0x888e;
            put(&mut f, o, &[0x02, 0x01, 0, 0]);
            o += 4;
        }
        "arp" => {
            dst = BC;
            ethertype = 0x0806;
            let mut arp = [0u8; 28];
            put(&mut arp, 0, &[0, 1, 8, 0, 6, 4, 0, 1]);
            put(&mut arp, 8, &smac);
            put(&mut arp, 14, &[10, 0, 0, 2]);
            put(&mut arp, 24, &[10, 0, 0, 1]);
            put(&mut f, o, &arp);
            o += 28;
        }
        "bgp" | "ip2me" => {
            dst = dmac_arg;
            ethertype = 0x0800;
            o += build_tcp(&mut f, o, kind == "bgp", id, dip, seq);
        }
        _ => {}
    }
    put(&mut f, 0, &dst);
    put(&mut f, 6, &smac);
    put_be16(&mut f, 12, ethertype);
    if !tcp_kind {
        put(&mut f, o, &TAG);
        o += TAG.len();
        put_u8(&mut f, o, id);
        o += 1;
        put(&mut f, o, &seq.to_ne_bytes());
        o += 4;
    }
    f.truncate(o.max(64));
    f
}

/// The IPv4 / TCP SYN of the `bgp` and `ip2me` kinds at `o`; returns its length.
fn build_tcp(f: &mut [u8], o: usize, bgp: bool, id: u8, dip: u32, seq: u32) -> usize {
    let ip = o;
    let tcp = o + 20;
    put(f, ip, &[0x45]);
    put_u8(f, ip + 8, 64);
    put_u8(f, ip + 9, 6);
    put(f, ip + 12, &0x0a00_0002u32.to_be_bytes());
    put(f, ip + 16, &dip.to_be_bytes());
    put_be16(f, tcp, 40000);
    put_be16(f, tcp + 2, if bgp { 179 } else { 8888 });
    put(f, tcp + 12, &[0x50, 0x02, 0xff, 0xff]);
    let payload = 20 + TAG.len() + 1 + 4;
    let total = u16::try_from(20 + payload).unwrap_or(0);
    put_be16(f, ip + 2, total);
    put(f, tcp + 20, &TAG);
    put_u8(f, tcp + 28, id);
    put(f, tcp + 29, &seq.to_ne_bytes());
    let ip_sum = csum(f.get(ip..ip + 20).unwrap_or_default());
    put_be16(f, ip + 10, ip_sum);
    // TCP checksum over the pseudo header and the segment
    let mut ph = vec![0u8; 12 + payload];
    put(&mut ph, 0, f.get(ip + 12..ip + 20).unwrap_or_default());
    put_u8(&mut ph, 9, 6);
    put_be16(&mut ph, 10, u16::try_from(payload).unwrap_or(0));
    put(&mut ph, 12, f.get(tcp..tcp + payload).unwrap_or_default());
    let tcp_sum = csum(&ph);
    put_be16(f, tcp + 16, tcp_sum);
    20 + payload
}

/// What the receive side counted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    /// Frames with our tag and kind.
    pub ours: u64,
    /// Everything else received.
    pub other: u64,
    /// Bytes of our frames.
    pub bytes: u64,
}

impl Counts {
    /// Counts one received frame (`buf` is the whole receive buffer, `n` the frame length: the C
    /// tool reads the kind byte from the buffer even when the frame ends before it).
    pub fn account(&mut self, buf: &[u8], n: usize, id: u8) {
        let frame = buf.get(..n).unwrap_or(buf);
        match find(frame, &TAG) {
            Some(t) if at(buf, t + 8) == id => {
                self.ours += 1;
                self.bytes += u64::try_from(n).unwrap_or(0);
            }
            _ => self.other += 1,
        }
    }
}

fn rx_loop(sock: &Sock, stop: &AtomicBool, id: u8) -> Counts {
    let mut counts = Counts::default();
    let mut buf = vec![0u8; 10000];
    while !stop.load(Ordering::Relaxed) {
        let Ok((n, pkttype)) = sock.recv_from_packet(&mut buf, false) else {
            continue;
        };
        if n == 0 || pkttype == PACKET_OUTGOING {
            continue;
        }
        counts.account(&buf, n, id);
    }
    counts
}

/// The `RESULT` line.
#[allow(clippy::too_many_arguments)]
pub fn result_line(kind: &str, tx: &str, rx: &str, sent: u32, failed: u32, seconds: f64, counts: &Counts) -> String {
    let rate = if seconds > 0.0 { f64::from(sent) / seconds } else { 0.0 };
    format!(
        "RESULT kind={kind} tx={tx} rx={rx} sent={sent} send_errors={failed} seconds={} rate={}/s rx_ours={} rx_other={} rx_bytes={}\n",
        fmt_f(seconds, 3),
        fmt_f(rate, 1),
        counts.ours,
        counts.other,
        counts.bytes
    )
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
    kind: &'a str,
    id: u8,
    count: u32,
    pps: f64,
    dmac: [u8; 6],
    dip: u32,
}

fn parse(args: &[String]) -> Result<Opts<'_>, i32> {
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    if args.len() < 6 {
        complain(&format!("usage: {} TX_IF RX_IF KIND COUNT PPS [DMAC] [DST_IP]", arg(0)));
        return Err(2);
    }
    let mut dmac = [0u8; 6];
    let mut dip = 0;
    if args.len() > 6 {
        let Some(m) = parse_mac(arg(6)) else {
            complain("bad DMAC");
            return Err(2);
        };
        dmac = m;
    }
    if args.len() > 7 {
        dip = arg(7).parse::<Ipv4Addr>().map_err(|_| 2)?.into();
    }
    let Some(id) = kind_id(arg(3)) else {
        complain(&format!("unknown kind {}", arg(3)));
        return Err(2);
    };
    Ok(Opts {
        tx: arg(1),
        rx: arg(2),
        kind: arg(3),
        id,
        count: atou(arg(4)),
        pps: atof(arg(5)),
        dmac,
        dip,
    })
}

fn run(args: &[String]) -> Result<(), i32> {
    let o = parse(args)?;
    let (txi, txmac) = up(o.tx)?;
    let (rxi, _) = up(o.rx)?;
    let tx = tx_socket();
    let rx = rx_socket(rxi)?;
    short_timeout(&rx);
    let clock = Clock::default();
    let stop = AtomicBool::new(false);
    let (sent, failed, t0, t1, counts) = std::thread::scope(|scope| {
        let reader = scope.spawn(|| rx_loop(&rx, &stop, o.id));
        usleep(200_000);
        let (mut sent, mut failed) = (0u32, 0u32);
        let t0 = clock.now();
        for i in 0..o.count {
            let frame = build(o.kind, o.id, txmac, o.dmac, o.dip, i);
            if send_frame(tx.as_ref(), txi, dest_of(&frame), &frame) {
                sent += 1;
            } else {
                failed += 1;
            }
            if o.pps > 0.0 {
                wait_until(&clock, t0 + f64::from(i + 1) / o.pps, 50);
            }
        }
        let t1 = clock.now();
        usleep(1_500_000);
        stop.store(true, Ordering::Relaxed);
        (sent, failed, t0, t1, reader.join().unwrap_or_default())
    });
    say(&result_line(o.kind, o.tx, o.rx, sent, failed, t1 - t0, &counts));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMAC: [u8; 6] = [2, 0, 0, 0, 0, 1];

    #[test]
    fn kind_ids() {
        assert_eq!(kind_id("plain"), Some(0x40));
        assert_eq!(kind_id("ip2me"), Some(0x46));
        assert_eq!(kind_id("nope"), None);
    }

    #[test]
    fn lldp_frame_layout() {
        let f = build("lldp", 0x41, SMAC, [0; 6], 0, 7);
        assert_eq!(f.len(), 64);
        assert_eq!(&f[..6], &[0x01, 0x80, 0xc2, 0, 0, 0x0e]);
        assert_eq!(&f[6..12], &SMAC);
        assert_eq!(&f[12..14], &[0x88, 0xcc]);
        assert_eq!(&f[14..22], b"TRAPTEST");
        assert_eq!(f[22], 0x41);
        assert_eq!(&f[23..27], &7u32.to_ne_bytes());
        assert!(f[27..].iter().all(|b| *b == 0));
    }

    #[test]
    fn bgp_frame_checksums_verify() {
        let f = build("bgp", 0x45, SMAC, [2, 0, 0, 0, 1, 0x20], 0x0a63_0001, 3);
        assert_eq!(f.len(), 14 + 20 + 33);
        assert_eq!(csum(&f[14..34]), 0, "IPv4 header");
        let mut ph = f[26..34].to_vec();
        ph.extend_from_slice(&[0, 6, 0, 33]);
        ph.extend_from_slice(&f[34..]);
        assert_eq!(csum(&ph), 0, "TCP");
        assert_eq!(&f[36..38], &179u16.to_be_bytes());
        assert_eq!(&f[34 + 20..34 + 28], b"TRAPTEST");
    }

    #[test]
    fn counting_matches_kind_and_reads_stale_bytes_like_c() {
        let mut buf = vec![0u8; 100];
        let mut c = Counts::default();
        buf[10..18].copy_from_slice(&TAG);
        buf[18] = 0x42;
        c.account(&buf, 40, 0x42);
        c.account(&buf, 40, 0x41);
        // the tag at the very end of the frame: the kind byte is read from the buffer
        c.account(&buf, 18, 0x42);
        c.account(&buf, 12, 0x42);
        assert_eq!(
            c,
            Counts {
                ours: 2,
                other: 2,
                bytes: 58
            }
        );
    }

    #[test]
    fn result_line_format() {
        let c = Counts {
            ours: 50,
            other: 1,
            bytes: 3200,
        };
        assert_eq!(
            result_line("lldp", "Ethernet0", "Ethernet120", 50, 0, 0.5, &c),
            "RESULT kind=lldp tx=Ethernet0 rx=Ethernet120 sent=50 send_errors=0 seconds=0.500 rate=100.0/s rx_ours=50 rx_other=1 rx_bytes=3200\n"
        );
    }
}

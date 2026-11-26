//! `sflow-probe`: the port of `tools/sflow_probe.c`.
//!
//! `sflow-probe netdev TX_IF RX_IF COUNT PPS SIZE DMAC SRC_IP DST_IP` and
//! `sflow-probe psample TX_IF GROUP COUNT PPS SIZE DMAC SRC_IP DST_IP`: sends sequence-numbered
//! UDP frames out `TX_IF` and collects the samples the chip takes of them, either as raw sample
//! frames (Ethernet / IPv4 / GRE `0x8818`) on `RX_IF` or as messages of the generic-netlink family
//! `psample`; prints the sample count, the 1-in-N estimate with its 95 % interval, the sequence
//! gaps and the sample metadata.

use crate::bytes::{at, be32, find, ne16, ne32, put, put_be16, put_u8};
use crate::common::{Clock, complain, rx_socket, say, send_frame, tx_socket, up, usleep};
use crate::cstd::{atof, atoi, fmt_f, parse_mac, strerror, strtoul0};
use crate::csum::csum;
use crate::os::{PACKET_OUTGOING, Sock, if_indextoname};
use std::fmt::Write as _;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The tag in every frame.
pub const TAG: [u8; 8] = *b"SFLOWPRB";
/// Distinct sequence numbers kept at most.
pub const MAXS: usize = 4 << 20;

// psample attribute types (linux/psample.h)
const A_IIF: u16 = 0;
const A_OIF: u16 = 1;
const A_ORIGSIZE: u16 = 2;
const A_GROUP: u16 = 3;
const A_GSEQ: u16 = 4;
const A_RATE: u16 = 5;
const A_DATA: u16 = 6;
const A_PROTO: u16 = 14;
const NLA_TYPE_MASK: u16 = 0x3fff;
const NLMSG_ERROR: u16 = 2;
const NLMSG_MIN_TYPE: u16 = 0x10;
const GENL_ID_CTRL: u16 = 0x10;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const CTRL_ATTR_MCAST_GROUPS: u16 = 7;
const CTRL_ATTR_MCAST_GRP_NAME: u16 = 1;
const CTRL_ATTR_MCAST_GRP_ID: u16 = 2;

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

// ----------------------------------------------------------------------------------- netlink

/// The `CTRL_CMD_GETFAMILY` request for the family `psample` (`nl_req` of the C tool).
pub fn getfamily_request() -> Vec<u8> {
    let name = b"psample\0";
    let nla_len = 4 + name.len();
    let alen = align4(nla_len);
    let total = 16 + 4 + alen;
    let mut b = vec![0u8; 512];
    put(&mut b, 0, &u32::try_from(total).unwrap_or(0).to_ne_bytes());
    put(&mut b, 4, &GENL_ID_CTRL.to_ne_bytes());
    put(&mut b, 6, &1u16.to_ne_bytes()); // NLM_F_REQUEST
    put(&mut b, 8, &1u32.to_ne_bytes()); // seq
    put_u8(&mut b, 16, CTRL_CMD_GETFAMILY);
    put_u8(&mut b, 17, 1);
    put(&mut b, 20, &u16::try_from(nla_len).unwrap_or(0).to_ne_bytes());
    put(&mut b, 22, &CTRL_ATTR_FAMILY_NAME.to_ne_bytes());
    put(&mut b, 24, name);
    b.truncate(total);
    b
}

/// The attributes of `area`: (raw type, offset of the value in `area`, attribute length).
fn attrs(area: &[u8]) -> Vec<(u16, usize, usize)> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + 4 <= area.len() {
        let len = usize::from(ne16(area, off));
        if len < 4 {
            break;
        }
        out.push((ne16(area, off + 2), off + 4, len));
        off += align4(len);
    }
    out
}

/// The family id and the id of the multicast group `packets` in a `GETFAMILY` reply of `n` bytes
/// (`psample_ids`); `None` for an error reply or when either is missing.
pub fn parse_family_reply(msg: &[u8]) -> Option<(u16, u32)> {
    if ne16(msg, 4) == NLMSG_ERROR {
        return None;
    }
    let end = usize::try_from(ne32(msg, 0)).unwrap_or(0).min(msg.len());
    let area = msg.get(20..end).unwrap_or_default();
    let (mut fam, mut grp) = (0u16, 0u32);
    for (ty, voff, len) in attrs(area) {
        if ty == CTRL_ATTR_FAMILY_ID {
            fam = ne16(area, voff);
        }
        if ty & NLA_TYPE_MASK == CTRL_ATTR_MCAST_GROUPS {
            let nested = area.get(voff..(voff + len - 4).min(area.len())).unwrap_or_default();
            for (_, goff, glen) in attrs(nested) {
                let inner = nested
                    .get(goff..(goff + glen - 4).min(nested.len()))
                    .unwrap_or_default();
                let (mut id, mut name) = (0u32, None);
                for (ity, ioff, _) in attrs(inner) {
                    if ity == CTRL_ATTR_MCAST_GRP_ID {
                        id = ne32(inner, ioff);
                    }
                    if ity == CTRL_ATTR_MCAST_GRP_NAME {
                        let s = inner.get(ioff..).unwrap_or_default();
                        let nul = s.iter().position(|b| *b == 0).unwrap_or(s.len());
                        name = s.get(..nul);
                    }
                }
                if name == Some(b"packets") {
                    grp = id;
                }
            }
        }
    }
    (fam != 0 && grp != 0).then_some((fam, grp))
}

/// The messages of one receive: (type, whole message). A message that does not fit ends the walk.
pub fn nlmsgs(buf: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    let mut off = 0;
    while buf.len() >= off + 16 {
        let len = usize::try_from(ne32(buf, off)).unwrap_or(0);
        if len < 16 || len > buf.len() - off {
            break;
        }
        if let Some(m) = buf.get(off..off + len) {
            out.push((ne16(m, 4), m));
        }
        off += align4(len);
    }
    out
}

// ------------------------------------------------------------------------------------ state

/// The first psample message's metadata.
#[derive(Debug, Default, Clone)]
pub struct Meta0 {
    /// A sample was seen.
    pub have: bool,
    /// Input interface index.
    pub iif: u32,
    /// Output interface index.
    pub oif: u32,
    /// Sample rate.
    pub rate: u32,
    /// Original frame size.
    pub origsize: u32,
    /// Psample group.
    pub group: u32,
    /// Length of the data attribute.
    pub datalen: u32,
    /// Protocol.
    pub proto: u32,
    /// Group sequence number of the first and the last message.
    pub gseq0: u32,
    /// See `gseq0`.
    pub gseq1: u32,
}

/// Everything the receive side collects.
#[derive(Debug, Clone)]
pub struct Probe {
    /// Sequence numbers of the sampled frames (unsorted).
    pub seqs: Vec<u32>,
    /// Samples that were not ours or were malformed.
    pub other: u32,
    /// `ENOBUFS` receives.
    pub nobufs: u32,
    /// Psample messages accepted.
    pub nmsg: u32,
    /// Size of the frames sent.
    pub fsize: usize,
    /// Only this psample group (0: any).
    pub want_group: u32,
    /// Hex-dump the first sample.
    pub hex: bool,
    /// First psample message.
    pub m0: Meta0,
    /// Messages differing from the first in input interface, output interface, rate.
    pub diff_iif: u32,
    /// See `diff_iif`.
    pub diff_oif: u32,
    /// See `diff_iif`.
    pub diff_rate: u32,
    /// Messages whose original size is not the frame size.
    pub diff_orig: u32,
    /// Smallest and largest rate seen.
    pub rate_min: u32,
    /// See `rate_min`.
    pub rate_max: u32,
    /// Netdev mode: the first sample's leading bytes.
    pub hdr0: Vec<u8>,
    /// Offset of the sampled frame in the first sample (-1: no tag).
    pub inner_off0: i32,
    /// Length of the first sample.
    pub sample_len0: i32,
    /// The sFlow header's counter words, first and last sample.
    pub cnt_first: [u32; 2],
    /// See `cnt_first`.
    pub cnt_last: [u32; 2],
    /// Samples with a counter pair.
    pub ncnt: u32,
}

impl Probe {
    /// An empty probe for frames of `fsize` bytes.
    pub fn new(fsize: usize, want_group: u32, hex: bool) -> Self {
        Self {
            seqs: Vec::new(),
            other: 0,
            nobufs: 0,
            nmsg: 0,
            fsize,
            want_group,
            hex,
            m0: Meta0::default(),
            diff_iif: 0,
            diff_oif: 0,
            diff_rate: 0,
            diff_orig: 0,
            rate_min: u32::MAX,
            rate_max: 0,
            hdr0: Vec::new(),
            inner_off0: -1,
            sample_len0: 0,
            cnt_first: [0; 2],
            cnt_last: [0; 2],
            ncnt: 0,
        }
    }

    /// `take_frame`: the sampled frame's tag and sequence number.
    pub fn take_frame(&mut self, f: &[u8]) {
        match find(f, &TAG) {
            Some(t) if t + TAG.len() + 4 <= f.len() => {
                if self.seqs.len() < MAXS {
                    self.seqs.push(ne32(f, t + TAG.len()));
                }
            }
            _ => self.other += 1,
        }
    }

    /// `netdev_sample`: one raw sample frame (Ethernet [802.1Q] / IPv4 / GRE 0x8818 / 16 B header / frame).
    pub fn netdev_sample(&mut self, b: &[u8]) {
        let n = b.len();
        let l2 = if n > 18 && at(b, 12) == 0x81 && at(b, 13) == 0x00 {
            18
        } else {
            14
        };
        if n < l2 + 24 || at(b, l2 - 2) != 0x08 || at(b, l2 - 1) != 0x00 || at(b, l2 + 9) != 47 {
            self.other += 1;
            return;
        }
        let g = l2 + usize::from(at(b, l2) & 15) * 4;
        if g + 28 > n || at(b, g + 2) != 0x88 || at(b, g + 3) != 0x18 {
            self.other += 1;
            return;
        }
        if self.hdr0.is_empty() {
            self.hdr0 = b.get(..(g + 28).min(64)).unwrap_or_default().to_vec();
            // the inner frame starts 42 B before our tag (14 + 20 + 8)
            self.inner_off0 = find(b, &TAG).map_or(-1, |t| i32::try_from(t).unwrap_or(0) - 42);
            self.sample_len0 = i32::try_from(n).unwrap_or(0);
            if self.hex {
                say(&dump("sample", b));
            }
        }
        let (w2, w3) = (be32(b, g + 16), be32(b, g + 20));
        if self.ncnt == 0 {
            self.cnt_first = [w2, w3];
        }
        self.ncnt += 1;
        self.cnt_last = [w2, w3];
        self.take_frame(b.get(g + 28..).unwrap_or_default());
    }

    /// `psample_msg`: the attributes of one psample message.
    pub fn psample_msg(&mut self, area: &[u8]) {
        let (mut iif, mut oif, mut rate, mut orig, mut group, mut proto) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
        let mut gseq = 0u32;
        let mut data: Option<&[u8]> = None;
        for (ty, voff, len) in attrs(area) {
            match ty & NLA_TYPE_MASK {
                A_IIF => iif = u32::from(ne16(area, voff)),
                A_OIF => oif = u32::from(ne16(area, voff)),
                A_PROTO => proto = u32::from(ne16(area, voff)),
                A_ORIGSIZE => orig = ne32(area, voff),
                A_GROUP => group = ne32(area, voff),
                A_GSEQ => gseq = ne32(area, voff),
                A_RATE => rate = ne32(area, voff),
                A_DATA => data = Some(area.get(voff..(voff + len - 4).min(area.len())).unwrap_or_default()),
                _ => {}
            }
        }
        if self.want_group != 0 && group != self.want_group {
            return;
        }
        self.nmsg += 1;
        if self.m0.have {
            self.diff_iif += u32::from(iif != self.m0.iif);
            self.diff_oif += u32::from(oif != self.m0.oif);
            self.diff_rate += u32::from(rate != self.m0.rate);
        } else {
            let dlen = data.map_or(0, |d| u32::try_from(d.len()).unwrap_or(0));
            self.m0 = Meta0 {
                have: true,
                iif,
                oif,
                rate,
                origsize: orig,
                group,
                datalen: dlen,
                proto,
                gseq0: gseq,
                gseq1: gseq,
            };
            if self.hex {
                if let Some(d) = data {
                    say(&dump("psample data", d));
                }
            }
        }
        self.m0.gseq1 = gseq;
        self.rate_min = self.rate_min.min(rate);
        self.rate_max = self.rate_max.max(rate);
        match data {
            Some(d) => {
                if self.fsize != 0 && usize::try_from(orig).ok() != Some(self.fsize) {
                    self.diff_orig += 1;
                }
                self.take_frame(d);
            }
            None => self.other += 1,
        }
    }

    /// One receive of the psample socket: counts every message in it.
    pub fn psample_datagram(&mut self, buf: &[u8]) {
        for (ty, msg) in nlmsgs(buf) {
            if ty == NLMSG_ERROR || ty < NLMSG_MIN_TYPE {
                continue;
            }
            self.psample_msg(msg.get(20..).unwrap_or_default());
        }
    }
}

/// `dump()`: the first 160 bytes as hex.
pub fn dump(what: &str, b: &[u8]) -> String {
    let mut s = format!("HEX {what} len={}\n", b.len());
    for (k, chunk) in b.chunks(16).take(10).enumerate() {
        let _ = write!(s, "  {:04x} ", k * 16);
        for byte in chunk {
            let _ = write!(s, " {byte:02x}");
        }
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------------------------- report

/// The lines printed after the run (everything from `SENT` on).
#[allow(clippy::too_many_arguments)]
pub fn report(mode: &str, psample: bool, tx: &str, sent: u32, failed: u32, t0: f64, t1: f64, p: &Probe) -> String {
    let span = t1 - t0;
    let pps = if t1 > t0 { f64::from(sent) / span } else { 0.0 };
    let mut s = format!(
        "SENT tx={tx} sent={sent} send_errors={failed} seconds={} pps={} size={}\n",
        fmt_f(span, 3),
        fmt_f(pps, 0),
        p.fsize
    );
    let mut seqs = p.seqs.clone();
    seqs.sort_unstable();
    seqs.dedup();
    let uniq = seqs.len();
    let dups = p.seqs.len() - uniq;
    let _ = write!(s, "SAMPLES mode={mode} n={uniq} other={} dups={dups}", p.other);
    if psample {
        let _ = write!(s, " messages={} enobufs={}", p.nmsg, p.nobufs);
    }
    let n = crate::usize_f64(uniq);
    if sent != 0 && uniq != 0 {
        let lo = n + 1.96 * n.sqrt();
        let hi = n - 1.96 * n.sqrt();
        let f_sent = f64::from(sent);
        let _ = write!(
            s,
            " one_in={} ci95={}-{}",
            fmt_f(f_sent / n, 1),
            fmt_f(f_sent / lo, 1),
            if hi > 0.0 { "" } else { "inf" }
        );
        if hi > 0.0 {
            s.push_str(&fmt_f(f_sent / hi, 1));
        }
    }
    s.push('\n');
    s.push_str(&gaps_line(&seqs));
    if psample && p.m0.have {
        s.push_str(&psample_meta(p));
    }
    if !psample && !p.hdr0.is_empty() {
        s.push_str(&netdev_meta(p));
    }
    s
}

fn gaps_line(seqs: &[u32]) -> String {
    let uniq = seqs.len();
    if uniq <= 2 {
        return String::new();
    }
    let (mut sum, mut sum2) = (0.0f64, 0.0f64);
    let (mut mn, mut mx) = (u32::MAX, 0u32);
    for w in seqs.windows(2) {
        let d = match *w {
            [a, b] => b.wrapping_sub(a),
            _ => 0,
        };
        sum += f64::from(d);
        sum2 += f64::from(d) * f64::from(d);
        mn = mn.min(d);
        mx = mx.max(d);
    }
    let k = crate::usize_f64(uniq - 1);
    let mean = sum / k;
    let sd = (sum2 / k - mean * mean).sqrt();
    let cv = if mean > 0.0 { sd / mean } else { 0.0 };
    format!(
        "GAPS n={} mean={} sd={} cv={} min={mn} max={mx} first_seq={} last_seq={}\n",
        fmt_f(k, 0),
        fmt_f(mean, 1),
        fmt_f(sd, 1),
        fmt_f(cv, 2),
        seqs.first().copied().unwrap_or(0),
        seqs.last().copied().unwrap_or(0)
    )
}

fn ifname(ix: u32) -> String {
    if ix == 0 {
        "-".to_owned()
    } else {
        if_indextoname(ix).unwrap_or_else(|| "-".to_owned())
    }
}

fn psample_meta(p: &Probe) -> String {
    let m = &p.m0;
    format!(
        "META iif={}({}) oif={}({}) rate={} rate_range={}-{} origsize={} datalen={} group={} proto=0x{:04x} group_seq={}-{} diff_iif={} diff_oif={} diff_rate={} origsize_not_{}={}\n",
        m.iif,
        ifname(m.iif),
        m.oif,
        ifname(m.oif),
        m.rate,
        p.rate_min,
        p.rate_max,
        m.origsize,
        m.datalen,
        m.group,
        m.proto,
        m.gseq0,
        m.gseq1,
        p.diff_iif,
        p.diff_oif,
        p.diff_rate,
        p.fsize,
        p.diff_orig
    )
}

fn hex_range(b: &[u8], from: usize, to: usize) -> String {
    (from..to).fold(String::new(), |mut s, i| {
        let _ = write!(s, "{:02x}", at(b, i));
        s
    })
}

fn netdev_meta(p: &Probe) -> String {
    let h = &p.hdr0;
    let l2 = if at(h, 12) == 0x81 { 18 } else { 14 };
    let g = l2 + usize::from(at(h, l2) & 15) * 4;
    let mut s = format!(
        "META sample_len={} inner_offset={} gre={} base={} sflow_hdr={}",
        p.sample_len0,
        p.inner_off0,
        hex_range(h, g, g + 4),
        hex_range(h, g + 4, g + 8),
        hex_range(h, g + 8, g + 24)
    );
    let (w0, w1) = (be32(h, g + 8), be32(h, g + 12));
    let _ = writeln!(
        s,
        " ext_type={} dir={} sp={} sp2={}",
        w0 >> 26,
        (w0 >> 15) & 1,
        w0 & 0x7fff,
        w1 & 0x7fff
    );
    let ds = p.cnt_last[0].wrapping_sub(p.cnt_first[0]);
    let dp = p.cnt_last[1].wrapping_sub(p.cnt_first[1]);
    let one_in = if ds != 0 { f64::from(dp) / f64::from(ds) } else { 0.0 };
    let lost = i32::from_ne_bytes(ds.to_ne_bytes())
        .wrapping_add(1)
        .wrapping_sub(i32::from_ne_bytes(p.ncnt.to_ne_bytes()));
    let _ = writeln!(
        s,
        "CHIP samples={}-{} packets={}-{} d_samples={ds} d_packets={dp} one_in={} lost={lost}",
        p.cnt_first[0],
        p.cnt_last[0],
        p.cnt_first[1],
        p.cnt_last[1],
        fmt_f(one_in, 1)
    );
    s
}

// ---------------------------------------------------------------------------------- sending

/// The frame template: IPv4 / UDP of `size` bytes, the tag at the start of the payload; the
/// sequence number goes in with [`set_seq`].
pub fn build_frame(size: usize, dmac: [u8; 6], smac: [u8; 6], sip: [u8; 4], dip: [u8; 4]) -> Vec<u8> {
    let mut f = vec![0u8; size];
    put(&mut f, 0, &dmac);
    put(&mut f, 6, &smac);
    put_u8(&mut f, 12, 0x08);
    put_u8(&mut f, 14, 0x45);
    put_be16(&mut f, 16, u16::try_from((size - 14) & 0xffff).unwrap_or(0));
    put_u8(&mut f, 22, 64);
    put_u8(&mut f, 23, 17);
    put(&mut f, 26, &sip);
    put(&mut f, 30, &dip);
    let ic = csum(f.get(14..34).unwrap_or_default());
    put_be16(&mut f, 24, ic);
    put_be16(&mut f, 34, 40001);
    put_be16(&mut f, 36, 6343);
    put_be16(&mut f, 38, u16::try_from((size - 34) & 0xffff).unwrap_or(0));
    put(&mut f, 42, &TAG);
    f
}

/// Writes the sequence number into the frame.
pub fn set_seq(f: &mut [u8], seq: u32) {
    put(f, 50, &seq.to_ne_bytes());
}

// ------------------------------------------------------------------------------------ main

/// What the receive thread reads from.
enum Source {
    Netdev(Sock),
    Psample(Sock),
}

fn rx_loop(src: &Source, stop: &AtomicBool, mut probe: Probe) -> Probe {
    let mut buf = vec![0u8; 70000];
    while !stop.load(Ordering::Relaxed) {
        match src {
            Source::Psample(s) => match s.recv(&mut buf) {
                Err(e) => {
                    if e.raw_os_error() == Some(libc_enobufs()) {
                        probe.nobufs += 1;
                    }
                }
                Ok(n) => probe.psample_datagram(buf.get(..n).unwrap_or_default()),
            },
            Source::Netdev(s) => {
                let Ok((n, pkttype)) = s.recv_from_packet(&mut buf, false) else {
                    continue;
                };
                if n == 0 || pkttype == PACKET_OUTGOING {
                    continue;
                }
                probe.netdev_sample(buf.get(..n).unwrap_or_default());
            }
        }
    }
    probe
}

/// `ENOBUFS`.
fn libc_enobufs() -> i32 {
    crate::os::ENOBUFS
}

/// `main`: the exit status.
pub fn main(args: &[String]) -> i32 {
    run(args).map_or_else(|code| code, |()| 0)
}

struct Opts<'a> {
    mode: &'a str,
    psample: bool,
    tx: &'a str,
    count: u32,
    pps: f64,
    size: usize,
    dmac: [u8; 6],
    sip: [u8; 4],
    dip: [u8; 4],
    hex: bool,
}

fn parse(args: &[String]) -> Result<Opts<'_>, i32> {
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    if args.len() < 10 || (arg(1) != "netdev" && arg(1) != "psample") {
        complain(&format!(
            "usage: {} netdev|psample TX_IF RX_IF|GROUP COUNT PPS SIZE DMAC SRC_IP DST_IP",
            arg(0)
        ));
        return Err(2);
    }
    Ok(Opts {
        mode: arg(1),
        psample: arg(1) == "psample",
        tx: arg(2),
        count: u32::try_from(strtoul0(arg(4)) & 0xffff_ffff).unwrap_or(0),
        pps: atof(arg(5)),
        size: crate::qos::clamp_size(atoi(arg(6))),
        dmac: parse_mac(arg(7)).ok_or(2)?,
        sip: arg(8).parse::<Ipv4Addr>().map_err(|_| 2)?.octets(),
        dip: arg(9).parse::<Ipv4Addr>().map_err(|_| 2)?.octets(),
        hex: std::env::var("HEX").is_ok_and(|v| atoi(&v) != 0),
    })
}

/// Opens the psample socket and joins the group; `Err(3)` is the "no psample family" exit.
fn open_psample(want_group: u32) -> Result<Sock, i32> {
    let fail = |what: &str, e: &std::io::Error| {
        complain(&format!("{what}: {}", strerror(e)));
        2
    };
    let s = Sock::netlink_generic().map_err(|e| fail("netlink", &e))?;
    s.bind_netlink().map_err(|e| fail("netlink", &e))?;
    let reply = s.send(&getfamily_request()).and_then(|_| {
        let mut out = vec![0u8; 8192];
        let n = s.recv(&mut out)?;
        out.truncate(n);
        Ok(out)
    });
    let ids = reply
        .ok()
        .filter(|r| !r.is_empty())
        .and_then(|r| parse_family_reply(&r));
    let Some((fam, grp)) = ids else {
        say("SAMPLES n=0 error=no-psample-family\n");
        return Err(3);
    };
    s.join_netlink_group(grp).map_err(|e| fail("join", &e))?;
    say(&format!(
        "PSAMPLE family={fam} group_packets={grp} filter_group={want_group}\n"
    ));
    Ok(s)
}

fn run(args: &[String]) -> Result<(), i32> {
    let o = parse(args)?;
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    let want_group = if o.psample {
        u32::try_from(strtoul0(arg(3)) & 0xffff_ffff).unwrap_or(0)
    } else {
        0
    };
    let sock = if o.psample {
        open_psample(want_group)?
    } else {
        let (ix, _) = up(arg(3))?;
        rx_socket(ix)?
    };
    let _ = sock.force_recv_buffer(256 << 20);
    let _ = sock.set_recv_timeout(Duration::from_millis(100));
    let source = if o.psample {
        Source::Psample(sock)
    } else {
        Source::Netdev(sock)
    };
    let clock = Clock::default();
    let stop = AtomicBool::new(false);
    let probe = Probe::new(o.size, want_group, o.hex);
    let (sent, failed, t0, t1, probe) = std::thread::scope(|scope| -> Result<_, i32> {
        let reader = scope.spawn(|| rx_loop(&source, &stop, probe));
        usleep(200_000);
        let (mut sent, mut failed) = (0u32, 0u32);
        let mut t0 = clock.now();
        let mut t1 = t0;
        let sending = if o.count > 0 { up(o.tx).map(Some) } else { Ok(None) };
        match sending {
            Ok(Some((txi, txmac))) => {
                let txs = tx_socket();
                let mut f = build_frame(o.size, o.dmac, txmac, o.sip, o.dip);
                t0 = clock.now();
                for i in 0..o.count {
                    set_seq(&mut f, i);
                    if send_frame(txs.as_ref(), txi, o.dmac, &f) {
                        sent += 1;
                    } else {
                        failed += 1;
                    }
                    if o.pps > 0.0 {
                        crate::common::wait_until(&clock, t0 + f64::from(i + 1) / o.pps, 10);
                    }
                }
                t1 = clock.now();
                usleep(1_500_000);
            }
            Ok(None) => {
                let listen = std::env::var("LISTEN").map_or(10.0, |v| atof(&v));
                usleep(f64_to_u64(listen * 1e6));
            }
            Err(code) => {
                stop.store(true, Ordering::Relaxed);
                let _ = reader.join();
                return Err(code);
            }
        }
        stop.store(true, Ordering::Relaxed);
        Ok((
            sent,
            failed,
            t0,
            t1,
            reader.join().unwrap_or_else(|_| Probe::new(o.size, want_group, o.hex)),
        ))
    })?;
    say(&report(o.mode, o.psample, o.tx, sent, failed, t0, t1, &probe));
    Ok(())
}

/// `(useconds_t)x` for a non-negative duration.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f64_to_u64(x: f64) -> u64 {
    if x.is_nan() || x <= 0.0 { 0 } else { x as u64 }
}

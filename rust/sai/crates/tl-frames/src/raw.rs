//! `raw-frames`: the port of `tools/raw_frames.c`.
//!
//! `raw-frames TX_IF RX_IF HEX COUNT PPS`: sends one given Ethernet frame (hex) COUNT times, the
//! source MAC replaced by the TX netdev's, a tag appended; counts tagged copies on `RX_IF`.

use crate::bytes::{find, ne32, put};
use crate::common::{
    Clock, complain, dest_of, rx_socket, say, send_frame, short_timeout, tx_socket, up, usleep, wait_until,
};
use crate::cstd::{atof, atou, fmt_f};
use crate::os::{PACKET_OUTGOING, Sock};
use std::sync::atomic::{AtomicBool, Ordering};

/// The tag appended after the given bytes.
pub const TAG: [u8; 8] = *b"RAWFRAME";
/// The C tool's frame buffer: the hex is cut at this size minus 64.
const FRAME_BUF: usize = 2048;

/// Why the hex string was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum HexError {
    /// A pair of characters that is not two hex digits.
    Bad,
    /// Fewer than the 14 bytes of an Ethernet header.
    Short,
}

/// The frame bytes of `hex` (pairs of hex digits; an odd last character is ignored; at most
/// 1984 bytes are taken).
pub fn parse_hex(hex: &str) -> Result<Vec<u8>, HexError> {
    let mut out = Vec::new();
    for pair in hex.as_bytes().chunks_exact(2) {
        if out.len() >= FRAME_BUF - 64 {
            break;
        }
        let [hi, lo] = *pair else { break };
        let digit = |c: u8| char::from(c).to_digit(16);
        match (digit(hi), digit(lo)) {
            (Some(h), Some(l)) => out.push(u8::try_from(h << 4 | l).unwrap_or(0)),
            _ => return Err(HexError::Bad),
        }
    }
    if out.len() < 14 { Err(HexError::Short) } else { Ok(out) }
}

/// Frame number `i`: the given bytes (`base`), the tag, the run id and the sequence number, padded
/// to 60 bytes.
pub fn build(base: &[u8], run_id: u32, i: u32) -> Vec<u8> {
    let mut f = base.to_vec();
    f.extend_from_slice(&TAG);
    f.extend_from_slice(&run_id.to_ne_bytes());
    f.extend_from_slice(&i.to_ne_bytes());
    if f.len() < 60 {
        f.resize(60, 0);
    }
    f
}

/// The frame as it goes out: the source MAC replaced.
pub fn with_source(mut frame: Vec<u8>, smac: [u8; 6]) -> Vec<u8> {
    put(&mut frame, 6, &smac);
    frame
}

/// Receive counters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    /// Frames with our tag and run id.
    pub ours: u64,
    /// Everything else.
    pub other: u64,
}

impl Counts {
    /// Counts one received frame.
    pub fn account(&mut self, frame: &[u8], run_id: u32) {
        match find(frame, &TAG) {
            Some(t) if t + 12 <= frame.len() && ne32(frame, t + 8) == run_id => self.ours += 1,
            _ => self.other += 1,
        }
    }
}

fn rx_loop(sock: &Sock, stop: &AtomicBool, run_id: u32) -> Counts {
    let mut counts = Counts::default();
    let mut buf = vec![0u8; 10000];
    while !stop.load(Ordering::Relaxed) {
        let Ok((n, pkttype)) = sock.recv_from_packet(&mut buf, false) else {
            continue;
        };
        if n == 0 || pkttype == PACKET_OUTGOING {
            continue;
        }
        counts.account(buf.get(..n).unwrap_or_default(), run_id);
    }
    counts
}

/// The `RESULT` line.
#[allow(clippy::too_many_arguments)]
pub fn result_line(tx: &str, rx: &str, len: usize, sent: u32, failed: u32, seconds: f64, counts: &Counts) -> String {
    format!(
        "RESULT raw tx={tx} rx={rx} len={len} sent={sent} send_errors={failed} seconds={} rx_ours={} rx_other={}\n",
        fmt_f(seconds, 3),
        counts.ours,
        counts.other
    )
}

/// `main`: the exit status.
pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// `getpid() ^ time(NULL)`.
fn make_run_id() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    std::process::id() ^ u32::try_from(secs & 0xffff_ffff).unwrap_or(0)
}

fn run(args: &[String]) -> Result<(), i32> {
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    if args.len() < 6 {
        complain(&format!("usage: {} TX_IF RX_IF HEX COUNT PPS", arg(0)));
        return Err(2);
    }
    let (txn, rxn) = (arg(1), arg(2));
    let (count, pps) = (atou(arg(4)), atof(arg(5)));
    let base = match parse_hex(arg(3)) {
        Ok(b) => b,
        Err(HexError::Bad) => {
            complain("bad hex");
            return Err(2);
        }
        Err(HexError::Short) => {
            complain("frame shorter than an Ethernet header");
            return Err(2);
        }
    };
    let (txi, txmac) = up(txn)?;
    let (rxi, _) = up(rxn)?;
    let base = with_source(base, txmac);
    let run_id = make_run_id();
    let tx = tx_socket();
    let rx = rx_socket(rxi)?;
    short_timeout(&rx);
    let clock = Clock::default();
    let stop = AtomicBool::new(false);
    let (sent, failed, t0, t1, counts) = std::thread::scope(|scope| {
        let reader = scope.spawn(|| rx_loop(&rx, &stop, run_id));
        usleep(200_000);
        let (mut sent, mut failed) = (0u32, 0u32);
        let t0 = clock.now();
        for i in 0..count {
            let frame = build(&base, run_id, i);
            if send_frame(tx.as_ref(), txi, dest_of(&base), &frame) {
                sent += 1;
            } else {
                failed += 1;
            }
            if pps > 0.0 {
                wait_until(&clock, t0 + f64::from(i + 1) / pps, 50);
            }
        }
        let t1 = clock.now();
        usleep(1_500_000);
        stop.store(true, Ordering::Relaxed);
        (sent, failed, t0, t1, reader.join().unwrap_or_default())
    });
    say(&result_line(txn, rxn, base.len(), sent, failed, t1 - t0, &counts));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("00"), Err(HexError::Short));
        assert_eq!(parse_hex("zz00"), Err(HexError::Bad));
        let h = "02000000012000000000000008004500";
        let f = parse_hex(h).unwrap();
        assert_eq!(f.len(), 16);
        assert_eq!(f[0], 2);
        // an odd trailing character is ignored
        assert_eq!(parse_hex(&format!("{h}f")).unwrap().len(), 16);
        // cut at 1984 bytes
        assert_eq!(parse_hex(&"ab".repeat(3000)).unwrap().len(), 1984);
    }

    #[test]
    fn frames_are_tagged_and_padded() {
        let base = vec![1u8; 20];
        let f = build(&base, 0x0102_0304, 9);
        assert_eq!(f.len(), 60);
        assert_eq!(&f[20..28], b"RAWFRAME");
        assert_eq!(&f[28..32], &0x0102_0304u32.to_ne_bytes());
        assert_eq!(&f[32..36], &9u32.to_ne_bytes());
        assert!(f[36..].iter().all(|b| *b == 0));
        let long = build(&[1u8; 100], 1, 2);
        assert_eq!(long.len(), 116);
    }

    #[test]
    fn counting() {
        let mut c = Counts::default();
        c.account(&build(&[0u8; 20], 5, 0), 5);
        c.account(&build(&[0u8; 20], 6, 0), 5);
        c.account(&[0u8; 30], 5);
        // the tag cut short before the run id
        c.account(&build(&[0u8; 20], 5, 0)[..30], 5);
        assert_eq!(c, Counts { ours: 1, other: 3 });
    }
}

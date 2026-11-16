#!/usr/bin/env python3
"""Known-stream serial loss check (consoled on the DMC-12, or any reader).

  stream-check.py gen  [--lines N] [--run ID]          > stream.txt   (the sender's file)
  stream-check.py check CAPTURE [--run ID] [--json]                  (the analyser)

gen: a BEGIN marker, N numbered 64-byte lines, an END marker. Line i is
"%07d <50 chars derived from i> %04x\n" (the last field: crc32 of the rest & 0xffff), so
every line differs and can be checked on its own. Send it nonstop to the target's console
tty, e.g. on the CS8210: cat /dev/shm/stream.txt > /dev/ttyS0 (onlcr there makes each \n
\r\n on the wire; check detects that from the BEGIN line).

check: finds the BEGIN marker of run ID (default: the last one) in the capture, regenerates
the expected bytes, and counts what arrived: intact lines, and for each stretch between two
intact lines, expected vs received bytes (lost = expected - received). Exit 0 only when the
stream arrived byte-exact through the END marker.
"""
import argparse, json, re, sys, zlib

ALPH = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
BAUD_BPS = 11520  # 115200 8N1


def line(i, eol=b"\n"):
    p = bytes(ALPH[(i * 7 + j * 13 + (i >> 4) * (j + 1)) % len(ALPH)] for j in range(50))
    body = b"%07d " % i + p
    return body + b" %04x" % (zlib.crc32(body) & 0xFFFF) + eol


def begin(run, n):
    return b"=== STREAMCHECK BEGIN run=%s lines=%d ===" % (run.encode(), n)


def end(run):
    return b"=== STREAMCHECK END run=%s ===" % run.encode()


def gen(a):
    out = sys.stdout.buffer
    out.write(b"\n" + begin(a.run, a.lines) + b"\n")
    for i in range(a.lines):
        out.write(line(i))
    out.write(end(a.run) + b"\n")


def check(a):
    data = open(a.capture, "rb").read()
    pat = rb"=== STREAMCHECK BEGIN run=" + (re.escape(a.run.encode()) if a.run else rb"(\S+)") + rb" lines=(\d+) ===(\r?\n)"
    ms = list(re.finditer(pat, data))
    if not ms:
        print("no BEGIN marker%s in %s" % (" for run " + a.run if a.run else "", a.capture))
        return 3
    m = ms[-1]
    run = a.run or m.group(1).decode()
    n = int(m.group(m.lastindex - 1))
    eol = m.group(m.lastindex)
    start = m.end()
    e = data.find(end(run), start)
    end_seen = e >= 0
    region = data[start:e if end_seen else len(data)]
    if end_seen and region.endswith(eol) is False:
        pass  # the END marker's own line start; whatever precedes it is counted below
    exp = [line(i, eol) for i in range(n)]
    explen = sum(map(len, exp))
    # intact lines: exactly the expected bytes, numbers increasing
    anchors = []  # (i, offset of the line in region)
    pos = 0
    last = -1
    for raw in region.split(b"\n"):
        seg = raw + b"\n"
        mm = re.match(rb"(\d{7}) ", raw)
        if mm:
            i = int(mm.group(1))
            if last < i < n and seg == exp[i]:
                anchors.append((i, pos))
                last = i
        pos += len(seg)
    # stretches between intact lines (and before the first, after the last)
    events = []
    prev_i, prev_end = -1, 0
    rlen = len(region)
    for i, off in anchors + [(n, rlen)]:
        e_bytes = sum(len(exp[k]) for k in range(prev_i + 1, i))
        r_bytes = off - prev_end
        if e_bytes or r_bytes:
            events.append({
                "after_line": prev_i, "before_line": i if i < n else None,
                "expected": e_bytes, "received": r_bytes, "lost": e_bytes - r_bytes,
                "at_s": round(sum(len(exp[k]) for k in range(0, prev_i + 1)) / BAUD_BPS, 2),
            })
        if i < n:
            prev_i, prev_end = i, off + len(exp[i])
    received = rlen
    lost = sum(ev["lost"] for ev in events)
    sizes = sorted(ev["lost"] for ev in events)
    res = {
        "run": run, "lines": n, "eol": "crlf" if eol == b"\r\n" else "lf",
        "expected_bytes": explen, "received_bytes": received, "lost_bytes": lost,
        "end_marker": end_seen, "intact_lines": len(anchors), "bad_lines": n - len(anchors),
        "loss_events": len(events), "stream_seconds": round(explen / BAUD_BPS, 1),
        "lost_per_event": {"min": sizes[0], "median": sizes[len(sizes) // 2], "max": sizes[-1]} if sizes else None,
        "first_events": events[:15],
    }
    if a.json:
        print(json.dumps(res, indent=1))
    else:
        print("run %s: %d lines, %d bytes expected (%s, ~%.0f s at 115200), %d received, %d lost; "
              "END marker %s" % (run, n, explen, res["eol"], res["stream_seconds"], received, lost,
                                  "seen" if end_seen else "MISSING"))
        print("intact lines %d/%d (%.2f%% bad), %d damaged stretches (%d same length: corrupted, not lost)" % (
            len(anchors), n, 100.0 * (n - len(anchors)) / max(n, 1), len(events),
            sum(1 for ev in events if ev["lost"] == 0)))
        if sizes:
            print("bytes lost per event: min %d median %d max %d" % (sizes[0], sizes[len(sizes) // 2], sizes[-1]))
            hist = {}
            for s in sizes:
                hist[s] = hist.get(s, 0) + 1
            top = sorted(hist.items(), key=lambda kv: -kv[1])[:12]
            print("most common loss sizes (bytes: count): " + ", ".join("%d: %d" % kv for kv in top))
            if len(events) > 1:
                gaps = [round(events[k + 1]["at_s"] - events[k]["at_s"], 2) for k in range(len(events) - 1)]
                gs = sorted(gaps)
                print("seconds between events: min %.2f median %.2f max %.2f" % (gs[0], gs[len(gs) // 2], gs[-1]))
            for ev in events[:15]:
                print("  at ~%6.2f s after line %7d: expected %6d received %6d lost %6d" % (
                    ev["at_s"], ev["after_line"], ev["expected"], ev["received"], ev["lost"]))
    return 0 if (end_seen and not events) else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--lines", type=int, default=32768)
    g.add_argument("--run", default="1")
    c = sub.add_parser("check")
    c.add_argument("capture")
    c.add_argument("--run")
    c.add_argument("--json", action="store_true")
    a = ap.parse_args()
    if a.cmd == "gen":
        gen(a)
        return 0
    return check(a)


if __name__ == "__main__":
    sys.exit(main())

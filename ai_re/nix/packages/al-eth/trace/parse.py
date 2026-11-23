#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0
"""Turn /proc/al_eth_trace (al_eth_trace on the hardware) into a difftest trace.

  parse.py al_eth_trace.txt -o hw-trace.txt [--datapath dp.txt]

The control path (process context: probe, open, close, ...) becomes a trace
in the format of ../difftest/mock.h with one scenario, "hw", whose calls come
from the entry marks; ../difftest/difftest.rs --hw --board replays it against the
Rust HAL, answering the Rust's register reads with the values the hardware
gave (they must happen in the same order at the same offsets) and comparing
every write, config access and delay. Accesses made in softirq/hardirq
context (lines ending in " @": NAPI, xmit) go to --datapath, with a summary.
Marks of calls the replay cannot reproduce from the trace alone (rx_mode,
mtu, set_mac) end the replayable part of a call; their accesses are kept as
comments.
"""
import argparse
import collections
import sys

EC_RFW_HASH0_KEY = 0x800   # ../rust/hal/regs.rs off::EC_RFW_HASH0_KEY, stride 4, 10 keys
RX_HASH_KEYS = 10


EC_FWD_MAC0 = 0x868        # off::EC_FWD_MAC0_DATA_L, 5 registers per entry, stride 0x14
EC_FWD_MAC_STRIDE = 0x14
MAC_TABLE_ALL_MC = 4
MAC_TABLE_DROP = 31


def rx_mode_args(lines):
    """The (promisc, allmulti, mc_count, uc_count) al_eth_set_rx_mode was
    called with, from the MAC table entries it wrote (al_eth_main.c:5196):
    promiscuous: only the drop entry, forwarding; else entry 4 (all-multicast,
    or cleared when there is no multicast address), entries 1..3 for
    secondary unicast addresses, then the drop entry, dropping."""
    entries = []
    for l in lines:
        p = l.split()
        off = int(p[2], 16)
        if p[0] != "W" or off < EC_FWD_MAC0 or (off - EC_FWD_MAC0) % EC_FWD_MAC_STRIDE != 16:
            continue
        entries.append(((off - EC_FWD_MAC0) // EC_FWD_MAC_STRIDE, int(p[3], 16)))
    if not entries or entries[-1][0] != MAC_TABLE_DROP:
        return None
    if len(entries) == 1:
        return (1, 0, 0, 0) if entries[0][1] & 0xf0 else None
    if entries[0][0] != MAC_TABLE_ALL_MC:
        return None
    mc = 1 if entries[0][1] else 0
    uc = sum(1 for i, v in entries if 1 <= i < MAC_TABLE_ALL_MC and v)
    return (0, 0, mc, uc)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("trace")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--datapath")
    ap.add_argument("--rev", default="2", help="PCI revision the probe ran with")
    a = ap.parse_args()

    calls = []          # (name, [lines]) in order of their entry marks
    dp = []
    header = []
    cur = {}            # pid -> the call it is in (concurrent callers stay apart)
    bh = None           # the rx_mode call collecting softirq-context EC writes
    with open(a.trace) as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("# al_eth_trace"):
                header.append(line)
                continue
            if line.endswith(" @"):
                # ndo_set_rx_mode runs under netif_addr_lock_bh (softirq
                # context to the recorder): its EC writes follow its mark
                if line.startswith("# mark rx_mode"):
                    bh = ("rx_mode", [])
                    calls.append(bh)
                    continue
                if line.startswith("#"):
                    continue
                if bh is not None and line.startswith("W e "):
                    bh[1].append(line[:-2])
                    continue
                bh = None
                dp.append(line[:-2])
                continue
            body, _, pid = line.rpartition(" [")
            pid = pid.rstrip("]")
            if line.startswith("# mark "):
                what = line.split()[2]
                if what == "bars":
                    continue   # inside probe
                if what == "mtu" and pid in cur and cur[pid][0] == "open":
                    continue   # al_eth_up_complete calls al_eth_change_mtu: part of open
                cur[pid] = (what, [])
                calls.append(cur[pid])
                continue
            if pid not in cur:
                cur[pid] = ("before-probe", [])
                calls.append(cur[pid])
            cur[pid][1].append(body)

    out = ["# from %s: %s" % (a.trace, " ".join(header)), "S hw"]
    for name, lines in calls:
        if name == "probe":
            out.append("I CALL probe %s" % a.rev)
        elif name == "open":
            keys = {}
            for l in lines:
                p = l.split()
                if p[0] == "W" and p[1] == "e":
                    off = int(p[2], 16)
                    if EC_RFW_HASH0_KEY <= off < EC_RFW_HASH0_KEY + 4 * RX_HASH_KEYS:
                        v = int(p[3], 16)
                        keys[(off - EC_RFW_HASH0_KEY) // 4] = int.from_bytes(v.to_bytes(4, "little"), "big")  # undo htonl
            if len(keys) != RX_HASH_KEYS:
                sys.exit("open without the 10 Toeplitz key writes: not a full al_eth_open")
            out.append("I CALL up " + " ".join("%08x" % keys[i] for i in range(RX_HASH_KEYS)))
        elif name == "close":
            out.append("I CALL down")
        elif name == "stats":
            if not lines:
                continue   # called before up: returns without touching the device
            out.append("I CALL stats")
        elif name == "rx_mode":
            args = rx_mode_args(lines)
            if args is None:
                out.append("# not replayed: rx_mode with an unexpected write pattern")
                out.extend("# " + l for l in lines)
                continue
            out.append("I CALL rx_mode %d %d %d %d" % args)
        else:
            out.append("# not replayed: %s (%d accesses)" % (name, len(lines)))
            out.extend("# " + l for l in lines)
            continue
        out.extend(lines)
    with open(a.out, "w") as f:
        f.write("\n".join(out) + "\n")

    if a.datapath:
        with open(a.datapath, "w") as f:
            f.write("\n".join(dp) + "\n")
    c = collections.Counter((l.split()[0], l.split()[1] if len(l.split()) > 2 else "") for l in dp)
    print("control: %d calls (%s); datapath: %d accesses %s" % (
        len(calls), ", ".join(n for n, _ in calls), len(dp), dict(c)))


if __name__ == "__main__":
    main()

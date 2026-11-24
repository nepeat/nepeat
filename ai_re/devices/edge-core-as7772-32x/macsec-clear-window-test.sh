#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), SONiC up with MACsec on cages 1-2: what passes IN CLEAR on a port that is
# configured for MACsec while the stack starts? A ping (20 per second, cage 1 -> cage 2) runs across a restart; every
# reply is placed on the time line of the PHY engines (syncd's "MACsec egress|ingress on|off" lines) and of the SAs
# (orchagent).
#   bash -l macsec-clear-window-test.sh [--dry-run] ['systemctl restart sonic.target']
#   bash -l macsec-clear-window-test.sh --cleanup [--dry-run]    undo an interrupted run
# Builds the VRF-pair topology of macsec-traffic-test.sh itself (loadtest/sonic-test-lib.sh beside it or in
# ./loadtest/: CONFIG_DB snapshot, restore on every exit, the whole CONFIG_DB compared with it, refuses over an
# earlier run's leftovers); the topology is runtime CONFIG_DB and lives through the restart.
# A reply while all four engines of the link are off went through both PHYs in bypass: in clear. While the engines
# are on and there is no SA the PHY drops data (the gate): there must be no reply. The ping needs its addresses,
# which come back late in a start, so every frame the CPU RECEIVES on the two ports is recorded too (a packet socket
# on all interfaces; IPv6 neighbour discovery, MLD, ARP, LLDP that the kernel and lldpd send as soon as a port is
# up): a frame other than EAPOL received while the engines are off crossed the cable in clear.
# Only frames from and to the CPU are seen here; what the ASIC forwards between front ports is not measured.
# Exit status: macsec-up-check.sh's (MACSEC_UP_CHECK: another path to it, for the offline test).
set -u
here=$(cd "$(dirname "$0")" && pwd)
lib=
for l in "${SONIC_TEST_LIB:-}" "$here/loadtest/sonic-test-lib.sh" "$here/sonic-test-lib.sh"; do
  if [ -n "$l" ] && [ -r "$l" ]; then lib=$l; break; fi
done
[ -n "$lib" ] || { echo "sonic-test-lib.sh not found: copy loadtest/sonic-test-lib.sh next to this script"; exit 2; }
# shellcheck source=loadtest/sonic-test-lib.sh
. "$lib"
st_args "$@"
ST_PHASES=("${ST_PAIRS_PHASES[@]}")
st_verify_extra() { st_mka_verify; }
[ "$ST_MODE" = cleanup ] && st_cleanup macsec-clear-window-test
cmd=${ST_ARGS[0]:-systemctl restart sonic.target}
check=${MACSEC_UP_CHECK:-$here/macsec-up-check.sh}
echo "== $(date -Is) $(readlink /run/current-system | cut -c12-23)... ; command: $cmd"
st_pairs_macsec_check
[ -r "$check" ] || st_refuse "$check not found (macsec-up-check.sh beside this script)"
st_refusals || exit 1
st_pairs_session macsec-clear-window-test
[ "$ST_DRY" = 1 ] && { echo "(dry run: '$cmd' and the ping across it are not run)"; exit 0; }
tmp=$ST_DIR/clear-window; mkdir -p "$tmp"
ping -c 2 -W 2 10.0.1.1 >/dev/null || { echo "FAIL: no ping 10.0.1.1 after the setup"; exit 1; }
ping -D -O -i 0.05 -W 1 10.0.1.1 > "$tmp/ping.txt" 2>&1 &
pp=$!; ST_BG+=("$pp")
t=$SECONDS   # the ping runs (its first line) before anything else: its SIGINT at the end must find it ready
while [ ! -s "$tmp/ping.txt" ] && kill -0 "$pp" 2>/dev/null && [ $((SECONDS - t)) -lt 10 ]; do sleep 0.1; done
python3 -u -c '
import socket, struct, time
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3))      # every protocol, every interface
while True:
    data, (ifname, proto, pkttype, hatype, addr) = s.recvfrom(128)
    if pkttype != 4 and ifname in ("Ethernet0", "Ethernet4"):               # not our own transmissions
        print("%.6f %s %04x %s" % (time.time(), ifname, proto, ":".join("%02x" % b for b in data[6:12])), flush=True)
' > "$tmp/rx.txt" 2>/dev/null &
cp=$!; ST_BG+=("$cp")
sleep 3
t0=$(date +%s.%N)
$cmd
bash "$check" -t 90 > "$tmp/check.txt" 2>&1; rc=$?
tail -2 "$tmp/check.txt" | head -1
sleep 5
kill -INT "$pp"; kill "$cp"; wait "$pp" "$cp" 2>/dev/null; ST_BG=()
journalctl -b -o short-unix --no-pager --since "@${t0%.*}" > "$tmp/journal.txt"
python3 - "$tmp" "$t0" <<'EOF'
import re, sys
tmp, t0 = sys.argv[1], float(sys.argv[2])
ev = []          # (t, text)
eng = {}         # (port, dir) -> on?; before the restart all four are on
for l in open(tmp + "/journal.txt", errors="replace"):
    m = re.match(r"([0-9.]+) \S+ (\S+?)(?:\[\d+\])?: (.*)", l)
    if not m:
        continue
    t, unit, msg = float(m.group(1)), m.group(2), m.group(3)
    k = re.search(r"port (ce[01]): MACsec (egress|ingress) (on|off)", msg)
    if k:
        ev.append((t, "engine", (k.group(1), k.group(2)), k.group(3) == "on"))
    elif "engines were left on by a previous process" in msg:
        ev.append((t, "reset", None, msg.split("table: ")[-1]))
    elif re.search(r"createMACsecSA: MACsec SA Ethernet[04]:", msg):
        ev.append((t, "sa", None, msg.split("MACsec SA ")[-1].replace(" is created.", "")))
    elif re.search(r"Set operation status (UP|DOWN) to host interface Ethernet[04]$", msg):
        ev.append((t, "oper", None, msg.split("Set operation status ")[-1]))
    elif unit == "systemd" and re.search(r"(Starting|Started|Stopping|Stopped) SONiC (syncd|orchagent|macsecmgrd)", msg):
        ev.append((t, "unit", None, re.sub(r" \(.*", "", msg)))
    elif "waits for the netdev" in msg or "loading success" in msg and re.search(r"'Ethernet[04]'", msg):
        ev.append((t, "mgr", None, msg.split(":- ")[-1][:110]))
pings = []       # (t, seq)
lost = 0
for l in open(tmp + "/ping.txt", errors="replace"):
    m = re.match(r"\[([0-9.]+)\] \d+ bytes from .*icmp_seq=(\d+)", l)
    if m:
        pings.append((float(m.group(1)), int(m.group(2))))
    elif "no answer yet" in l:
        lost += 1
rx = []          # (t, ifname, ethertype)
for l in open(tmp + "/rx.txt", errors="replace"):
    f = l.split()
    if len(f) == 4:
        rx.append((float(f[0]), f[1], f[2]))
state = {("ce0", "egress"): True, ("ce0", "ingress"): True, ("ce1", "egress"): True, ("ce1", "ingress"): True}
sas = 4          # before the command: MKA up (the check at the top of macsec-traffic-test.sh)
phase = "before"
def label():
    on = sum(state.values())
    if on == 0:
        return "bypass, " + phase
    if on == 4:
        return "macsec+sa" if sas >= 4 else "macsec,gate"
    return "mixed"
# walk time: count replies per interval between events
rows, cur, start, n = [], "macsec+sa", t0 - 3, 0
merged = sorted([(t, 0, e) for t, *e in ev] + [(t, 1, s) for t, s in pings] + [(t, 2, (i, p)) for t, i, p in rx])
counts = {}
first_last = {}
rxc = {}
odd = []         # data seen while an engine was on without SAs
def near(t):
    d = min((abs(t - e[0]), e[0]) for e in ev if e[1] in ("engine", "reset"))
    return "%+.0f ms from the nearest engine change" % ((t - d[1]) * 1000)
for t, kind, x in merged:
    if kind == 1:
        lab = label()
        if lab in ("mixed", "macsec,gate"):
            odd.append("%+.3f s ping reply seq %d, engines on: %s; %s" % (t - t0, x, sorted(" ".join(k) for k, v in state.items() if v), near(t)))
        counts[lab] = counts.get(lab, 0) + 1
        fl = first_last.setdefault(lab, [t, t]); fl[1] = t
        continue
    if kind == 2:
        d = rxc.setdefault(label(), {"eapol": 0, "other": 0, "types": {}, "span": [t, t]})
        d["span"][1] = t
        if x[1] == "888e":
            d["eapol"] += 1
        else:
            d["other"] += 1
            d["types"][x[1]] = d["types"].get(x[1], 0) + 1
            if label() in ("mixed", "macsec,gate"):
                odd.append("%+.3f s frame %s received on %s, engines on: %s; %s" % (t - t0, x[1], x[0], sorted(" ".join(k) for k, v in state.items() if v), near(t)))
        continue
    what, key, val = x
    before = label()
    if what == "engine":
        state[key] = val
        if not val:
            sas = 0
    elif what == "reset":
        for k in state: state[k] = False
        sas = 0
    elif what == "sa":
        sas += 1
    elif what == "unit" and "Stopping SONiC syncd" in str(val) and phase == "before":
        phase = "stack stopping"
    elif what == "unit" and "Starting SONiC syncd" in str(val):
        phase = "stack starting"
    if what == "engine" and phase == "before":
        phase = "stack stopping"
    after = label()
    print("%+8.3f s  %-7s %-52s engines on: %d of 4, SAs %d  -> %s" % (t - t0, what, (" ".join(key) + (" on" if val else " off")) if what == "engine" else str(val)[:52], sum(state.values()), sas, after))
print("== ping replies by the state of the link at the time (20 pings per second; %d replies, %d 'no answer yet')" % (len(pings), lost))
LABS = ("macsec+sa", "bypass, stack stopping", "bypass, stack starting", "mixed", "macsec,gate")
for lab in LABS:
    c = counts.get(lab, 0)
    fl = first_last.get(lab)
    print("  %-24s %5d replies%s" % (lab, c, "" if not fl else "  (first %+.2f s, last %+.2f s)" % (fl[0] - t0, fl[1] - t0)))
print("== frames the CPU received on Ethernet0 / Ethernet4 by the state of the link (EtherType: count)")
for lab in LABS:
    d = rxc.get(lab)
    if d:
        print("  %-24s EAPOL %4d, other %5d %s  (%+.2f .. %+.2f s)" % (lab, d["eapol"], d["other"], dict(sorted(d["types"].items())), d["span"][0] - t0, d["span"][1] - t0))
    else:
        print("  %-24s none" % lab)
clear = {lab: (counts.get(lab, 0), rxc.get(lab, {}).get("other", 0)) for lab in LABS if lab.startswith("bypass")}
gate = counts.get("macsec,gate", 0) + rxc.get("macsec,gate", {}).get("other", 0) + counts.get("mixed", 0) + rxc.get("mixed", {}).get("other", 0)
for lab, (p, f) in clear.items():
    print("== IN CLEAR (both PHYs in bypass), %s: %d ping replies, %d other frames received" % (lab.split(", ")[1], p, f))
print("== with an engine on and no SA (the gate: must be 0): %d replies or data frames" % gate)
for o in odd:
    print("   " + o)
if odd:
    print("   (syncd logs an engine change when its register sequence is done, 36-110 ms after it began: a frame that close to a change is not placed reliably)")
EOF
exit "$rc"

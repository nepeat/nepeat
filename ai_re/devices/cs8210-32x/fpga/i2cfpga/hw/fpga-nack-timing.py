#!/usr/bin/env python3
"""Empty-cage tests of the CS8210 I2C FPGA, built on cs8210-qsfp's Dev class
(same allowlist, 32-bit accesses only).
1. per port: 1-byte read at 0x50 -> expect status bit 1 (NACK) on that port only.
2. port 1: time the busy period (start write -> bit 31 clear) per speed profile.
Writes: +0x1000/+0x1008/+0x100c (vendor probe values), +0x1050/+0x1054 (commands)."""
import importlib.machinery, sys, time, statistics

q = importlib.machinery.SourceFileLoader("q", sys.argv[1]).load_module()
d = q.Dev("/sys/bus/pci/devices", q.DEFAULT_BDF)
assert d.check(False, False)
d.open(True)

def start_read(port, addr=0x50, n=1):
    b = q.chb(port)
    d.w32(b + q.C_CMD, q.CMD_READ | (addr << 8) | n)
    d.w32(b + q.C_START, q.START_BUSY)

assert d.r32(q.R_PRESENT) == 0, "a module is present; this test is for empty cages"

# init all channels exactly as the vendor probe
for p in range(1, 33):
    b = q.chb(p)
    d.w32(b + q.C_MRESET, q.PROBE_MRESET)
    d.w32(b + q.C_PROFSEL, q.PROBE_PROFSEL)
    d.w32(b + q.C_PROFILE, q.PROBE_PROFILE)

print("== test 1: NACK per port, status of all ports after each")
bad = 0
for p in range(1, 33):
    start_read(p)
    time.sleep(0.002)
    st = [d.r32(q.chb(k) + q.C_STATUS) for k in range(1, 33)]
    busy = d.r32(q.chb(p) + q.C_START) & q.START_BUSY
    others = {k + 1: v for k, v in enumerate(st) if k + 1 != p and v not in (0, 2)}
    ok = st[p - 1] == 2 and not busy
    bad += not ok
    print(f"port {p:2}: status={st[p-1]:#010x} busy={bool(busy)} "
          f"{'OK' if ok else 'UNEXPECTED'} {'others: ' + str(others) if others else ''}")
print(f"test 1: {32 - bad}/32 as predicted")

print("== test 2: busy duration on port 1 per profile (perf_counter_ns, poll loop)")
b = q.chb(1)
# poll-loop cost: time 1000 reads of the start register
t0 = time.perf_counter_ns()
for _ in range(1000):
    d.r32(b + q.C_START)
per_read = (time.perf_counter_ns() - t0) / 1000
print(f"one poll read ~{per_read:.0f} ns")
for sel, label in ((0, "code 0x18 '400k'"), (1, "code 0x09 '1M'"), (2, "code 0x66 '100k'")):
    d.w32(b + q.C_PROFSEL, sel)
    durs, polls = [], []
    for _ in range(300):
        d.w32(b + q.C_CMD, q.CMD_READ | (0x50 << 8) | 1)
        t0 = time.perf_counter_ns()
        d.w32(b + q.C_START, q.START_BUSY)
        n = 0
        while d.r32(b + q.C_START) & q.START_BUSY:
            n += 1
            if n > 100000:
                break
        durs.append(time.perf_counter_ns() - t0)
        polls.append(n)
    st = d.r32(b + q.C_STATUS)
    print(f"sel {sel} {label}: median {statistics.median(durs)/1000:.1f} us, "
          f"min {min(durs)/1000:.1f} us, median polls {statistics.median(polls)}, "
          f"final status {st:#010x}")
d.w32(b + q.C_PROFSEL, q.PROBE_PROFSEL)
d.close()

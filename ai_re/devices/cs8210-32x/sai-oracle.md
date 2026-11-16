# CS8210-32X: what the isai oracle showed (2026-11-16)

The oracle ([packages/sai-teralynx/oracle](../../nix/packages/sai-teralynx/oracle/README.md)) runs one
SAI script on our SAI (native) and on isai 6.2.0 (under qemu-ipd) with every libifcs call traced,
and diffs them per SAI step. Findings that changed our code; traces are local
(`analysis/oracle/<run>/`, sha256 prefix in brackets).

## Performance (`perf.json`, `temperature.json`)

- **The "62 ms per OPER_STATUS get" was the temperature read.** Each `IFCS_NODE_ATTR_*_LAST_TEMP`
  read takes ~400 ms inside libifcs and holds its API lock; the frontend's temperature publisher
  read all five monitors (2 s) every 5 s, so the first SAI call in each window waited ~2 s (the
  benchmark's 2001 ms / 32 calls was one such wait). `LINK_STATUS` itself costs ~5 us.
  [perf-20261116-034818 `2a8fbd10`, temperature-20261116-035053 `38efd958`]
  Fix: one monitor per read, round-robin, at most every 15 s (`SAI_TERALYNX_TEMP_PERIOD`): one
  ~400 ms wait per 15 s remains (visible in sai_bench as a 400 ms step right after create_switch).
- isai reads `LINK_STATUS` per OPER_STATUS get, and for counters asks `devport_stats_get` for
  exactly the requested ids (4), where we asked the vendor CLI's "all 112" form (8x slower).
  Fix: the 12 mapped ids in one call. isai maps `IF_OUT_OCTETS` to `TX_BYTES_OK`, we to
  `TX_BYTES_ALL` (open).
  [perf-20261116-035259: ours `3f36c84c`, isai `c952804c`, report `efd2de2f`]
- isai's create_switch: 4794 IFCS calls (ours 174), 18.8 s of IFCS time under qemu.

## Host traps and CoPP (`traps.json`: SONiC's copp_cfg groups, policers, 17 trap types)

[traps-20261116-035604: isai `455e5bd4`, ours `92514314`, report `9dd757cf`]
- One ingress ACL table (match profile: DMAC, ether type, L4 protocol, L4 ports, L4 byte 1,
  hostif trap, ...; action profile: drop, copy to CPU, copy override, meter, counter, ...),
  scope per inno block, **6 ACLs (IB 0-5)** added to it.
- Per SAI trap: a hostif trap object (isai pre-creates ~40 at init) whose `QUEUE_NUM` is the trap
  group's queue; per IB an ACE: matches (LLDP: three DMACs 01:80:c2:00:00:0e/03/00 + 0x88cc;
  BGP: 0x0800 + port 179 + `HOSTIF_TRAP` = the IP2ME trap; IP2ME / NEIGHBOR_MISS / TTL: the
  hostif trap alone), actions COPY_TO_CPU (that trap; green/yellow, red too when unpoliced),
  COPY_TO_CPU_OVERRIDE (red, when policed), DROP (TRAP action), METER, counter.
- TTL_ERROR = system trap `FWD_L3_TTL_CHECK_FAIL` (17): `QUEUE_NUM` + `ENABLE`, plus an ACE on
  it for the policer; L3_MTU_ERROR = system trap 23, queue + enable only (no policer).
- Policers: `acl_meter` per IB, byte type, RFC 2697, colour aware; pps converted at 84 B per
  packet (CIR = pps x 672, CBS = packets x 84).
- ACE priority: almost all traps 0x7ffffffe, IP2ME 0x80000000, a PFC drop 1: lower wins.
- Our 0.12.41 SDK refuses isai's table attribute `TYPE` and needs `DEFAULT_STATS` off with
  `MAX_COUNTERS`, and `MAX_METERS` set (SDK log); otherwise the same objects work
  ([sai-features.md](sai-features.md), "Host traps").

## Running it

`run-oracle.sh SCRIPT both` takes ~3-4 min (isai's create_switch ~20 s, remove ~13 s, tracing
on top); ipd goes to `legacy_abi=0` for the isai run and back to 1 (box-run.sh, also on failure).

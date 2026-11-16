# CS8210-32X: Marvell's amd64 SAI under qemu-user (experiment)

Owner's ask (2026-11-16): try upstream's Teralynx SAI instead of our own. No arm64 build exists
([search](../sonic-nixos/cs8210-sai-search-20261115.txt)), so: Marvell's amd64 `libsai` under a
patched qemu-user on the A72s, talking to the real `ipd` driver. Our own SAI path stays the
working one ([sonic.md](sonic.md)).

## Status (2026-11-16)

- **Upstream SONiC runs the chip through Marvell's SAI under qemu-ipd**, 2 ports (cages 1, 16):
  pinned orchagent/portsyncd/managers (arm64) + sairedis **202505** syncd (amd64, under qemu-ipd) on
  isai 6.2.0. Both ports oper up in SONiC 36 s after start (create_switch 20 s, under sairedis' 60 s
  sync timeout), traps installed, counters polled, LLDP sees the loop. Started by hand, nothing deployed.
- **Link via SAI only:** `SAI_PORT_SERDES` TX taps from the vendor tables + FEC RS + link training:
  400G up in 5.6 s, 0 uncorrectable FEC. No RX settings or rx_eq_fine_tune needed.
- **Speed:** isai under qemu is 6.6x slower to create_switch (21.3 s vs 3.2 s) and 8-25x slower per
  route/FDB call than our native SAI; table below.
- **SAI ABI trap:** isai is SAI 1.16.1, where `sai_attribute_t` is 48 bytes; the pinned sairedis
  (SAI 1.19.2) has 64. syncd must be built from SONiC 202505 (patched, below), not the pinned one.
- Box left restored: ipd `legacy_abi=1`, nothing running. [Evidence](sai-qemu-hw-20261116.txt).
- **Decision (owner): SONiC runs on our SAI**; isai under qemu is kept as a behaviour oracle: its
  libifcs calls traced and diffed against ours ([sai-oracle.md](sai-oracle.md)).
- Next: a syncd/orchagent pairing without the version gap (orchagent 202505 too, or a 1.16->1.19
  attribute shim), all 32 ports, traffic; whether the speed gap matters for SONiC operations (bulk
  route programming) before choosing between this and our own SAI.
- Code: [packages/qemu-ipd](../../nix/packages/qemu-ipd/README.md) (+ `sonic-isai.nix`),
  [packages/isai-amd64](../../nix/packages/isai-amd64/default.nix). Not wired into
  `packages/default.nix` (build commands in the README).

## Release: isai 6.2.0 (SAI 1.16.1), Marvell `debian/202505/T6.2.0`

`isai.deb` sha256 `8249175638c90b63…` (all four hashes pinned in isai-amd64). Reasons:

- **libifcs attaches only to its own ipd version.** `_ii_pcie_attach` parses the version string of
  `IPD_QUERY_NODE` into 6 numbers and compares all six with `ifcs_get_version()` +
  `ifcs_get_version_extended()`; a mismatch logs "IFCS SDK version … mismatch with driver version"
  and fails (disassembly of the release's `libifcs.so`, which carries DWARF). Values:
  6.2.0 = **0.16.2.0.0.0** = the box's ipd (mrvl-teralynx `53d8dcf`, `IPD_VERSION_STRING`);
  7.0.1 = 0.16.5.0.0.0; T5.0.3 = 0.15.13.0.0.0. The 6.2.0 `ishell.deb` even bundles an x86
  `ipd.ko` 0.16.2.0.0.0. (The "0.16.0"/"0.15.0" strings in the libraries are not the check.)
- **Same ioctl ABI:** `inno_ioctl.h` of mrvl-teralynx branches T6.1.1/T6.2.0 is byte-identical to
  master 0.16.2. From T6.2.1 on it changes: `inno_ioctl_rupt_mask_t` gains `int flag` with
  CLEAR/EXIT semantics (an old driver would *set* masks a 7.x libifcs asks to clear), and
  commands 41-43 (BFD flags, port LAG) appear.
- Our driver runs with `legacy_abi=1` (for the 2020 SDK); 6.2.0 needs the module loaded with
  `legacy_abi=0` (0444 parameter: reload). No new driver build needed.
- Also weighed: kamelnetworks issue #60 (SAI 7.0.x crashes on Teralynx 7, T5.0.3 works) is a second
  reason to stay below 7.x. T5.0.3 is the fallback if 6.2.0 misbehaves, but needs an ipd 0.15.13
  (no public source branch; would mean a version-spoof + ABI diff against 0.16.2).
- Chip/board support in 6.2.0: `configs/sku/innovium.77700_A` (TL7, same SKU file as our SAI),
  `configs/yaml/board/innovium_32x400G.config.yaml`; sonic-buildimage has the x86 TL7 32x400
  Midstone-200i (`sai.profile` -> `inno.config.yaml` -> IFCS yaml + SKU, same IFCS yaml family
  as our generator's).
- SAI ABI: sairedis' configure check (vendor >= 1.9) passes for the pinned sairedis (1.19.2), but
  the structs are not binary compatible: `sizeof(sai_attribute_t)` 48 (1.16.1) vs 64 (1.19.2),
  enum values equal. syncd = sairedis 202505 (SAI submodule 1.16.1), see "SONiC" below.

Dependencies of `libsai.so.1.0` (207 MB, stripped, IFCS + AAPL linked in): libstdc++, jansson,
libyaml, libpython3.11, libortools.so.9, libmkaecp.so (bundled; imports 35 OR-Tools 9.11 CP-SAT
symbols + abseil lts_20240722). We give it nixpkgs python311/jansson/libyaml, real abseil
20240722 and a generated OR-Tools stub (aborts with the symbol name if called). `dlopen(RTLD_NOW)`
of libsai + libmkaecp succeeds in the build.

## What the userspace does to the driver (libsai 6.2.0, `objdump` of every `ioctl@plt` call)

All on `/dev/ipd%d` (opened by `_ii_pcie_probe/_attach`), type `'I'`, asm-generic encoding
(same on x86_64 and arm64), argument = pointer (`_IOWR(…, T *)`, size field 8) except the int ones:

| nr | command | used in |
|---|---|---|
| 1 | INFO_NODES (`_IOR`) | probe, user_config_create |
| 2 | QUERY_NODE | attach (version check above) |
| 3, 4 | RUPT_MASK, RUPT_WAIT | the 7 interrupt threads (rx, tx, learn, pic, error, async, rx_offloaded_stats), threads_stop |
| 7, 8 | RING_ALLOC, RING_FREE | ring_alloc/free, ipd_ring_create/delete |
| 9, 10 | HRR_ALLOC, HRR_FREE | attach, detach |
| 11, 12 | LEARN_ALLOC, LEARN_FREE | learn_alloc, detach |
| 13, 14 | TX_SEND, UNPIN_PAGE | send_packet, rupt_thread_tx |
| 15, 16 | CREATE/DELETE_NETDEV | create/delete_netdev |
| 17 | RING_PAGE_ALLOC | attach, ring_alloc |
| 19, 20, 21 | PIC_STATUS_ALLOC/FREE/DUMP | attach, detach, chain_dump |
| 23, 24 | RING_ENABLE/DISABLE | ring_enable |
| 25, 26 | GET/SET_NETDEV | get/set_netdev |
| 27, 28, 29 | CREATE/DELETE/SET_NETLINK | netlink (sflow) |
| 30, 31 | GET/SET_PARAMS | attach, get/set_params |
| 32, 33, 34 | DMA_INIT, FLUSH_RECOVER_DMA, HW_INIT (`_IOW`, `void *`) | attach, user_config_create |
| 35-40 | IS_USER_CONFIG_DONE, USER_CONFIG_DONE/CLEARED, OVERRIDE_FLOW_CONTROL, CLEANUP_RESOURCES, SFLOW_SET_PARAMS (`_IOW`, `int`) | attach, user_config_*, … |

Not used: 5/6 READ/WRITE_MEM, 18 RING_PAGE_FREE, 22 DEBUG_EVT_THREAD_TIMER. Other ioctls in
libsai: `SIOCGIFINDEX` (socket; qemu supports it) and `I2C_SLAVE` on `/dev/i2c-*` in AAPL's
`avago_system_i2c_*` (SerDes I2C access method; refused by qemu-ipd: never passed through).
**mmap:** on the chip 264 device mappings, 257 of them single scattered DMA pages (qemu-ipd's
first fixed 256-entry table aborted run 1; both tables are now sorted and unbounded). Call sites:
`_ii_pcie_attach` (6 calls: BAR0 and DMA pool via `/dev/ipd` offsets = physical
addresses, `remap_pfn_range`), `_ii_pcie_ring_alloc` (3), `_ii_pcie_learn_alloc` (2),
`pd_shml_init` (3), `wb_malloc` (1). On arm64 ipd maps all of it `pgprot_noncached`
(Device-nGnRnE), including the DMA pool. Layouts: all 31 `inno_ioctl.h` typedefs are identical
on x86_64 and aarch64 (pahole, test `layout`), and several carry user pointers.

## qemu-ipd (qemu 11.1.2, two patches)

1. **linux-user passthrough:** an ioctl of type `'I'` on an fd of the `ipd` char major
   (`/proc/devices`) goes to the host unchanged: command, integer argument or pointer, and every
   pointer inside the struct. That is only right with guest addresses == host addresses, so it
   requires `guest_base == 0` (`-B 0`) and refuses otherwise; nr outside 1..43 is refused. All
   other ioctls take qemu's normal path (unknown = ENOTTY). mmaps of the device are recorded; a
   guest **atomic** (lock-prefixed RMW, cmpxchg) on them faults the guest (x86: SIGSEGV) in the
   atomic helper before the host touches the address: on the A72 (ARMv8.0, no LSE) it would be an
   exclusive pair on Device memory. `QEMU_IPD_TRACE=1` logs every ipd ioctl and mmap.
2. **`QEMU_IPD_STRICT=1`** (the runner sets it): barriers also while the guest is single-threaded
   (qemu drops them otherwise; the device is a second observer), **DMB OSH** instead of ISH (what
   Linux uses before MMIO), and 16-byte SSE accesses as two 8-byte halves: plain qemu gives aligned
   ones single-copy atomicity when the guest CPU has AVX, which on a host without LSE2 is the
   `ld/st_i128` helper's LDXP/STXP loop, i.e. a read that writes back.

Guest side: `-cpu Nehalem` (no AVX; libsai itself is baseline SSE2: no ymm, no cmpxchg16b, no
movnt), and **`libipd-devmem.so`** as the guest's `LD_PRELOAD`: memcpy/memmove/memset/memcmp
touching an ipd mapping use naturally aligned 8/4/2/1-byte accesses (glibc's x86 SSE2 versions
would be unaligned 16-byte host accesses: SIGBUS on Device memory, the same bug the native arm64
run hit). Unaligned accesses the compiler inlined into libsai would still SIGBUS: a clean process
death, not a device access.

Offline tests (all pass, `tests.*`): `layout`; `passthrough` (qemu-ipd on x86 under a
seccomp-notify fake driver: the guest's pointer arrives as the same address, struct written back,
integer arg, errno, unknown nr / other fd / I2C_SLAVE not passed, 4 device-map atomics fault,
partial munmap and MAP_FIXED replacement tracked); `codegen` (strict keeps a single-threaded
mfence; aligned SSE `al16` -> `pair`); `aarch64` (the box binary under
`qemu-aarch64 -cpu cortex-a72`: plain = 17 `ld/st_i128` helper calls and DMB ISH; strict = 0
helper calls, 0 LDXP/STXP, 0 DMB ISH, DMB OSH); `devmem` (every alignment/length 0-79 into and
out of a device map, overlapping memmove). Also run by hand on the workstation (x86, no ASIC, in
bwrap with an empty /dev): `isai-version` prints 11601 natively and under qemu-ipd; `isai-smoke`
gets through `sai_api_initialize`, reads the profile and the IFCS yaml, scans `/dev` for ipd and
returns `create_switch` -7 (ITEM_NOT_FOUND), the same natively and under qemu-ipd + devmem; the
syncd built against this libsai loads it and stops at the missing redis config.

## Benchmark: our SAI (native) vs isai (qemu-ipd), 2026-11-16

Same boot, same IFCS yaml (32 x 400G), [tools/sai_bench.cpp](../../nix/packages/qemu-ipd/tools/sai_bench.cpp)
built for each, mean of 3 runs at IFCS `log_level: "error"` (all calls SUCCESS):

| operation | calls | native ms | isai+qemu ms | isai / native |
|---|---|---|---|---|
| create_switch | 1 | 3228 | 21322 | 6.6x |
| port get oper_status | 32 | 2001 | 3.5 | 0.002x (ours: 62.5 ms per call) |
| port get speed / mtu | 32 | 0.6 / 0.01 | 3.1 / 11.8 | |
| port set mtu | 32 | 3.3 | 54.8 | 17x |
| port get_stats (4 ids) | 32 | 55.3 | 27.9 | 0.5x |
| route create / remove (drop, VRF) | 1000 | 34.7 / 25.0 | 264 / 308 | 7.6x / 12x |
| fdb create / remove (static) | 100 | 13.8 / 10.8 | 321 / 271 | 23x / 25x |
| remove_switch | 1 | 243 | 13208 | 54x |

Logging: isai at the default `info` writes ~13 k lines per init (the "ECCH UE Cache update" flood):
+3.7 % create_switch, +31 % routes, +20 % FDB; the knob is `ifcs.options.log_level` in the IFCS
yaml. Our SAI's 0.12 SDK logs to a file and hardly at info (no cost). Two findings for our SAI:
**OPER_STATUS get takes 62.5 ms** (the slow getter of the smoke tool's report tick), and counter
reads are 2x slower than isai's. Full table with per-call us: the evidence file.

## Link and SONiC with isai, 2026-11-16

- Link: `create_port_serdes` (TX_FIR_ATTN/PRE1/PRE2/PRE3 per lane from `preemphasis.cmd`),
  `FEC_MODE` RS, `LINK_TRAINING_ENABLE`, admin up: both ports up in 5.6 s, 0 uncorrectable FEC,
  ~3-8 correctable/s. In SONiC the same values go into CONFIG_DB PORT (`attn`, `pre1`..`pre3` as
  hex lists, `fec rs`, `link_training on`); upstream boards feed them via media_settings.json/xcvrd.
- SONiC: [sonic-isai.nix](../../nix/packages/qemu-ipd/sonic-isai.nix) on nepeat/nix's cs8210 config
  (2 cages via empty breakouts), unit files run as runtime units. syncd = sairedis 202505 built
  with doxygen 1.9.5 (the SAI 1.16 meta generator fails with current doxygen) plus one patch
  (`patches/sairedis-202505-capability-unknown-attr.patch`: a capability query for an attribute
  SAI 1.16 lacks answers NOT_SUPPORTED instead of shutting syncd down; the pinned orchagent asks for
  SAI_SWITCH_ATTR_FAST_LINKUP_POLLING_TIMEOUT). Left over from the version gap: warnings for 1.19-only
  port stats in flex counters; any 1.19-only attribute orchagent *sets* would still stop syncd.
- No syncd timeout seen; create_switch 20 s of the 60 s sync budget. A start-limit loop happened with
  the 1.19 syncd (orchagent's failure hook restarts syncd): each attempt failed before ASIC access.

## Alternatives weighed (FEX-Emu 2608, box64 0.4.4, both in nixpkgs)

Both pass LP64 ioctls straight through, so neither needs an ioctl patch, and both are faster than
TCG. Neither gives the device-memory guarantees above without work of its own: box64 wraps libc
natively by default (glibc arm64 memcpy on Device memory: the SIGBUS we already hit), its default
memory model emits no barriers (`BOX64_DYNAREC_STRONGMEM`), and x86 lock ops become exclusives
on the A72 with no guard; FEX's TSO mode on ARMv8.0 uses LDAR/STLR (alignment-checked) and
exclusives with a SIGBUS fix-up path whose behaviour on Device memory is unknown. qemu is the one
whose generated code we tested for this CPU. Keep box64 as plan B if TCG is too slow for the SDK's
time-based timeouts (SerDes firmware download, PLL/link polls).

## Phase 2 plan (first window, run as written; results above and in the evidence file)

Preconditions: MACsec agent done; `cs8210-switch`, `sonic.target`, any sai-smoke unit inactive;
`cs8210-qsfp` shows only cages 1 and 16 (stop otherwise); no flash/EEPROM/CPLD writes, no reboot,
fand untouched.

1. Read-only: `uname -r`, `getconf PAGESIZE` (must be 4096), `zcat /proc/config.gz | grep VA_BITS`
   (48 expected; guest_base 0 needs the x86 47-bit layout), `grep ipd /proc/devices`,
   `cat /sys/module/ipd/parameters/legacy_abi`, `lsmod | grep ipd` (refcount 0), `df /nix`.
2. `nix copy --to ssh://root@10.60.2.190` the runner (arm64, 144 MB closure) and the guest
   closure (amd64, 777 MB).
3. No-ASIC runs on the real A72: the passthrough test guest on a fake file and the codegen probe
   with `-d out_asm` (expect the aarch64 test's numbers), then `QEMU_IPD_TRACE=1 isai-qemu
   …/isai-version` (expect `11601`, no ipd access).
4. Driver to the 0.16 ABI: `rmmod ipd; modprobe ipd legacy_abi=0` (the command-line value is
   parsed after modprobe.d's `legacy_abi=1`); check `legacy_abi` = N, `/dev/ipd0`, dmesg
   "Version - 0.16.2.0.0.0". New ground: in this mode `HW_INIT` runs `inno_cpu_block_init`, which
   legacy mode skips and this box has never run.
5. One smoke run, logged to `sai-qemu-hw-<date>.txt`: profile `SAI_INIT_CONFIG_FILE` -> an
   inno.config.yaml (`IFCS_INIT_FILE` = our generator's CS8210 yaml merged with the vendor
   `isg.yaml`, as sai-teralynx does; `IFCS_SKU_FILE` = the release's `innovium.77700_A`); Credo
   PHYs first (as the native run); `QEMU_IPD_TRACE=1 isai-qemu …/isai-smoke profile 300 209 81`.
   Expected: QUERY_NODE accepted, HW/DMA_INIT, BAR/pool mmaps, ring and interrupt setup, SerDes
   firmware load (slow under TCG), create_switch 0, 32 ports, maybe the cage 1 <-> 16 link; then
   remove_switch. The IFCS yaml keys may need 0.12 -> 0.16 adjustments (unknown until it parses).
   The vendor CLI server (`IFCS_INNO_CLI_PORT`, 9999 upstream) is a shell: leave it unset or
   firewall it.
6. Back: `rmmod ipd; modprobe ipd` (legacy_abi=1 again), `/dev/ipd` present, so cs8210-switch and
   our SAI work as before.

Risks: an SError from an MMIO access the native SDK never makes (cpu block init, a qemu access
pattern we did not foresee) panics the kernel; a misprogrammed DMA ring could write host memory
(guest_base 0 keeps pointers right, no IOMMU); TCG slowness may trip SDK timeouts (fails init, no
damage expected); a soft lockup in ipd teardown (seen once with the unapplied perf patches).

# CS8210-32X: SONiC on NixOS

Cross-device notes and the stack itself: [sonic-nixos](../sonic-nixos/PROGRESS.md).

## Status (2026-11-16)

- **Our SAI runs the chip** (`sai-teralynx`: sai-openbcm's frontend + an IFCS backend dlopen()ing the
  vendor `libifcs.so` 0.12.41): every phase B/C feature passes its sai-script chip test on the 400G
  cage 1 <-> 16 loop ([sai-features.md](sai-features.md), [sai-hw-integration-20261116.txt](sai-hw-integration-20261116.txt)).
- **Full SONiC on the loop, started by hand** (`scripts/sonic-handrun.sh`, no generation), xcvrd on:
  Vrf-a/Vrf-b eBGP + BFD over PortChannels (LACP) and over plain ports, routes offloaded both ways,
  ASIC transit routing, ACL drop, DSCP->queue, CoPP policing, port/queue/RIF/ACL counters, CRM
  ([evidence](sai-hw-sonic-20261116.txt)). 2 h soak with 11 link/LAG flaps: BGP + BFD back 1-7 s after each, no loss outside the flaps, daemon RSS flat, I2C FPGA healthy throughout.
- **The 08:13 stall was a dead I2C FPGA** (0002:01:00.0 stopped answering while SONiC started; each
  access then stalls the whole SoC and xcvrd kept reading 32 all-ones "present" cages). Trigger not
  found; the platform now serialises all FPGA access, only xcvrd touches it, and the root port's own
  status (or all-ones) marks it dead for every process ([analysis](sai-hw-fpga-stall-20261116.txt)).
- **Recover:** `sonic-handrun.sh down` (stops the stack, removes the runtime units, users, links,
  firewall rules), then `cs8210-switch` as before. A dead FPGA (`/run/cs8210-qsfp.dead`, or the
  cage guard refusing) took a power cycle on 2026-11-16 (PCIe remove/rescan not tried).
- **Code:** [packages/sai-teralynx](../../nix/packages/sai-teralynx/README.md), [boards/cs8210](../../nix/modules/boards/cs8210/sonic.nix)
  (ports, IFCS yaml, sai.profile incl. the diag socket, buffermgrd, pmon), `nepeat/nix`
  `nixos/module/cs8210/sonic` (cross build, Credo unit, guard, watchdog).

## Running SONiC by hand

Build the machine with `hardware.cs8210.sonic.enable = lib.mkForce true` and `lineMedia = { "1" = "copper";
"16" = "copper"; }` through `extendModules` (git+file flake `?dir=nix`, the module reaches `ai_re/nix`), copy
the closure, then on the box (ASIC lease held, only cages 1 and 16 populated, cs8210-switch stopped):
`sonic-handrun.sh up <toplevel> <config_db.json>` / `status` / `down`. It links the closure's sonic units
into /run/systemd/system and adds what they need for the run only (users frr/frrvty/redis-sonic, three
sysctls, modules, tmpfiles lines, /etc/sonic and /etc/frr/vtysh.conf, BGP/BFD/ICMP firewall rules on
Ethernet+/PortChannel+/Vlan+/Vrf+; stops the host lldpd). Diag: `openbcm-diag -s
/run/sai-teralynx/diag.sock copp|acl|counters <devport>|devport <n>`.

Test configs in [sonic-loop/](sonic-loop/): `config-a-vrf-bgp.json` (Ethernet0 in Vrf-a 10.0.0.0/31,
Ethernet120 in Vrf-b 10.0.0.1/31) and `config-b-vrf-bgp-lag.json` (the same over PortChannel1/2,
10.0.1.0/31, plus Loopback1/2 10.66.1.1 / 10.66.2.1 in the VRFs); both with BGP + BFD, an L3 ACL,
AZURE-style DSCP/TC maps, schedulers, lossy buffers, flex counters PORT/QUEUE/RIF/ACL. What the loop
needs (one box is both ends):

- **A MAC per router interface** (INTERFACE / PORTCHANNEL_INTERFACE `mac_addr`): with the one system MAC
  on both RIFs every unicast frame has SA = DA and the ASIC drops it (ARP works, nothing else does).
  PortChannels also need distinct `system_mac` (the LACP system id), set equal to their `mac_addr`.
- **BGP by vtysh** after each bgpd start (bgpcfgd renders one ASN for all VRFs, no `local_asn`), in Vrf-b
  `neighbor <a> local-as 65200 no-prepend replace-as` and `allowas-in 1`; bgpd then counts that peer as
  iBGP and runs its BFD multihop, so Vrf-a gets `ebgp-multihop 2` too and both peers `no neighbor <x> bfd`
  + `neighbor <x> bfd` (one-hop vs multihop sessions never come up). Exact lines:
  [sai-hw-sonic-20261116.txt](sai-hw-sonic-20261116.txt).
- **Firewall:** a VRF's local traffic reaches INPUT with the VRF device as its interface; Ethernet+
  rules alone leave BGP in Connect. The deployed firewall (iptables here) now gets frr.nix/bfd.nix
  chains for every interface but `frr.managementInterfaces` (enp0s1, enp0s3 on this board).

Soak: [scripts/sonic-soak.sh](scripts/sonic-soak.sh) (state + per-daemon RSS each minute, a flap every
10 min alternating Ethernet0 / PortChannel1 admin down 30 s, a 5/s ping across the loop).

Module bugs this found (fixed, 2026-11-16): the SDK dir in syncd's LD_LIBRARY_PATH (doubled prefix,
jansson's -dev output: libifcs dlopen failed), the Credo loader's path inside the chroot, unit `path` entries
ending in /bin (cs8210-qsfp not found), a stale /tmp/cls_phy_fifo hanging the Credo loader, no buffermgrd
(ports never admin up), queue RESERVED_SIZE refused by 0.12.41, the cage guard's `read` at EOF under
`set -e`. Notes from the AS7760 runs: [sonic-nixos](../sonic-nixos/PROGRESS.md).

## xcvrd and the I2C FPGA

The cages' management FPGA (PCI 1d0f:8571 behind root port 0002:00:00.0) is read by mmap of its BAR0
(`boards/cs8210/sonic_platform/fpga.py`). When it stops answering, every BAR or config access is a PCIe
completion timeout that stalls the whole SoC for seconds (SSH, fand's I2C, journald all time out; hard-IRQ
time rises on all CPUs); nothing reaches the kernel log (no AER: external-PCIe INTx is not delivered).
Since 2026-11-16: all access under cs8210-qsfp's flock (one at a time, any process), Chassis() reads
nothing (only xcvrd uses the cages), sideband snapshots reused 0.25 s, and before each session the root
port's DevSta/LnkSta/AER (on the SoC, never stalls) or an all-ones register read marks it dead in
`/run/cs8210-qsfp.dead`; then no process touches it until a reboot. The cage guard checks the same and
refuses syncd. xcvrd starts after the Credo bring-up. Modules stay in reset (read-only platform), so the
DAC loop's EEPROMs are not read: xcvrd reports presence only.

## SAI source: our own, on the vendor IFCS SDK

- **No arm64 Teralynx SAI exists.** All 98 published Innovium/Marvell SAI packages
  (`Innovium/SONiC` 201811..202405, `Marvell-switching/sonic-marvell-teralynx-binaries` up to SAI
  7.0.1 / OCP SAI 1.18.1, incl. the `-vs` builds) are `Architecture: amd64`
  ([list](../sonic-nixos/cs8210-sai-search-20261115.txt)); sonic-buildimage only knows x86 Teralynx
  boxes (Celestica Midstone-200i). The vendor SDK on this box has `libifcs.so` and no libsai.
- **Rejected: Marvell's amd64 SAI under qemu-user.** It wants ipd 0.16 (we have it), but qemu-user
  answers every ioctl it has no thunk for with ENOTTY, and the SDK lives on ipd ioctls; plus the
  SerDes firmware download and the DMA rings under emulation.
- **Chosen:** sai-openbcm's hardware-agnostic frontend (shared with the AS7760; its optional backend hooks default to the AS7760's behaviour)
  + a new IFCS backend. SAI ABI = sairedis' pin (OCP SAI 1.19.2), by construction. IFCS
  interop declarations are our own, from the SDK's `pyctypes/ifcs_ctypes.py` and checked against
  the DWARF of `innovium.user` ([README](../../nix/packages/sai-teralynx/README.md)).
- **Kernel driver:** the existing ipd 0.16.2 with `legacy_abi=1` (the 2020 libifcs speaks 0.12.41).

## Vendor files at run time (never in git or the store)

From `/var/lib/cs8210-sdk/root/root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK`: `libifcs.so`
`388031e7eb24…`, `libAAPL.so` `cf4e84849839…`, `cel_configs/isg.yaml` `377e3b97…` (or
`isg_R01.yaml` `c1f85dd7…` for board version 2; this unit is version 1), the pre-emphasis tables in
`cel_cmds/` (re-indexed per devport by `boards/cs8210/sai-files.py`), and the Credo `c-phy/` tools
+ firmware `B52b0.lz4.1.92.2.bin` (run in the chroot by `sonic-cs8210-credo`). The IFCS SKU file is
the public sonic-buildimage `innovium.77700_A` (byte-identical to the SDK's `innovium.77701_A`).

## Board layer facts

- Ports: 32 cages x 8 lanes, lane = serdes_group x 8 + k + 1 (Marvell/Innovium SONiC convention);
  names `Ethernet0, 8, …`, aliases `Ethernet<cage>/<n>`; groups from the vendor `dynamic_config.sh`.
  1x100G/1x40G/4x25G/4x10G use lanes 0-3 (`allowUnusedLanes`). Devport numbering = the vendor's.
- Generated IFCS yaml, pre-emphasis and link-training lines equal the vendor's `dynamic_config.sh`
  output for 32x400, the lab 4x10G layout and every mode ([check](sonic-sdk-config-20261115.txt),
  [script](scripts/sonic_sdk_config_check.py)).
- pmon (read-only): fans/LM75/ASIC from fand, PSUs from the sys CPLD, cages through the I2C FPGA,
  syseeprom from the TLV EEPROM ([tests](sonic-platform-test-20261115.txt)). PSU slot 2-4 presence
  bits look wrong (only PSU 1 fitted, but 2-4 decode as present); xcvrd runs `--skip_cmis_mgr`.
- `asic_type` = `marvell-teralynx` (upstream's value; differences from `broadcom` in the pinned
  orchagent: combined V4V6 ACL tables allowed, PFC watchdog handler, no DNAT tracking).

## Found on the chip (2026-11-16)

- `libifcs.so` references 19 CPython 2.7 functions (its CLI hooks): `dlopen(RTLD_NOW)` fails.
  `libsai-teralynx-shim.so` stubs them (log once, fail).
- **SIGBUS in glibc's `__memcpy_generic` inside `ifcs_init`**: libifcs memcpy()s to/from the PCI BAR,
  and current glibc's aarch64 memcpy (unaligned, 128-bit SIMD) faults on Device memory; the vendor's
  glibc 2.25 did not. The loader points libifcs' `memcpy` GOT entries at an aligned 8/4/1-byte copy
  loop (`src/shim.c`, `ifcs_lib.cpp`). Any other vendor .so run outside its chroot needs the same.
- Ordering: Credo PHYs first, then the ASIC (the module's unit order) gives link in ~4 s.
- The slow getter was the temperature read: ~400 ms per monitor inside libifcs, holding its lock
  (any IFCS call waited). Now one monitor per 15 s ([sai-oracle.md](sai-oracle.md)).

## Board notes for SONiC

- LEDs: the vendor's `pci write led_*` lines are not applied (LEDs dark); devport 129 (AUX to
  `enp0s3`) stays down.
- The frontend's fake-backend guard only knows Broadcom devices: never set `SAI_OPENBCM_BACKEND=fake` here.
- Cage modules are held in reset at power-up (FPGA reg 0x178); the read-only platform does not release
  them, so their EEPROMs stay unread (`cs8210-qsfp --reset P 0` by hand).

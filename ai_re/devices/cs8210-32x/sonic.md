# CS8210-32X: SONiC on NixOS

Cross-device notes and the stack itself: [sonic-nixos](../sonic-nixos/PROGRESS.md).

## Status (2026-11-16)

- **Our SAI runs the chip** (`sai-teralynx`: sai-openbcm's frontend + an IFCS backend dlopen()ing the
  vendor `libifcs.so` 0.12.41): every phase B/C feature passes its sai-script chip test on the 400G
  cage 1 <-> 16 loop ([sai-features.md](sai-features.md), [sai-hw-integration-20261116.txt](sai-hw-integration-20261116.txt)).
- **SONiC on it, started by hand** (`scripts/sonic-handrun.sh`, no generation): redis, syncd, orchagent,
  the managers, vrfmgrd, coppmgrd, teammgrd, FRR (zebra, bgpd, bfdd), bgpcfgd, LLDP and pmon came up;
  all 32 netdevs adopted with the system MAC. Not yet reached: link up, BGP/BFD, ACL, counters, soak.
- **Blocked (2026-11-16 08:13 box time):** after the 4th start the box stalled (55 % hard-IRQ time,
  xcvrd 114 % CPU, SSH timeouts ~20 min); since then the I2C FPGA 0002:01:00.0 reads all-ones, so
  cage presence is unreadable and the cage guard refuses syncd. Needs a reboot (or a PCIe
  remove/rescan of that device, not tried). xcvrd is off by default on this board now.
- **Recover:** `sonic-handrun.sh down` (stops the stack, removes the runtime units, users, links,
  firewall rules), then `cs8210-switch` as before.
- **Code:** [packages/sai-teralynx](../../nix/packages/sai-teralynx/README.md), [boards/cs8210](../../nix/modules/boards/cs8210/sonic.nix)
  (ports, IFCS yaml, sai.profile incl. the diag socket, buffermgrd, pmon), `nepeat/nix`
  `nixos/module/cs8210/sonic` (cross build, Credo unit, guard, watchdog).

## Running SONiC by hand

Build the machine with `hardware.cs8210.sonic.enable = lib.mkForce true` and `lineMedia = { "1" = "copper";
"16" = "copper"; }` through `extendModules` (git+file flake `?dir=nix`, the module reaches `ai_re/nix`), copy
the closure, then on the box (ASIC lease held, only cages 1 and 16 populated, cs8210-switch stopped):
`sonic-handrun.sh up <toplevel> <config_db.json>` / `status` / `down`. It links the closure's sonic units
into /run/systemd/system and adds what they need for the run only (users frr/frrvty/redis-sonic, three
sysctls, modules, tmpfiles lines, /etc/sonic and /etc/frr/vtysh.conf, BGP/BFD firewall rules; stops the host
lldpd). Test config: [sonic-loop/config-a-vrf-bgp.json](sonic-loop/config-a-vrf-bgp.json) (Ethernet0 in Vrf-a
10.0.0.0/31, Ethernet120 in Vrf-b 10.0.0.1/31, BGP + BFD between them, an L3 ACL, AZURE-style DSCP/TC maps,
schedulers, lossy buffers, flex counters PORT/QUEUE/RIF/ACL). bgpcfgd has no `local_asn`: eBGP between the
VRFs needs `neighbor 10.0.0.0 local-as 65200` in Vrf-b by vtysh. Diag: `openbcm-diag -s
/run/sai-teralynx/diag.sock copp|acl|counters <devport>|devport <n>`.

Module bugs this found (fixed, 2026-11-16): the SDK dir in syncd's LD_LIBRARY_PATH (doubled prefix,
jansson's -dev output: libifcs dlopen failed), the Credo loader's path inside the chroot, unit `path` entries
ending in /bin (cs8210-qsfp not found), a stale /tmp/cls_phy_fifo hanging the Credo loader, no buffermgrd
(ports never admin up), queue RESERVED_SIZE refused by 0.12.41. Notes from the AS7760 runs:
[sonic-nixos](../sonic-nixos/PROGRESS.md).

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
  them (the loop cages are released already this boot).

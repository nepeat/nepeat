# CS8210-32X: SONiC on NixOS

Cross-device notes and the stack itself: [sonic-nixos](../sonic-nixos/PROGRESS.md).

## Status (2026-11-16)

- **Our SAI runs the chip:** `sai-teralynx` (sai-openbcm's SAI frontend + an IFCS backend that
  dlopen()s the vendor `libifcs.so` 0.12.41 at run time) initialised the Teralynx 7 from our
  generated init yaml, created all 32 ports with the right lanes, brought the cage 1 <-> 16
  400G CR8 DAC loop up (oper up in ~4 s, through both Credo PHYs in MACsec bypass), adopted
  the SDK netdevs as `Ethernet0`/`Ethernet120`, read the die temperature (52-54 C) and tore
  down cleanly (ports down, netdevs gone, `ifcs_deinit`, ipd closed). Driven directly by
  `sai-teralynx-smoke`, no redis/syncd/orchagent yet ([evidence](../sonic-nixos/cs8210-sai-hw-20261116.txt)).
- **Not run yet:** the SONiC stack itself (syncd/orchagent/FRR), any traffic, LLDP.
- **Builds:** libsai and syncd cross-built for aarch64; the cs8210 system with
  `hardware.cs8210.sonic.enable = true` evaluates (build: entry below). With it off the system
  is unchanged (nix-diff: only the flake source path).
- **Switch it on:** `hardware.cs8210.sonic.enable` (machine; default off, not started at boot unless
  `autoStart`): `systemctl start sonic.target`. Mutually exclusive with `cs8210-switch`
  (syncd `Conflicts=`). A guard refuses syncd unless only `allowedCages` (default 1, 16) hold modules.
- **Recover:** `systemctl stop sonic.target` (or `sai-smoke`), then `cs8210-switch` as before.
- **Code:** [packages/sai-teralynx](../../nix/packages/sai-teralynx/README.md) (SAI, profile keys,
  what is mapped), [boards/cs8210](../../nix/modules/boards/cs8210/sonic.nix) (ports, IFCS yaml
  generator, runtime vendor files, pmon), `nepeat/nix` `nixos/module/cs8210/sonic` (machine glue:
  cross build, Credo unit, guard, watchdog).

## SAI source: our own, on the vendor IFCS SDK

- **No arm64 Teralynx SAI exists.** All 98 published Innovium/Marvell SAI packages
  (`Innovium/SONiC` 201811..202405, `Marvell-switching/sonic-marvell-teralynx-binaries` up to SAI
  7.0.1 / OCP SAI 1.18.1, incl. the `-vs` builds) are `Architecture: amd64`
  ([list](../sonic-nixos/cs8210-sai-search-20261115.txt)); sonic-buildimage only knows x86 Teralynx
  boxes (Celestica Midstone-200i). The vendor SDK on this box has `libifcs.so` and no libsai.
- **Rejected: Marvell's amd64 SAI under qemu-user.** It wants ipd 0.16 (we have it), but qemu-user
  answers every ioctl it has no thunk for with ENOTTY, and the SDK lives on ipd ioctls; plus the
  SerDes firmware download and the DMA rings under emulation.
- **Chosen:** sai-openbcm's hardware-agnostic frontend (unmodified; the AS7760 stays byte-identical)
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
- One report tick of the smoke tool took ~5 s instead of 2 s: some getter (temperature or stats) is
  slow; matters for flex-counter polling. Not isolated yet.

## Before the first SONiC run

- Deploy a generation with `hardware.cs8210.sonic.enable = true`, `lineMedia = { "1" = "copper"; "16" = "copper"; }`
  (the loop is a DAC) and `autoStart = false`; start `sonic.target` by hand.
- Traffic/LLDP: no protocol traps in the backend (`trap_set` NOT_SUPPORTED) and no LAG/ACL; frames
  reach the CPU only via VLAN floods and TRAP routes. **Two looped ports in one VLAN would storm**
  (SONiC removes ports from VLAN 1, but check the SDK's default L2 state, DESIGN.md U1, before sending).
- Unknown on the chip: `ifcs_netdev_create` (the default netdev mode; the run used `adopt`), L2/L3 and
  ECMP mappings, link-event struct, aging units, MTU semantics (SDK netdev MTU 9462 vs the ASIC's
  9216-byte limit: keep SONiC's 9100).
- LEDs: the vendor's `pci write led_*` lines are not applied (LEDs dark); devport 129 (AUX to
  `enp0s3`) stays down.
- The frontend's fake-backend guard only knows Broadcom devices: never set `SAI_OPENBCM_BACKEND=fake` here.
- Cage modules are held in reset at power-up (FPGA reg 0x178); the read-only platform does not release
  them (the loop cages are released already this boot).

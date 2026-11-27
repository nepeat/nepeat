# CS8210-32X: SONiC on NixOS

Cross-device notes and the stack itself: [sonic-nixos](../sonic-nixos/PROGRESS.md).

## Status (2026-11-25)

- **New cabling (2026-11-25):** 1 <-> 2 and 31 <-> 32 400G-DR4 optics, 3 <-> 17 400G CR8 DAC; Nix-base config of the
  three loops ("Running it"); all three loops up with ping across since 2026-11-26 (1 <-> 2 had been down on lane 4:
  cat fur on the fibre / optic faces, both modules replaced by the owner; PROGRESS 2026-11-26).
  Cages: cs8210-optics ("Optics"); LEDs: "LEDs".
- **Our SAI runs the chip** (`sai-teralynx`: sai-openbcm's frontend + an IFCS backend dlopen()ing the
  vendor `libifcs.so` 0.12.41): every phase B/C feature passes its sai-script chip test on the 400G
  cage 1 <-> 16 loop ([sai-features.md](sai-features.md), [sai-hw-integration-20261116.txt](sai-hw-integration-20261116.txt)).
- **SONiC at boot** (deployed 2026-11-16, two reboots: BGP/LACP ~62 s, BFD ~64 s after kernel start; [evidence](sai-hw-sonic-boot-20261116.txt)):
  "Running it" below. The loop config needs no vtysh (BGP_NEIGHBOR `local_asn`, STATIC_ROUTE advertisements).
- **Full SONiC on the loop, first by hand** (`scripts/sonic-handrun.sh`), xcvrd on:
  Vrf-a/Vrf-b eBGP + BFD over PortChannels (LACP) and over plain ports, routes offloaded both ways,
  ASIC transit routing, ACL drop, DSCP->queue, CoPP policing, port/queue/RIF/ACL counters, CRM
  ([evidence](sai-hw-sonic-20261116.txt)). 2 h soak with 11 link/LAG flaps: BGP + BFD back 1-7 s after each, no loss outside the flaps, daemon RSS flat, I2C FPGA healthy throughout.
- **The 08:13 stall was a dead I2C FPGA** (0002:01:00.0 stopped answering while SONiC started; each
  access then stalls the whole SoC and xcvrd kept reading 32 all-ones "present" cages). Trigger not
  found; since 2026-11-25 one process owns the FPGA (cs8210-optics, "Optics"), and the root port's own
  status (or all-ones) marks it dead for every process ([analysis](sai-hw-fpga-stall-20261116.txt)).
- **Recover:** `systemctl stop sonic.target` (or `systemctl start cs8210-switch`, which stops syncd), boot
  an older generation from the U-Boot menu, or `autoStart = false`. A dead FPGA (`/run/cs8210-qsfp.dead`, or the
  cage guard refusing) took a power cycle on 2026-11-16 (PCIe remove/rescan not tried).
- **Code:** [packages/sai-teralynx](../../nix/packages/sai-teralynx/README.md), [boards/cs8210](../../nix/modules/boards/cs8210/sonic.nix)
  (ports, IFCS yaml, sai.profile incl. the diag socket, buffermgrd, pmon), `nepeat/nix`
  `nixos/module/cs8210/sonic` (cross build, Credo unit, guard, watchdog), `nixos/module/cs8210/optics` + `rust/cs8210`
  (cages, LEDs).

## Running it

**At boot** (machine `cs8210`: `hardware.cs8210.sonic.enable`, `autoStart` is the default): sonic.target from
multi-user.target. Order: `cs8210-pcie-errors` (all-ones reads) -> `cs8210-optics` (cages, ~21 s from power-up to
the last module up) -> `sonic-cs8210-guard` (FPGA alive, every populated cage declared) -> `sonic-cs8210-credo`
(MACsec bypass, ~39 s) and `cs8210-sai-files` -> syncd (~51 s) -> orchagent, managers, FRR; xcvrd after the guard
and Credo. CONFIG_DB = the Nix base (machine `cs8210`, since 2026-11-25): the three loops' six ports at 1x400G RS
FEC, Ethernet0/16/240 in the default VRF (10.0.1.0/31, 10.0.3.0/31, 10.0.31.0/31), the far ends Ethernet8/128/248
in Vrf-a/b/c (.1/31, MACs 0e:48:c6:7a:55:cf/d0/d1); the other 26 ports down; LLDP, CoPP, PORT counters. No saved
file (`/var/lib/sonic/config_db.json` of the old cage 1 <-> 16 loop moved to `config_db.json.cage1-16-loop-20261116`);
`config save` would make one. Test: `ping -I Ethernet16 10.0.3.1`, `ping -I Ethernet240 10.0.31.1`
([evidence](evidence/links-20261125.txt)). Credo line side: cage 3 copper (the DAC), 1 and 2 optics; ASIC side:
cage 17 copper, 31/32 optics (`lineMedia`, `media`); at boot `cs8210-optics credo-spec` overrides the Credo media per
cage from the module in it (passive copper media type 03h -> `copper_*`, line AN/LT; anything else -> `optics_*`, none). `sonic-nix reload` without a saved file loads the base. Recovery as on the AS7760 (shared recovery.nix):
orchagent crash -> syncd restart, teamd respawn, reload ([evidence](sai-hw-sonic-boot-20261116.txt)).
Vendor stack instead: `systemctl start cs8210-switch` (Conflicts= stops syncd and the Credo unit); back:
`systemctl start sonic.target`. Diag: `openbcm-diag -s /run/sai-teralynx/diag.sock copp|acl|counters <devport>|devport <n>`.

**When libifcs wedges** (the learn thread's garbage-index bug, [learn-wedge.md](learn-wedge.md)): our libsai's
learn-ring guard re-reads an all-ones index and skips that poll (journal `sai-teralynx: ifcs guard: ...`, counters
`sai_teralynx_ifcs_guard_*`). Backstops: syncd's MemoryMax (`syncdMemoryMaxMiB`, 2 GiB; under the vm.overcommit_memory
= 1 that the NixOS redis module sets, the unguarded ~32 GiB memset took the box's memory on 2026-11-16,
[oom-20261116.txt](oom-20261116.txt)) and `sonic-cs8210-ifcs-watchdog` (VmSize, RSS, learn-thread CPU, the kernel line,
guard, cpu-tx, tx-stall with a host-side witness; restart, then ipd reload, then alarm; never under the ASIC lease;
`--once --dry-run` prints its readings, `--metrics` the `cs8210_ifcs_watchdog_*` / PCIe AER / link / ipd ring
families of the `cs8210-ifcs` producer; details: learn-wedge.md section 6). Prime suspect for the ASIC going bad: ASPM L0s on its PCIe link (off by `cs8210-asic-aspm`).

**Another closure by hand:** `scripts/sonic-handrun.sh up <toplevel> <config_db.json>` / `status` / `down` links
the closure's units into /run/systemd/system and adds users, sysctls, modules, tmpfiles, /etc entries and
firewall rules for the run. Only on a generation with SONiC off (/etc units win over /run; `up` refuses
otherwise). `down` also puts vrfmgrd's local rule back at pref 0 (left at 1001, a networkd restart cut the host
off: sai-hw-sonic-boot-20261116.txt, section 2), deletes the Loopback/Bridge/dummy netdevs the run added and
restarts the host lldpd once SONiC's is gone.

Test configs of the old cage 1 <-> 16 DAC loop (2026-11-16) in [sonic-loop/](sonic-loop/): `config-a-vrf-bgp.json` (Ethernet0 in Vrf-a 10.0.0.0/31,
Ethernet120 in Vrf-b 10.0.0.1/31) and `config-b-vrf-bgp-lag.json` (the same over PortChannel1/2,
10.0.1.0/31, plus Loopback1/2 10.66.1.1 / 10.66.2.1 in the VRFs, advertised as STATIC_ROUTE blackholes
10.66.x.0/24); both with BGP + BFD, an L3 ACL, AZURE-style DSCP/TC maps, schedulers, lossy buffers, flex
counters PORT/QUEUE/RIF/ACL. What the loop needs (one box is both ends), all in CONFIG_DB (no vtysh since 2026-11-16):

- **A MAC per router interface** (INTERFACE / PORTCHANNEL_INTERFACE `mac_addr`): with the one system MAC
  on both RIFs every unicast frame has SA = DA and the ASIC drops it (ARP works, nothing else does).
  PortChannels also need distinct `system_mac` (the LACP system id), set equal to their `mac_addr`.
- **A local AS per side** (BGP_NEIGHBOR `local_asn` + `local_as_no_prepend` + `local_as_replace_as`, our
  bgpcfgd patches; bgpcfgd renders one ASN, 65100, for every VRF): Vrf-a presents 65101 to a 65201 peer and
  Vrf-b the reverse, so both sessions are eBGP with one-hop BFD and no AS path holds 65100 (no allowas-in).
  The 2026-11-16 hand-run did this by vtysh with one side iBGP, which needed multihop BFD on both.
  `BGP_GLOBALS|Vrf-a`/`Vrf-b` are there for YANG only (BGP_NEIGHBOR's VRF leafref; bgpcfgd ignores the table).
- **Firewall:** a VRF's local traffic reaches INPUT with the VRF device as its interface; Ethernet+
  rules alone leave BGP in Connect. The deployed firewall (iptables here) gets frr.nix/bfd.nix
  chains for every interface but `frr.managementInterfaces` (enp0s1, enp0s3 on this board).

Soak: [scripts/sonic-soak.sh](scripts/sonic-soak.sh) (state + per-daemon RSS each minute, a flap every
10 min alternating Ethernet0 / PortChannel1 admin down 30 s, a 5/s ping across the loop).

Module bugs this found (fixed, 2026-11-16): the SDK dir in syncd's LD_LIBRARY_PATH (doubled prefix,
jansson's -dev output: libifcs dlopen failed), the Credo loader's path inside the chroot, unit `path` entries
ending in /bin (cs8210-qsfp not found), a stale /tmp/cls_phy_fifo hanging the Credo loader, no buffermgrd
(ports never admin up), queue RESERVED_SIZE refused by 0.12.41, the cage guard's `read` at EOF under
`set -e`; for the boot: syncd before /dev/ipd, FPGA/ASIC access before the all-ones setup, the vendor
stack's bond0/vrf-fp (off with SONiC), networkd deleting vrfmgrd's local rule on a restart, teamd respawn
resetting a PortChannel's mac_addr. Notes from the AS7760 runs: [sonic-nixos](../sonic-nixos/PROGRESS.md).

Found at H3 (SAI-independent, same on the C++ and the Rust SAI; [evidence](evidence/rust-h3/00-index.txt)): `sonic-nix reload` races
the VRFs (intfmgrd binds Ethernet128 to Vrf-b before vrfmgrd has re-created it), and FRR keeps BFD on the old interface when an
address moves onto a PortChannel (workaround not verified).

## Optics: cs8210-optics (the I2C FPGA's one owner)

`cs8210-optics` (Rust, monorepo `rust/cs8210`; NixOS `nixos/module/cs8210/optics`, unit `cs8210-optics`) is the
only process that touches the cages' FPGA (PCI 1d0f:8571 behind root port 0002:00:00.0). It holds the flock
`/run/lock/cs8210-qsfp.lock` for its life, checks the root port (link, DevSta, AER CmpltTO; on the SoC, never stalls)
before every poll and request, treats all-ones or a changed version register as a dead FPGA and then writes
`/run/cs8210-qsfp.dead` and touches nothing until a reboot (the 2026-11-16 stall: each access to a dead FPGA stalls
the whole SoC; [analysis](sai-hw-fpga-stall-20261116.txt)). It turns on the FPGA's PCI memory decode (off after U-Boot).

- **Declared cages** = the cages of the admin-up ports in the Nix base (`hardware.cs8210.optics.cages`, derived in
  `nixos/module/cs8210/sonic`): channel probed (400 kHz), ResetL released, the module brought up. Every other cage,
  and an emptied one, is held in ResetL (a module plugged into an undeclared cage stays dark; the cage guard then
  refuses syncd). LPMode is left deasserted (FPGA 0x170 = 0, so modules reach high power by the pin).
- **Bring-up** (`cs8210-xcvr` `bringup`): SFF-8636 high-power class enable; CMIS LowPwrRequestSW cleared, ModuleReady,
  then on a paged module the application whose host lanes and rate fit the cage's ports (1x400G here): Deinit ->
  staged ApSel -> Apply_DataPathInit -> ConfigSuccess -> Deinit cleared + Tx on -> DataPathActivated. The Molex
  1064281405 DR4s power up on app 2 (4 x 100G-FR1) and go to app 1 (400GBASE-DR4) in about 3 s; the DACs need
  nothing. A module already up (daemon restart, warm reboot) is adopted without a write. Nothing latches: a
  failed bring-up is retried every 60 s for ever, and a module that was up and is found out of ModuleReady /
  DataPathActivated on two monitor reads in a row (10 s apart; not while LPMode is asserted on purpose) is brought
  up again. xcvrd keeps `--skip_cmis_mgr` (one state machine per module).
- **SONiC**: `sonic_platform`'s Sfp/Chassis call the daemon's socket (`/run/cs8210-optics/optics.sock`, JSON lines;
  `optics.py`): sideband, optoe flat-offset reads/writes, LPMode, reset. `show interfaces transceiver
  presence|eeprom -d` and the TRANSCEIVER_* tables work ([evidence](evidence/optics-20261125.txt)). Every page select is
  read back: a page the module does not have is an error, not page 00h's bytes. The Molex DR4s advertise VDM but
  refuse page 2Fh (VDM control / freeze), so sonic_platform reports no VDM statistics for them and xcvrd never
  freezes; its CDB firmware query (page 9Fh, "enter password" here) runs once per module ([evidence](evidence/links-20261126.txt)).
- **Metrics**: `/run/prometheus-textfile/cs8210-optics.prom`, the board-neutral `switch_optic_*` of the AS7760
  (info, temperature, Vcc, per-lane Rx/Tx power and bias: Grafana "switch optics") + `cs8210_optics_*` (fpga_up,
  module_ready, bringup_failed, reset_held, i2c errors, Rx LOS / Tx fault, Rx dBm). Refreshed every 10 s.
- **CLI**: `cs8210-optics status [N]` (lanes with N), `json`, `sideband`, `dump N` (all pages, hex), `read N OFF LEN`,
  `write`, `lpmode N 0|1`, `reset N` (ResetL pulse, then a fresh bring-up), `retry N`, `guard --allowed LIST`.
  Without the daemon nothing can reach the FPGA (`systemctl start cs8210-optics`).

Cage guard (`sonic-cs8210-guard`, before syncd): `cs8210-optics guard --allowed <declared cages>`: the FPGA answers
and every populated cage is declared, else syncd does not start (house network, PROGRESS 2026-11-11).

**Credo PHY die temperatures**: `cs8210_credo_temperature_celsius{cage}` (cages 1-16) every 60 s, producer
`cs8210-credo`: the vendor's query `firmware_get_slice_temp slice_id=all` through `phy_shell` in the SDK chroot, so
phy_server (the MDIO FPGA's one owner) answers it; `phy_shell`'s `cls_phy_cmd_running` flag serialises it with
cs8210-macsec. 45-52 C idle (2026-11-26). Not a fand input (fan policy is the owner's).

## LEDs

- **Port LEDs: the ASIC's LED stream, which libifcs sets up itself (corrected 2026-11-26; V / I as tagged).** The
  two LED CPLDs (MACsec LED 0x63: cages 1-16; switch LED 0x64: cages 17-32) decode the Teralynx's serial LED stream;
  the CPU can only force a whole CPLD to one test colour (reg 0x09 bit 0 = test, 0x0a bits 2:0 = colour: red 6,
  green 3, blue 5, white 0, off 7; yellow 2, cyan 1, magenta 4 inferred from the codes being active-low R/B/G bits).
  libifcs 0.12.41 programs the stream at node create, under any SAI: LED_CFG sck_rate 5 + pic_stream_mode 1,
  LED_REFRESH_PRECLIFF 0x18eec2, LED_REFRESH_CLIFF 0x15e, LED_REFRESH_TMR_CTL enable (V: the box's
  `analysis/onie-live-2026-11-10/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK/libifcs.so` 0x110bd48-0x110be24, called from
  `ii_node_mod_create_impl`). Its LED_CFG read-modify-write takes LEARN_MSG 0x48bc as the base, not LED_CFG, so
  LED_CFG bits 4-10 (activity blink, lane / link / fault masks, sdo polarity) were predicted to hold LEARN_MSG's bits (V: 0x110bd68;
  libifcs 6.2.0 reads 0x13514); **refuted by H1 (V, 2026-11-26): the chip's LED_CFG reads 0x805, bits 4-10 zero** ([R1 table](evidence/rust-h1/13-led-r1-table.txt)). The vendor init lines our SAI skips
  (`SAI_TERALYNX_INIT_CMDS` runs only `ifcs set devport` lines) only retune it: `pci write led_cfg sck_rate_f 0x8`
  (5 -> 8) and `led_refresh_cliff 0x020` (0x15e -> 0x20); their PIC_STREAM_MODE_F 1, precliff 0x18eec2 and
  ENABLE_F 1 are libifcs' own values (V: `analysis/sai-teralynx/vendor012/cel_cmds/misc_config.cmd` 35-39).
  Addresses (BAR0, from the SDK's `ifcs_ctypes` enum and the vendor ishell log): LED_CFG 0x13514 (sck_rate_f bits
  3:0, link_up_mask_f bit 7, pic_stream_mode_f bit 11), LED_REFRESH_CLIFF 0x13530, LED_REFRESH_PRECLIFF 0x13538,
  LED_REFRESH_TMR_CTL 0x1353c (enable_f bit 1); per-PIC LED_SHIFT_MASK from 0x13540, LED_COLOR_MASK from 0x13620,
  LED_SWCTRL (software override) from 0x13700, stride 0x20 per IB. No libifcs code touches the per-PIC registers or
  the LED_BLINK timers by immediate address (V: scan of both libifcs versions; I: computed addresses not excluded).
  /dev/ipd allows one opener (syncd), so any LED write is a SAI change: the Rust SAI has a board register list
  applied after every init (`SAI_TERALYNX_POST_INIT_REGS`) and a read-only watch list in its diag `regs`
  (rust/sai README "Teralynx backend"); the read side ran at H1 (LED_CFG 0x805, refresh 0x15e / 0x18eec2, SHIFT/COLOR masks 0xffffff, SWCTRL 0 on all 33 PICs).
  **H2 register walk (V, 2026-11-26, Rust libsai through `SAI_TERALYNX_POST_INIT_REGS`, SONiC stopped, owner not at the panel; [evidence](evidence/rust-h2/01-led-walk.txt))**: every LED register tried is plain read/write at its declared width (92 steps: SWCTRL bits of cage 31's and cage 3's PIC, COLOR / SHIFT mask bits 0-23, LED_CFG sck_rate and bits 4-10, REFRESH_CLIFF, BLINK_TMR_CTL; 0 read-back mismatches) and no write moved any of the other 111 watched registers; the per-PIC words and the blink timer **survive a libifcs deinit + init** (libifcs re-programs only LED_CFG and the LED_REFRESH registers at node create), so an override outlives syncd until something writes it back (cold power cycle not tested); a hardware blink time base exists and is off after init (BLINK_TMR_CTL enable counts LED_BLINK to 250 and its prescaler to 10^6; which SWCTRL code blinks with it: panel only); every register was restored to R1 after each step. What the LEDs show for any of it: not observed, [led-panel-checklist.md](led-panel-checklist.md) (owner replay, ~25 steps).
- **What the 4 LEDs per cage show (owner at the panel, 2026-11-26, C++ SAI, no LED writes; V):** cage 3 up at
  1x400G = LED 1 magenta, LEDs 2-4 dark; cages 1 and 2 down = dark. Reading (I): 1x400G is one port at lane 0, so
  slot 0 of its PIC, shown on LED 1; magenta is the colour the reset COLOR_MASK gives (the predicted stray LED_CFG bits are not there, H1)
  a link-up slot. LED_SHIFT_MASK / LED_COLOR_MASK are 24 bits and LED_SWCTRL 16 bits per PIC: 8 slots (ports per
  PIC) x 3 / x 2 bits (I), so the 4 LEDs most likely show the sub-ports of a 4x100G breakout (I). Not readable
  while syncd runs except through the SAI (/dev/ipd has one opener, sysfs resource0 is claimed by ipd; the C++
  SAI's diag shell has no register read). The vendor material we hold has no per-LED description: diag
  `leds.yaml` / `cel-led-test` only drive the CPLD-wide test colour, `sys_cpld.ko` only knows led_enable +
  colour, the DIAGOS release notes only mention front-panel LED behaviour "according to the new sys CPLD", and
  public sonic-buildimage has no Teralynx LED config (Celestica Midstone-200i included). To settle the slot codes
  and colours: the owner replays the H2 register walk at the panel ([led-panel-checklist.md](led-panel-checklist.md)); for comparison the vendor stack (`systemctl start cs8210-switch`, its retuned sck_rate / cliff) on a
  1x400G link, a down port and a 4x100G breakout.
- **Proposal (owner's wish, plan only):** the SAI agent owns the policy inside the SDK owner (LED_SWCTRL per PIC, or
  PIC stream mode with per-lane masks); this side supplies health inputs it already has: per-lane Rx LOS / CDR LOL /
  Rx power vs the module's warning thresholds and module state from `cs8210-optics` (its socket and
  `switch_optic_*`), FEC corrected / uncorrectable deltas per port from the SAI counters. For a 1x400G cage: LED 1 =
  link (as today, magenta) and lanes 1-2, LEDs 2-4 = lanes 3-4 / 5-6 / 7-8 (dark = fine, amber = Rx power below the
  module's warning or a high corrected-FEC rate, blinking red = LOS / LOL / uncorrectable FEC), Arista-style; a
  breakout cage keeps one LED per sub-port with the same colours. Interface: `cs8210-optics` publishes a small
  per-cage health JSON (lane flags + Rx margin) the SAI polls at its LED refresh; nothing new touches the I2C FPGA.
- **System LED** (sys CPLD 0x20 bits 2:0): `cs8210-leds monitor` (unit `cs8210-leds`) keeps it green while fand
  runs without failsafe and the FPGA is alive, blinking amber otherwise (it was 7, undefined, before).
  PSU LEDs: the CPLD's own (0x21 test-mode bits clear). Fan tray LEDs: fand (green / blinking red / solid red,
  board-control.md "Fan CPLD register map").
- **Tests for a human at the panel** (each write read back by the tool): `cs8210-leds status`; `cs8210-leds test
  red|green|blue|white|off [--cages 1-16|17-32|all] [--seconds 30]` (all 16 cages of a CPLD in one colour, back to
  the stream after); `cs8210-leds walk` (every colour on all 32, then the halves, then the system LED states, ~35 s).
  [evidence](evidence/leds-20261125.txt).

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
  declarations are generated at build time from a public ctypes file (ishell 2.0.1), equal to the
  SDK's 0.12.41 for everything used ([README](../../nix/packages/sai-teralynx/README.md#ifcs-declarations)).
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
- pmon: fans/LM75/ASIC from fand, PSUs from the sys CPLD, cages through cs8210-optics,
  syseeprom from the TLV EEPROM ([tests](sonic-platform-test-20261115.txt)). PSUs: 2 bays (sys CPLD 0x08), PMBus
  readings from `cs8210-psu` ([board-control.md](board-control.md) "PSUs"); xcvrd runs `--skip_cmis_mgr` (cs8210-optics does CMIS).
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

- devport 129 (AUX to `enp0s3`) stays down. LEDs: "LEDs" above.
- The frontend's fake-backend guard only knows Broadcom devices: never set `SAI_OPENBCM_BACKEND=fake` here.

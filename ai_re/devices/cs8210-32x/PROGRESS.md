# CS8210-32X

## identity

- device model: Celestica CS8210-32X (user identification; Celestica CS8210
  confirmed by the SSD's device tree and diagnostic package). Codename: Briggs.
- chassis S/N: `C8210F2B02211113T0029` (provided label photo).
- SSD: Western Digital Ultrastar DC SA210, 120 GB M.2 SATA.
- SSD model: `HBS3A1912A4M4B1`; S/N: `A069BCC5`; P/N: `0TS1653`.
- SSD firmware: `M6102100`; exact capacity: 120,034,123,776 bytes.
- acquisition attachment: USB adapter, `/dev/sdb`; serial confirmed against photo.
- platform software: ARM64, Annapurna Labs Alpine; Innovium/Teralynx SDK.
- serial console: **115200 8N1, no flow control — confirmed 2026-11-09** on the
  console port via FTDI FT232R adapter; clean text from the first stage-2 loader
  byte through U-Boot and ONIE. Pinout and levels were not measured by the agent.
- tmux session `serial-cs8210`; power via `tools/lab_power.py` (HA lab plug).
- CPU: 16x Cortex-A72 (`0x41/0xd08`), 8 GB DDR (2 x 4 GB, SPD at 0x52), Alpine v3.
- boot chain: AL stage2/3 v3.65.1 -> U-Boot 2018.05-alpine_db-v3.65.1.015 ->
  ONIE 2017.11.015 (FIT from SPI `0x410000`), kernel 4.9.124+.
- EEPROM: CS8210-32X-UN-00, P/N R3181-F9003-01, rev R0B-2, mfg 2021-04-28 (TH),
  base MAC 0C:48:C6:7A:55:CE (4 MACs), BoardSN R3181-G0001-02TH0321110078.
- CPLDs: System 15, SWLED 2, MACLED 1, top/bottom fan 3.
- board components: not physically inspected by the agent; see storage notes in the
  2026-11-09 log entry.

## todo / standing notes

- **Fans** (user request 2026-11-09: slow them down, the unit is very loud at the
  CPLD power-on default). Done from Linux: `services.cs8210-fancontrol` disarms
  the fan watchdog and holds PWM 127 (~50%, ~10-11k rpm) below 35 C, ramping to
  full speed at 55 C; any error or service stop forces full speed and re-arms
  the watchdog. Manual control: `cs8210-fan status|set|quiet|max|watchdog`.
  **Still loud between power-on and NixOS userspace (~40 s)**: nothing in
  U-Boot touches the fans (no env var, no script, only the CPLD version
  banner). The U-Boot equivalent would be raw writes, not yet tried:
  `i2c dev 3; i2c mw 0x74 0.0 0x04` (top; `0x08` = bottom), then
  `i2c mw 0x66 0x0c 0x18` and `i2c mw 0x66 0x22 0x7f` ... `0x82`.
- Fan control scripts and hardware convenience scripts are in the nix tree
  (user request 2026-11-09): `nixos/module/cs8210/{fan-control,tools}`.
- Tach scale is an assumption: `cs8210-fan` prints raw x 100 rpm.

## current status

**NixOS 26.11 / mainline Linux 7.3.3 on the SSD, booting unattended.**
Working: serial console, 16 cores, 8 GiB, SATA SSD, management Ethernet
(al_eth + 88E1510), TPM 2.0, lldpd, I2C sensors, RTC, fan control, all three
external PCIe ports. **Switch side (started by hand, `systemctl start
cs8210-switch`): Teralynx 7 ASIC initialised by the vendor SDK running in a
chroot against Marvell's open-source `ipd` driver (legacy-ABI patch), 32
front-panel netdevs `fpp1`..`fpp32`, Credo MACsec PHYs on ports 1-16 with
firmware loaded, in MACsec bypass.**
Fans temperature-controlled including the ASIC die (about 55% at idle).
Not working / unproven: any link or traffic (no optics yet), Linux offload,
real MACsec keying, aux SPI flash, second NIC.
Config: `nepeat/nix`, flake output `cs8210`. The vendor userland lives only on
the switch (`/var/lib/cs8210-sdk/root`). ONIE in SPI flash is intact and is
the fallback boot; the factory DIAGOS exists only as the 2026-10-17 image.

Earlier offline work: [ssd-analysis.md](ssd-analysis.md),
[kernel-review.md](kernel-review.md), [offline research](offline-research.md),
[board-control.md](board-control.md), [serial candidate](bringup/README.md),
[Fable review triage](reviews/2026-10-17-triage.md).

## artifacts

- `images/ssd-2026-10-17.img.zst`: full-device image, 1,677,810,900 bytes
  compressed from 120,034,123,776 bytes (98.6% smaller).
- `evidence/`: acquisition metadata, partition table, SMART output, verification.
- `photos/`: original HEIF label photos and converted PNGs.
- `analysis/extracted/`: 856 selected firmware, SDK, diagnostic and identity
  files, 268,351,514 bytes logical size; hashed in `evidence/extracted-manifest.json`.
- `analysis/celestica_cs8210-r0.dts`: decompiled board device tree.
- Temporary raw analysis image was removed after inspection. All mounts and loop
  devices were released; only the zstd full-disk image is retained.
- `scripts/acquire.py`, `scripts/verify.py`: acquisition and verification methods.

Binary images, photos, and raw evidence are local-only and ignored by git. They
can contain device configuration or secrets. Summarized findings belong here.

## log

### 2026-11-10 (k) — FPGA layers 6 and 7: the 32 I2C channels re-rolled into one module, channel logic decoded

Offline only (switch, serial and outlet untouched). Reports
[fpga/i2cfpga/LAYER6.md](fpga/i2cfpga/LAYER6.md) (method, numbers, limits)
and [fpga/i2cfpga/CHANNEL.md](fpga/i2cfpga/CHANNEL.md) (plain-language
explanation); views `fpga/i2cfpga/mtd5/i2c_channel_{A,B}.v`.
- **Layer 6 (`modules.py`), lossless.** The 32 channel copies are factored
  into one module `i2c_channel` with two templates (family A 921 elements,
  family B 1,279; 776 correspond) + 32 instances (placement, port bindings,
  packing choices, routing, explicit deviations) + the non-channel
  remainder. LUT INITs, FF/control/carry lines and BRAM/IOB site features
  are regenerated from the template (48,647 of 133,986 cell lines; 19,556
  per-instance packing lines; 7,577 raw deviation lines; the rest is
  non-channel). Per instance 69-96% is pure template (median A 76%, B 86%).
- **Byte identity, fresh run in this session**: `fpga/i2cfpga/roundtrip6.sh`
  (layers 1-5 from the image, then template + instances + remainder ->
  netlist -> FASM -> frames -> image) gives `BYTE-IDENTICAL (layer 6)` for
  the 32 MiB flash dump (golden and update image both through layer 6;
  netlist, FASM and frames equal at every stage); `cmp` re-run by hand, SHA-256
  `fa559a02...` on both. The vendor 8 MiB file was not run through layer 6.
- **Layer 7 (`channelrtl.py`), a readable view.** Decoded from the logic:
  - register offsets (BDD of the write enables with the address bits as
    variables): `cb+0x00` = three 8-bit profile codes, `0x08` = 2-bit
    select, `0x0C` bit 0 master reset and bit 31 soft-reset pulse, `0x50`/
    `0x54` and a second pair `0x58`/`0x5C` with "go" flags on bit 31 of
    `0x54`/`0x5C` (V); simulation of the template writes 0x00660918 as
    profile 1/2/3 = 0x18/0x09/0x66 (power-up 0x66/0x17/0x08).
  - **driver channel of every instance** (from its decoded register page):
    all 32 distinct, 0..31; pad-pair ch21 = driver channel 17, ch25 = 0, ...
    (`mtd5/update.channel-map.txt`). This answers the open "ch -> port"
    question offline.
  - SCL divider: 16-bit down-counter reloaded with the selected code, a tick
    every code+1 clocks, held while SCL is stretched (V); bit-level FSM: a
    6-bit phase register, 6 ticks per bit, 11 for START (V); its four
    commands identified in simulation as START / STOP / bit-1 / bit-0 (I).
    Simulated SCL at 62.5 MHz: code 0x18 -> 386 kHz, 0x09 -> 868 kHz,
    0x66 -> 98.9 kHz (I; 6x(code+1)+12..14 clocks per bit, not 4x(code+1)).
  - The **two families** are the same engine with a different cut (V for
    the parts compared): family B decodes its own register page, family A
    receives finished register strobes from the core and has part of its
    divider/profile logic outside its block. Families: A = driver channels
    0-9, 21, 27-31; B = 10-20, 22-26.
- Not done / failed: a full transaction simulation through the shared PCIe
  core (the strobe timing of the core was not reproduced; the engine was
  driven at the bit-command level instead); the byte sequencer and
  transaction controller are not tabulated; family A's register map is
  partial; sideband / register-file / PCIe-BRAM re-rolling not started.

### 2026-11-10 (j) — FPGA layers 4 and 5: lossless netlist, 32 I2C channel engines

Offline only (switch and outlet untouched). Work in [fpga/](fpga/README.md);
reports [fpga/i2cfpga/LAYER4.md](fpga/i2cfpga/LAYER4.md) and
[fpga/i2cfpga/BLOCKS.md](fpga/i2cfpga/BLOCKS.md).
- **Layer 4 (netlist) done, lossless.** `fasmnet.py` turns the FASM into
  cells (sites with their FASM lines), nets (driver pin -> sinks, with the
  pips) and an `unassigned` bucket, using a routing graph built from
  prjxray-db (`xc7graph.py`, ppips handled: `always` followed, `default` =
  IMUX VCC tie, `hint` left to the site model). Flash update image: 458,693
  FASM lines, each owned exactly once (cells 133,986, nets 324,707,
  unassigned 0); 8,652 cells, 49,792 nets.
- **Byte identity re-verified in this session**: `fpga/i2cfpga/roundtrip4.sh` (image -> layers 1-3 -> netlist
  -> split into one file per block -> merge -> FASM -> frames -> image) gives
  `cmp`-identical images for the 32 MiB flash dump (golden + update) and the
  8 MiB vendor file (image0 + image1); netlist -> FASM is identical for all
  four images, 0 unassigned lines.
- Logical view `xc7bels.py`: 47,491 UNISIM-like primitives (25,237 LUTs,
  21,565 FFs, 270 CARRY4, 48 RAMB18, ...), no unconsumed SLICE feature;
  structural Verilog in `fpga/work/l4/update.v`.
- Bug found and fixed on the way: prjxray's `IOB_Y0` is the *upper* site of
  an IO tile. With the naive mapping the pad pairs were swapped, which looked
  like dead I2C pads; with the fasm2bels rule the IO ring is uniform.
- **Layer 5 (blocks), first pass.** Every cell/net/primitive gets a block
  path. Strongest results:
  - PCIe: PCIE_2_1 gen2 x1 with a 64-bit TRN interface; vendor/device
    `1d0f:8571`, rev 01, subsystem 0000:0000 are fabric ties (V); 2 x 8
    RAMB18 on the MIM ports = TX retry / RX buffers (V); MMCM + BUFGCTRL
    pclk mux + DCLK BUFG + /16 user clock = Xilinx `pipe_clock` topology
    (V topology, I name); all user logic on one 62.5 MHz clock.
  - IO: 64 open-drain IOBUF (T from FDPE INIT 1), 64 LUT-driven OBUF, 72
    IBUF (V); the 64 open-drain pads pair into exactly 32 mutual-best pairs
    (V).
  - Channels: lockstep structural matching from the 32 pad pairs matched
    28,720 logic nodes; every channel has one RAMB18 (SDP x36, 8 address
    bits from the channel; fits the 512 B + 512 B windows, I). Surprise:
    the channels come in **two structural families of 16** (about 390 vs
    540 FFs per channel); cause unknown.
  - Not achieved: tying register addresses (0x140..0x178, 0x1000+ch*0x100)
    to decode logic. Write-enable cones are 49-61 FFs deep with tens of
    cubes; no address constant could be read off. Channel numbers are
    therefore pad-pair order, not driver ports.
- fasm2bels cross-check: chipsalliance f4pga-xc-fasm2bels runs after two
  import stubs and one runtime patch; with assertions on it stops on the GTP
  refclk BUFG input having two sources (the same node our netlist flags),
  with `-O` it finishes, and its primitive counts match ours up to
  explained modelling differences (`LAYER4.md`).

### 2026-11-10 (i) — SError panic analysis, PCIe all-ones mode, enp0s3 up, FPGA round trip

- **Panic at 01:22** (boot uptime 3175 s), captured on serial: `SError Interrupt on
  CPU6 ... Comm: ifcs-learn` (SDK MAC-learning thread, userspace), ESR
  `0xbf000000`, `Kernel panic - not syncing: Asynchronous SError`. An ASIC
  BAR0 access went unanswered. Console showed no IPD-ERR and no AER/PCIe errors
  before it; the on-disk journal stops 77 s earlier (now synced every 15 s).
  CPLD fault log: nothing at crash time; the `XP12R0V_CPU_OK / VoltageIssueA`
  entry seen afterwards appears after every outlet power-off (12 V dropping),
  here at my 01:24 power cycle. Root cause of the ASIC going deaf **not
  established**. Only correlate: it was the one boot in which I had pulsed
  `asic_reset` three times; the pulse is now opt-in (`off --reset-asic`).
- **Mitigation**: DWC "AMBA error response" register (root-port config space
  `0x8D0`, Annapurna HAL `enable_axi_slave_err_resp`) was `1` on all three
  ports. Set to `0` (HAL-documented: return 0xFFFFFFFF without error) by
  `cs8210-pcie-errors.service` at boot. Verified: a BAR read with memory decode
  off, a guaranteed SError before, now returns `0xffffffff`.
- Steady-state power with the stack running ~700 W, flat; ASIC temperature
  creeping 59 -> 61 C at 50% fans. A soak logger (`/root/powerlog.sh` via
  `systemd-run --unit=cs8210-powerlog`, journal) samples every 30 s.
- **enp0s3 (second al_eth, 00:03.0)**: no PHY, vendor DT `port3 mode =
  "10g-serial"`. al_eth now runs a port without link management when there is
  no SerDes object (U-Boot's SerDes setup is used as is), carrier from MAC link
  status. Its far end is **ASIC devport 129** (`type AUX`, 10G, 1 lane, ISG
  32), which the vendor config defines with `admin_state FALSE`; `port enable
  129` (now in `cs8210-switch-up`) brings both ends to LINK_UP. Never leaves the
  chassis. enp0s3's MAC is random per boot (as in stock).
- **ethtool**: the driver's ethtool table was compiled out (legacy
  get_settings API). New minimal ops (drvinfo, link, link_ksettings): enp0s1
  reports 1000/Full via phylib (88E1510 is gigabit-only, matches the UniFi);
  enp0s3 reports from MAC mode.
- **FPGA**: full round trip flash -> packets -> frames -> FASM + 3 unknown bits
  -> back is BYTE-IDENTICAL for the flash image and the vendor file
  (`fpga/i2cfpga/roundtrip.sh`, report `fpga/i2cfpga/LAYER2.md`). Fabric proven
  to be prjxray xc7a100t (all 9448 frames match exactly once). Update image:
  ~25.1k LUTs, ~21.6k FFs, 48 RAMB18 (empty), PCIe gen2 x1 hard block, one
  MMCM, 218 IOs (64 bidirectional, likely 32x SDA/SCL). Package FGG484 or
  FGG676 (chip marking not read).

### 2026-11-10 — WorldEdit installation

- Installed WorldEdit 7.4.6 Bukkit release for Paper 26.3, from EngineHub's
  Modrinth project. Download SHA-512 matched upstream metadata; SHA-256
  pinned as flake packages.worldedit. Provenance in papermc/worldedit.json.
- Built on switch, rooted at /root/papermc/worldedit-package; plugin symlink
  /root/papermc/plugins/worldedit.jar. Stopped server gracefully to install,
  then restarted. No world edits performed.
- Switch rebooted externally during verification. After it returned, the DHCP
  wait and automatic startup succeeded. RCON worldedit version confirmed
  7.4.6 with WORLD_EDITING capability; plugins listed WorldEdit enabled.
  cuttlefishCuller rejoined successfully.

### 2026-11-10 — Minecraft console and open admission

- User requested an admin console, no whitelist, and op for cuttlefishCuller.
- Added just console / just command via mcrcon, exposed in the flake and
  devshell and installed in root's profile. Generated a random RCON password
  only on the switch; server.properties mode 0600. RCON TCP 25575 remains
  blocked from the LAN; use SSH then just console.
- Whitelist and enforcement disabled; online-mode remains true. No router
  forwarding added. RCON confirmed op and whitelist status.
- Persisted level-4 operator cuttlefishCuller, UUID
  7a61f29a-5d83-401f-bc3d-b616152c836b, in live ops.json.

### 2026-11-10 — PaperMC human controls and DHCP startup fix

- Added papermc/justfile with start/stop/restart, status, logs/recent, memory,
  join, build/deploy, and boot-status. Exposed just as a flake package and
  added a devshell with just/Python. Installed just in root's Nix profile.
- Deployed flake, justfile, and docs to /root/papermc; menu and read-only
  recipes validated on the switch, start recipe exercised.
- Switch rebooted during this work (not initiated here). The persistent boot
  dependency worked, but Paper exited after failing to bind before DHCP
  assigned 10.60.2.190. Added a bounded address wait before server startup;
  started the service again. No reboot test of the new wait performed.

### 2026-11-10 — PaperMC side quest

- User requested a standalone Nix flake deployed to `/root/papermc` on
  `10.60.2.190` with a 4 GiB memory budget.
- Deployed pinned Paper **26.3-223** / Java 25 on aarch64; sources and
  operations: [papermc/README.md](papermc/README.md).
- Systemd service caps total memory at 4 GiB (3.5 GiB high watermark,
  swap disabled); heap 2560 MiB, four JVM-visible processors, low CPU weight.
- Survival, six slots, view/simulation 6/4, online authentication.
  Management address TCP 25565 only; labeled INPUT rule managed by service.
- Persistent unit and multi-user target dependency installed under
  `/etc/systemd/system.attached`; confirmed target Wants includes papermc.
  No NixOS rebuild, hardware changes, or reboot.
- First startup reached Done in 33.467 seconds; external Minecraft status
  probe returned Paper 26.3 (protocol 777), 0/6 players, correct MOTD.
  Memory ~1.58 GiB after world creation, no automatic restarts.
- Default user's SSH key was unavailable; used the documented local
  `sudo ssh` root key. Initial devshell path-source attempts stalled;
  normal `nix develop` worked once sandbox access was restored.
- Full in-game login/load testing remains unperformed.

### 2026-11-10 (h) — FPGA reverse engineering started (byte-identical first)

User goal: byte-identical rebuild of the FPGA images from a readable form,
then make it progressively more meaningful. Work lives in [fpga/](fpga/README.md).
- I2C FPGA flash (`mtd5`) is a multiboot image: header (watchdog + IPROG to
  `0x400000`), compressed golden image, compressed update image at
  `0x400000`, erased from `0x801000`. It differs from the SSD's shipped
  `i2c_fpga_v01.02.0E_20200604.bin` (older); flash is the target.
- **Layer 1 done**: `fpga/i2cfpga/bitasm.py` packet listing <-> image,
  BYTE-IDENTICAL for both the 32 MiB flash image and the 8 MiB shipped file.
- prjxray-db has no XC7A75T; expected to be the XC7A100T die (like 35T ->
  50T). Layer 2 (frames, re-packing encoder, FASM + unknown bits) handed to
  an agent with the instruction to prove the fabric match from frame
  addresses rather than assume it.

### 2026-11-10 (g) — power budget, data-plane power CLI, QSFP tool

- **Power** (PSU1 PMBus, only PSU fitted; 117 V in): **~696 W at the wall**
  with the switch stack running idle, no optics. Regulator telemetry
  (TPS53688 READ_POUT, PAGE select + read): ASIC `vddcore` 127 W (159 A),
  `avdd` 88 W, `avddh` 25 W (ASIC ~240 W); Credo/MACsec rails ~165 W; PSU
  loss ~42 W; remaining ~220 W fans, CPU card, FPGAs, misc.
  Stopping the SDK alone changes nothing.
- System CPLD `0x03` bit 5 = vendor `asic_reset` (active low, from
  `sys_cpld.ko` attribute index 24 and the `cpld_set_data` jump table). **It
  is a pulse**: the bit returns to 1 within ~5 s, so the ASIC cannot be held
  in reset. After the pulse its core rail drops only 128 -> 118 W; it is
  leakage-dominated. No ASIC rail-enable exists in the CPLD map.
- Credo chips: MDIO FPGA `0x80000 + 0x20*(N-1)` bit 0 (vendor `phyN_reset`,
  32-bit RMW). In reset: Credo rails 165 -> 123 W, **wall 696 -> 631 W**.
- `cs8210-switch-power off|on [--start]|status` (nix vendor-sdk): stop SDK,
  Credo reset, unbind + PCI-remove the ASIC, reset pulse; `on` releases,
  rescans PCIe and rebinds ipd (no reboot needed, verified twice, SDK and
  Credo come back, 32 netdevs). Saves ~65 W.
  Bigger savings would need PMBus OPERATION-off on the ASIC/MACsec
  regulators: not attempted (rail sequencing / back-powering risk, and the
  CPLD's voltage-fault action `0x24` is undocumented).
- **QSFP cages** ([protocol review](reviews/2026-11-10-i2c-fpga-protocol.md),
  agent, offline): register-style I2C engine, per channel control block
  `0x1000 + ch*0x100`, write window `0x4000 + ch*0x200`, read window
  `0x8000 + ch*0x200`, command `cb+0x50`, start/busy `cb+0x54`, status
  `cb+0x60`, all 32-bit, completion polled. Tool `cs8210-qsfp` in the nix
  tools. First read-only run on hardware OK: no modules, all cages
  `reset=1`, channel profile `0x00081766` (vendor probe would write
  `0x00660918`, `cb+0x0c=0x80000000`, `0x178=0`). Apply with `--init` /
  `--reset P 0` when the first module is in.
- FPGA images: the I2C FPGA bitstream IDCODE is `0x3632093` (believed
  Xilinx Artix-7 XC7A75T). The other programmables are Lattice `.vme`
  images (not identified yet).

### 2026-11-10 (f) — module support survey, QSFP cage sideband state

- What the cages take (vendor `dynamic_config.sh` / Credo lane tables):
  1x400, 1x200, 2x200, 2x100, 4x100, 8x50 (QSFP-DD); **1x100 (QSFP28)**,
  **1x40 (QSFP+)**, and 4x25 / 4x10 breakouts. No single-lane 1x10 / 1x25
  entry, so SFP+ through a QSA adapter would have to run as sub-port 1 of a
  4x10 port (inferred, untested). Nothing below 10G per lane. No module
  vendor/part-number checks found in the SDK.
- User ordered QSA adapters and DACs (2026-11-10).
- I2C FPGA sideband, read-only on mainline (32-bit BAR0 reads, as the vendor
  driver does): `present 0`, `present_changed 0`, `interrupt 0`,
  `interrupt_mask 0xffffffff`, `lpmode 0`, **`reset 0xffffffff` (every cage
  held in reset)**. The vendor IRQ handler writes 0 to `0x178` on a presence
  change; nothing on mainline does that yet, so plugged modules would stay in
  reset. Polarity of present/reset inferred from the vendor tools, to be
  confirmed with the first module.
- Agent working offline on the I2C master transaction protocol (module
  EEPROM reads, ideally a mainline i2c_adapter).

### 2026-11-10 (e) — CPU RX errors explained and fixed; ASIC temperature; fans back to ~50%

- **`Unsupported VF 3` / `Invalid LDH` was not a driver bug.** A temporary
  debug build hex-dumped the rejected frames: all on CPU RX ring 3, all plain
  Ethernet with no switch header, all IPv6 (MLD reports, NS for DAD, RS) from
  `5e:94:49:e4:a2:48`, the MAC of the SDK-internal netdev `inno_0_1000`. The
  host's own frames on that interface come straight back. The exponential
  spacing was router-solicitation backoff. Stock ONIE never sends on it.
  Fix: udev sets `disable_ipv6=1` on `inno_*`, lldpd limited to
  `enp0s1,fpp*`. Result: 0 errors over 2.5 min (was ~20 in the same window);
  lldpd now covers enp0s1 + fpp1..32.
- Fixed my own bugs in the service: `cs8210-switch-down` never ran (a
  makeWrapper `--argv0` does not change `$0` of a bash script; now `--down`),
  and each chroot entry had a private `/tmp`, so `phy_shell exit` could not
  reach `phy_server`'s fifo. `/tmp` and `/var/run` of the chroot are now
  shared under `/run/cs8210-sdk`. Start, stop (9.5 s) and restart are clean.
- **ASIC die temperature on mainline**: MCP3425 ADC at `0x68` behind
  `0x70:2 -> 0x74:0 -> 0x72:7` (Linux bus 21 here), reading the ASIC thermal
  diode. Conversion recovered from the vendor `check_switch_temp`
  (disassembly, float constants): `T = -124.28 V^2 - 422.03 V + 384.62`,
  V = raw / 1000 (power-on config `0x10`: continuous, 12 bit, gain 1).
  Reads 52-59 C idle with the SDK running (59 C steady at ~50% fans).
- Board curve now starts at 40 C (MACsec card LM75s idle at ~38 C; vendor
  board warning 53 C), so the fans sit at the 50% floor at idle. In `cs8210-show temperature` as
  `switch-core (ASIC)`.
- **Fans**: system CPLD `0x05` bits 7/5 (active-low fan_max_speed) had been
  cleared, forcing 100%. Setting them back (what the vendor's install.sh does)
  sticks. `cs8210-fancontrol` now reads the ASIC temperature every cycle,
  applies a second curve (70 C -> min, 95 C -> full; vendor PID setpoint is
  97 C, limit 105 C), releases the CPLD force only while that sensor reads
  plausibly, re-writes PWM if the CPLD overwrote it, and on any error or stop
  sets PWM 255, re-clears the CPLD bits and re-arms the fan watchdog.
  Verified cold boot with the ASIC running: 54-58% (~11.5k rpm, was ~19k);
  stopping the daemon -> CPLD `0x05=0x40`, 100%; restarting -> back to 54%.

### 2026-11-10 (d) — switch ASIC online on mainline: fpp1..fpp32 netdevs

- **Root cause of `Node 0 couldn't be created`** (debug agent, on hardware,
  [2026-11-10-ipd-legacy-abi-debug.md](reviews/2026-11-10-ipd-legacy-abi-debug.md)):
  `libifcs.so` (`_ii_pcie_attach`) sscanf's the version string the driver
  returns from QUERY_NODE and compares major/minor/patch with its own
  (0.12.41); the legacy patch returned upstream's `0.16.2.0.0.0`. The mismatch
  is only logged above the SDK's default log level. Fix: module parameter
  `legacy_version` (default `0.12.41.1.0.1`) reported in legacy mode.
- **Result, from a cold boot** (`systemctl start cs8210-switch`, 49 s):
  `Node 0 created and enabled.`, `Knet config done`, **`fpp1`..`fpp32`**
  (MTU 9462, MACs `0e:48:c6:7a:55:cf`..`ee`, UP / NO-CARRIER with no optics)
  plus `inno_0_0129` and `inno_0_1000`; 8 MSI-X vectors (`inno_irq-0-napi` ..
  `7-rx-mcu0`, only `pic` counting); Credo: 16/16 slices firmware `01.92.02`,
  `credo config done!` in MACsec bypass. No oops or SError.
- New in the nix tree: `cs8210-switch-up [mode]` / `cs8210-switch-down` and
  `cs8210-switch.service` (not enabled at boot on purpose); DHCP restricted to
  `enp0s1` so the host does not auto-configure the front-panel netdevs.
- **Open problems**
  - `IPD-ERR: Unpack-Unsupported VF 3` (or 7) / `Invalid LDH` on the CPU RX
    ring, about one per frame the host sends on `inno_0_1000` (16 in the first
    minute, RX counter stays 0). The 0.16.2 v0 header parser rejects something
    the old module accepted, or the legacy SET_NETDEV translation is off.
    **CPU-path receive is therefore not proven.** First thing to fix before
    any traffic test.
  - **Fans are at 100% once the ASIC has been initialised**: system CPLD reg
    `0x05` went `0xe0 -> 0x40` (bits 7/5 clear = forced max) and stays so
    across a warm reboot; the fan daemon's PWM writes read back as 255. Left
    alone deliberately: there is **no ASIC die temperature on mainline yet**.
    The vendor reads it from an MCP3425 ADC (`0x68` behind `0x72` ch 7) through
    a `check_switch_temp` helper, and Q16 from the ASC10 monitors; our kernel
    has IIO disabled and no ASC10 driver. Board LM75s read 30-35 C.
  - The netdevs are the SDK's CPU-path interfaces ("knet": each port in a
    private VLAN with the CPU port). Nothing is offloaded from Linux; no
    switchdev. No link has been tested (no optics fitted).
  - Aux SPI flash, second NIC, kernel-side MDIO/I2C FPGA drivers: unchanged.

### 2026-11-10 (c) — old SDK against the new ipd: legacy ABI patch, first live runs

- Compat verdict (agent,
  [2026-11-10-ipd-source-compat.md](reviews/2026-11-10-ipd-source-compat.md)):
  ipd 0.16.2 cannot serve the 2020 userspace as-is. Same ioctl numbers and
  header version, same mmap convention and Teralynx-7 header path, but 11
  ioctl argument structs grew, device init moved from `open()` to new ioctls
  the old library never calls, the node is `/dev/ipd0`, and INFO_NODES no
  longer sets `num_nodes`. The agent's patch adds `legacy_abi=1` (old struct
  layouts with size asserts, init at open, node named `ipd`). In the nix tree
  as `kernel/patches/ipd-legacy-abi.patch`, with `options ipd legacy_abi=1`.
- Marvell's newer SDK userspace (`sonic-marvell-teralynx-binaries`, SAI up to
  7.0.1) is **amd64 only**, so it cannot replace the arm64 SDK on this box.
- Live result so far, patched module loaded with `legacy_abi=1`, vendor
  userland in the chroot (needed `netaddr` copied into python2.7
  site-packages; ONIE installs it from a tarball at boot):
  the driver opens, initialises the chip (swap status 0, SYNC_MODE 1, **MSI-X
  8 vectors**, DMA init done), answers INFO_NODES and one QUERY_NODE, then the
  SDK prints `ERR: Node 0 couldn't be created, rc: IFCS_DRIVER_ERROR` and
  closes the device. No netdevs yet. No kernel errors, no crash.
  A debugging agent with ssh access (strict no-MMIO / no-reboot rules) is
  working on what the library rejects after QUERY_NODE.
- Plumbing mistakes of mine in this stretch, for the record: a bad scripted
  edit truncated `libMdio.c` (rewritten); `pkill -f` matched and killed my own
  ssh command twice; `modprobe` after `switch-to-configuration switch` loads
  modules from the *booted* generation, so a new module has to be insmod'ed
  by path or the box rebooted.

### 2026-11-10 (b) — ipd driver source found and running; Credo PHYs up on mainline

- **What the FPGAs do** (agent review
  [2026-11-10-fpga-roles-and-datapath.md](reviews/2026-11-10-fpga-roles-and-datapath.md),
  offline analysis of the stock files): neither carries traffic.
  `1d0f:8571` (Xilinx, own card) is the QSFP-DD management plane: 32 I2C
  masters (one per cage) plus present / reset / lpmode / interrupt bits.
  `1d0f:ec30` (Lattice, on the MACsec card) is the Credo PHY management plane:
  16 MDIO masters plus per-chip reset/gpio/interrupt bits.
  **Only ports 1-16 go through Credo CMS50216 MACsec PHYs; ports 17-32 connect
  the ASIC straight to the cages.** Port map, lane swaps and the stock bring-up
  sequence are in the review.
- **The ipd kernel driver is open source** (user found the lead): Marvell
  publishes it under GPLv2 at `github.com/Marvell-switching/mrvl-teralynx`
  (`drivers/ipd`, version 0.16.2, commit `53d8dcf`, 2025-10-15), the driver
  SONiC builds. It lists `1d98:1b58` rev `0x10`. Built for 7.3.3/arm64 as a nix
  derivation with one shim (`strncpy` no longer exists in the kernel). On the
  switch it autoloads by PCI alias and logs `probe found PCI device 1d98:1b58
  revision - 10 ... initialized. Version - 0.16.2.0.0.0`; node is `/dev/ipd0`
  (dynamic major), with a udev symlink `/dev/ipd` for the old SDK.
  **Not yet known: whether the 2020 SDK userspace (built against ipd
  0.12.41) is ABI-compatible with this 0.16.2 driver.** The ioctl numbers do
  not encode struct sizes, so a mismatch would be silent; an agent is diffing
  the old module's DWARF layouts against the new source before the old
  `innovium.user` is pointed at it.
- **Credo MACsec PHYs brought up on mainline.** The vendor tools (`phy_server`,
  libcredo) run unmodified in a chroot of the ONIE root filesystem
  (`/var/lib/cs8210-sdk/root` on the switch, glibc 2.25). The only replaced
  piece is `c-phy/libMdio.so`: ours (`nixos/module/cs8210/vendor-sdk/libmdio`)
  drives the MDIO FPGA from userspace via the PCI resource file instead of
  the vendor's 4.9-only `/dev/mdio-N:XX` + sysfs stack; the vendor command
  files' `echo HZ > /sys/bus/mdio_bus/devices/mdio-N:XX/mdc_freq` lines land
  in plain files the launcher provides. Run:
  `c-phy/auto_load_user.sh -m 1-32:optics_1x400G:fec-def -e macsecbypass -l`.
  Result (`evidence/mainline-sdk-2026-11-10/credo-bringup.log`): 16/16 slices
  `[Firmware load] Version: 01.92.02` (the version the factory log shows), no
  errors or timeouts, `credo config done!`. Mode is **MACsec bypass**
  (retimer, no encryption) on purpose: the vendor default installs
  GCM-AES-128 SAs with a fixed test key printed in the script.
  PHY firmware lives in RAM, so this has to be re-run after every boot.
- New in the nix tree: `hardware.cs8210.vendorSdk` (ipd module, libMdio,
  `cs8210-sdk-chroot`). The vendor userland itself is deliberately not in the
  repo.

### 2026-11-10 — stock SDK reference run on ONIE (ASIC + MACsec PHYs online under the vendor stack)

User direction: find out what the FPGAs do, use the diag OS / ONIE / stock files
to chart a course, then get the switch chip and MACsec online on mainline with
the front-panel ports as (eventually offloadable) Linux interfaces.

Booted stock ONIE from the U-Boot prompt (`run onie_bootcmd`, RAM only; NixOS on
the SSD untouched) and ran the vendor bring-up by hand:
`./integrator_mode -m 32x400 -a 0E:48:C6:7A:55:CE -k -d` in
`/root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK`. Evidence in
`evidence/stock-sdk-run-2026-11-10/` (integrator.log 2773 lines, dmesg, state);
the installed SDK 0.1.7, drivers and diag configs were copied off the live
ONIE into `analysis/onie-live-2026-11-10/` (local-only).

What a working stack looks like on this unit:

- `ipd.ko` ("Innovium Switch PCIe Driver ... Version 0.12.41.1.0.1", 454 kB
  loaded) probes `1d98:1b58 rev 10`, creates `/dev/ipd` (char major 101).
  Userspace is `innovium.user` (one process, ~750 MB VSZ) started by
  `auto_load_user.sh`, config assembled from `cel_configs/*.yaml`, SKU file
  `configs/sku/innovium.77701_A` (Teralynx 7, IVM77700 family).
- **32 netdevs `fpp1`..`fpp32`** (renamed from `inno_0_0001`..), MTU 9462, MACs
  base+1.., plus `inno_0_0129` and `inno_0_1000`. No optics fitted, so carrier 0.
- ASIC interrupts: **MSI-X**, 7 vectors used of 32 (`inno_irq-0-napi`, `1-async`,
  `2-tx`, `3-rx`, `4-err`, `5-lrn`, `6-pic`); only `pic` fires when idle.
  PCI command: Mem+ BusMaster+ DisINTx+.
- `innovium.user` mmaps `/dev/ipd` with the **physical address as the mmap
  offset**: BAR0 (`0xc0200000`, 128 K), a few CMA regions (`0xaf...`) and
  several hundred single 4 K pages above 4 GiB (DMA buffers).
- Credo side: `phy_server` (from `c-phy/`) talks to char devices
  `/dev/mdio-N:addr` (major 240, one per MDIO master, created by the
  `mdio-fpga`/`mdio-lattice` modules), loads PHY firmware and configures the
  MACsec engines: log shows `DEV_EIP164_n` / `DEV_EIP163_n` (Rambus/Inside
  Secure MACsec IP: 1024 SAs, 512 SCs, 12 channels per device), ending in
  `credo config done!`.
- Mode string actually applied for `32x400`:
  `1-16:copper_1x400G;17-32:optics_1x400G` (the script forces host link
  training on ports 1-16), `boardID:0x01`.
- Temperatures after bring-up: hottest sensor ~50 C (ASIC powered and
  configured, no traffic), others 25-41 C.

Two offline agents are working from the stock files in parallel: FPGA roles /
port map / bring-up sequence, and a port specification for a mainline
replacement of `ipd.ko`.

### 2026-11-09 (latest+1) — FPGAs and all 16 MACsec PHY ports identified on mainline

Register maps came from a second offline agent reversing the vendor modules
([review](reviews/2026-11-09-fpga-asic-probe-maps.md)); every access below is
one the vendor drivers perform. Now `cs8210-pci-probe` / `cs8210-show macsec`
in the nix tools.

- **Both FPGAs leave U-Boot with PCI memory decode off** (`COMMAND=0000`). A BAR
  read in that state would have been a third SError panic. `echo 1 > enable`
  first; the stock kernel logs the same `enabling device (0000 -> 0002)`.
- mdio-fpga `1d0f:ec30` BAR0+0x0 = **`0x00001008`** (same as the stock dmesg).
  Its eight "phy misc" blocks read: reset released, gpio/irq/mask 0.
- i2c_accel_fpga `1d0f:8571` BAR0+0x0 = `0x010b0009`: version 0x0b00 (the stock
  driver calls itself v0.11), board version 9.
- **Credo CMS50216 on all 16 MDIO masters** (`mdio-1:02` ... `mdio-16:11`):
  clause-45 1.00cc = `0x2161`, 1.00cb = `0x00a2` (vendor `phy_id 0x216100a0`
  with a mask). Read through the FPGA's MDIO master registers with the "new"
  completion handshake; sys CPLD reg `0x01` = `0x10` selects it. This needs
  16-bit writes to FPGA registers (bus reset, address, start, done-ack); no
  PHY register is written. Sys CPLD `0xa4 = 0xb0` (MACsec present),
  `0x06 = 0xff`, `0xa5 = 0xff` (the diag YAML expected `0x3f` under a mask).
- Innovium `1d98:1b58` rev `0x10`: identification is PCI config only. The
  vendor `ipd.ko` probe never reads BAR0, and its first BAR0 traffic at open
  time is a series of writes, so nothing further was read from the ASIC.
- al_eth: `enp0s1 selects TX queue N, but real number of TX queues is 4` was
  logged a few hundred times per boot (queue id = CPU number, 16 CPUs, 4
  queues). Fixed with a modulo in `al_eth_select_queue`; zero after reboot.

### 2026-11-09 (latest) — TPM solved, lldpd fixed, ssh keys baked in

- **TPM works on mainline, from boot, declaratively.** Root cause found by a
  fresh-context agent disassembling the vendor kernel
  ([review](reviews/2026-11-09-tpm-aux-spi.md)): the TPM and the FPGA flash both
  sit on the aux controller's SS0 behind a board mux steered by SoC GPIOs
  **PBS52 / PBS54** (PL061 `gpio6` at `0xfd898000`, bits 4 and 6). The vendor
  `dw_spi_set_cs()` strcmp()s the SPI modalias, sets the two GPIOs and forces
  chip select 0; U-Boot leaves the mux on the flash at OS handoff. Confirmed live:
  `gpio6` data read `0xbb` (52=1, 54=0); after writing 52=0, 54=1 a re-bind gave
  `2.0 TPM (vendor-id 0x15D1, device-id 0x1B, rev-id 22)`.
  Fix is DT-only: `gpio6` PL061 node, `gpio-mux` on bits 4/6, `spi-mux@0` with
  `tpm@2`; modules `mux-gpio` + `spi-mux` loaded at boot. `tpm2_getcap` reports
  IFX SLB9..., `tpm2_pcrread` works.
  My earlier "no GPIO changes during transfers" observation was wrong in
  effect: I sampled within one device's traffic, where the mux does not move.
- **Aux flash (i2cfpga, n25q256a) still does not answer** with the mux at
  PBS52=1/PBS54=0: JEDEC reads all ones through spi-nor and by hand, in every
  mode/speed tried. PBS52=0 reaches the TPM regardless of PBS54. All eight GPIO
  banks read the same on mainline as under the stock kernel, so something else
  gates the flash. Node kept in the DTS with `status = "disabled"`.
- **lldpd**: the aarch64 build failure is lldpd's bundled, stale
  `include/linux/types.h` shadowing the system one; it lacks `__u128`, which
  current arm64 `asm/sigcontext.h` uses. Fixed with a 5-line patch in
  `nixos/machine/cs8210` (overlay). Running; sees the upstream switch on port 4.
- **ssh**: root's authorized keys are now the five keys from
  `https://github.com/nepeat.keys` (fetched 2026-11-09; the first is the
  existing erin-laptop key) plus `root@sea420-desktop`. The temporary bring-up
  key and my probe scripts were removed from the switch; the private half was
  deleted from the scratchpad. Note: `erin@sea420-desktop` has no ssh keypair,
  so from the desktop it is `sudo ssh root@10.60.2.190`.
- Second offline agent started on register maps for the MDIO FPGA / Credo
  PHYs, the I2C FPGA and the Innovium ASIC, so the first reads are ones the
  vendor drivers provably do (unacknowledged MMIO reads panic mainline here).

### 2026-11-09 (late) — TPM investigation, tools, state at end of session

Deployed over ssh (`nix copy` + `switch-to-configuration boot`), which also
exercised the bootmenu builder on the box: two generations listed, default boots.

Verified on the final system: `systemctl is-system-running` = running, no SATA
exceptions since `ahci.mobile_lpm_policy=1`, fan daemon active, 14 temperature
sensors with board locations (switch board, MACsec card, both fan boards,
i2cfpga card), CPLD versions read over I2C match U-Boot's banner (system 0x15,
switch LED 2, MACsec LED 1, fan 3/3), `fw_printenv` works against `/dev/mtd1`.

Corrections to earlier notes:

- The EEPROM at `0x57` on the root I2C bus is **blank** on this unit. The ONIE
  TlvInfo that U-Boot/ONIE show lives on the **switch board EEPROM**, `0x56`
  behind `0x70:2 -> 0x74:0 -> 0x71:7`. The i2cfpga card has an IPMI FRU EEPROM
  at `0x56` behind `0x71:0`. Neither is in the SPI flash `board_eeprom`
  partition, which starts with zeros.
- Each fan CPLD has 7 fan slots, not 6.

**TPM / aux SPI: still not working on mainline. What was established:**

- Stock kernel sees the TPM (SLB9670, `0x15D1/0x1B`) and the n25q256a on every
  boot path, including when booted with `booti` from the prompt (experiment D,
  stock kernel + stock DTB over TFTP). U-Boot's `al_tpm_tis20_end: write failed /
  TPM extend failed` lines appear on every `booti` and are a red herring.
- Not the cause: interrupting autoboot (A), `scsi init` (B), probe timing
  (re-binding `tpm_tis_spi` 55 s after boot still reads `0xFFFF`), pin mux
  (`pbs_mux_sel_4` at `0xfd8a8220` reads `0x30000333` under U-Boot, stock and
  mainline: pins 32/33/34/39 on function 3, as the public HAL's
  `al_muio_mux_iface_pins_spim_aux_v3` says).
- DTS now uses `amazon,alpine-dw-apb-ssi` for both controllers (what U-Boot's
  embedded DT uses; enables mainline's CS-override path). Boot flash still
  works, aux bus still reads ones.
- Under the stock kernel, sampled with `devmem` during real transfers: DW
  `SER` reads **1** for both TPM and flash traffic (the DT says flash is CS 1),
  `CS_OVERRIDE` (`+0xf4`) stays `0xF`, `CTRLR0=7`, no GPIO bank or SGPO word
  changes. (`gpio5` bit 4 toggles by itself even when idle.)
- A hand-driven transfer through `/dev/mem` (fill FIFO, set `SER`) returns the
  right JEDEC ID on the boot flash controller (`c2 25 39`) but all ones on the
  aux controller, **even under the stock kernel while its own driver works**.
  So the stock driver does something per transfer that is not visible in the
  registers sampled. The vendor kernel has `dw_spi_gpio_cs_control` /
  `cs-gpios` strings, but the vendor DT has no `cs-gpios`.
- Next idea: disassemble `dw_spi_set_cs` / transfer path in
  `analysis/kernel-review/vendor-Image`, or trace SPI pins with a logic
  analyser. `bootm` cannot be used to boot mainline for comparison: the 71 MB
  Image exceeds this U-Boot's `CONFIG_SYS_BOOTM_LEN`.

Mistakes worth not repeating:

- **Reading unimplemented PBS registers through `/dev/mem` panics mainline**
  (`Asynchronous SError Interrupt`). U-Boot and the stock kernel return
  `0xbad0add1` for the same reads. I did this twice (`0xfd8a8000` block, then
  the `0xfd8a8130` row, whose second word is unimplemented). Both needed a
  power cycle; no persistent damage seen.
- U-Boot `dhcp` overwrites `serverip` with the DHCP server's; set it afterwards.
- ONIE over non-interactive ssh has no `/usr/sbin` or `/sbin` in PATH
  (`fw_setenv`, `reboot`, `devmem` need full paths); `fw_setenv` needs `-f`.
- ONIE's dropbear accepts root without a key, so a script aimed at NixOS will
  happily run on ONIE if that is what is booted. Check `uname -r` first.

Left on the bench: switch powered on, running NixOS, fans at 50%. TFTP test
files in `/srv/tftpboot/cs8210/` on sea420-desktop (mainline and stock ONIE
kernel/initrd/DTB) can be deleted.

### 2026-11-09 (night) — mainline 7.3.3 boots, NixOS installed on the SSD

**Result: NixOS 26.11 with mainline Linux 7.3.3 boots unattended from the SSD
to a root shell on the serial console, with management Ethernet up.**

Sequence, including the failures:

1. RAM boot via TFTP from the U-Boot prompt (nothing persistent). First `booti`
   attempt failed: `Wrong Ramdisk Image Format`. This U-Boot does not accept a raw
   initrd as `addr:size`; it needs a mkimage legacy ramdisk. That also rules out
   `sysboot`/extlinux with an initrd, so the DMC12 bootmenu builder was ported
   after all (`module/cs8210/uboot-menu`): raw `Image` + `booti`, wrapped initrd.
   Load addresses: Image `0x08000000` (booted in place, header flags allow it),
   FDT `0x10000000`, initrd `0x20000000`, script `0x07000000`, menu `0x07100000`.
2. Second attempt booted to NixOS stage 1: console, 16 CPUs, memory map with all
   three vendor no-map regions, timer 75 MHz, internal PCI, **all three external
   PCIe ports linked** (Innovium `1d98:1b58` Gen3 x4, FPGA `1d0f:8571` Gen2 x1,
   MDIO/MACsec FPGA `1d0f:ec30` Gen1 x1), AHCI + SSD, boot SPI flash partitions,
   I2C mux tree.
3. Built a raw disk image (`system.build.sdImage`: 16 MiB FAT stub + ext4 root,
   grown on first boot), power-cycled to ONIE and wrote it to `/dev/sda` over ssh
   (`pigz | ssh | gunzip | dd`), verified by SHA-256 of the exact image length.
   **The factory DIAGOS on the SSD is gone**; it survives only in
   `images/ssd-2026-10-17.img.zst`.
4. First disk boot reached multi-user, then the kernel oopsed in
   `al_eth_lm_mode_change` (NULL SerDes object) when networkd opened the second,
   PHY-less NIC (`00:03.0`, 10GBASE-SR towards the switch board). That hung
   logins. Fixed in the vendored driver: refuse to open a port that needs link
   management when there is no SerDes support (V3 AVG SerDes is not ported).
5. al_eth on `00:01.0` also needed: PCI rev 4 -> effective rev 2 (as the vendor
   HAL does), clause-22 MDIO bus ops (the driver only registered C45 ones), and
   SGMII as the phylib interface. Result: `Marvell 88E1510`, 1 Gbps/Full,
   MSI-X per queue, DHCP 10.60.2.190, ssh works.
6. `fw_setenv -f nos_bootcmd 'scsi init; ext4load scsi 0:2 0x07000000
   /boot/boot.scr; source 0x07000000'` from ONIE. **This is the only firmware
   change made.** `uboot-env` md5 before `e8790bbe...`, after `fffb5a04...`.
   Stock `bootcmd` still ends in `run onie_bootcmd`, so ONIE is the fallback.
   To undo: `fw_setenv -f nos_bootcmd echo`.
7. Under write load the SSD threw `ata16: SError { PHYRdyChg CommWake }` and
   NCQ timeouts about once a minute (three in the first three minutes, with I/O
   errors). Cause: SATA link power management (`min_power_with_partial` from the
   kernel's mobile LPM default). With `max_performance` set live: 6 GB of
   fsync'd writes, zero new errors. Made permanent with
   `ahci.mobile_lpm_policy=1`.
8. Fans: applied the vendor quiet recipe over i2c-dev (fan watchdog reg `0x0c`
   `0x19 -> 0x18`, PWM regs `0x22..0x82` `0xff -> 0x7f` on both fan CPLDs). Tach
   raw 189/208 -> 112/123. Register map confirmed on hardware: CPLD version at
   `0x00` = 3, **7 fan slots per board** (the diag YAML comments out the 7th),
   sys CPLD `0x05 = 0xe0`. Codified as `cs8210-fan`, `cs8210-show` and
   `services.cs8210-fancontrol` in the nix module.

Open problems:

- **TPM not visible yet.** `tpm_tis_spi spi1.0: TPM vid 0xFFFF` and the aux SPI
  flash reads JEDEC `ff ff ff`: the whole aux SPI bus reads back ones under
  mainline. U-Boot itself prints `al_tpm_tis20_end: write failed / TPM extend
  failed` on every `booti` boot of ours, but not on the stock ONIE `bootm` path,
  where both U-Boot and the 4.9 kernel see the TPM. So the bus is already dead
  before Linux starts on our path. Suspects: `scsi init`, or `booti` vs `bootm`.
- Second NIC (`00:03.0`) needs an Alpine V3 AVG SerDes port; left down.
- MACsec: the FPGA `1d0f:ec30` and the MACsec card's LM75s/CPLD are visible; the
  Credo CMS50216 PHYs behind the FPGA MDIO are not probed yet.
- Workflow notes: no aarch64 builder was reachable, so the kernel and al_eth are
  cross-built from x86 (`hardware.cs8210.kernelBuildSystem`); `lldpd` is disabled
  on this host because it does not compile for aarch64 on current nixpkgs
  master. A temporary bring-up ssh key was appended to
  `/root/.ssh/authorized_keys` on the switch over the serial console (not in
  the nix repo).

### 2026-11-09 (evening) — SPI flash backup, live DT capture, NixOS port started

User direction: back up ONIE "in case we fuck up", then port the DMC12 NixOS
module (https://codeberg.org/dma/dmc12-nixos, Alpine v2 K2C-AB) to this Alpine v3
K2X-N board inside `nepeat/nix`, boot it, get console, install to `/dev/sda`.
Boot goals: al_eth management port, PCIe (switch ASIC), TPM, MACsec gear visible.

- **Flash backup done.** All six MTD partitions read over ssh from ONIE into
  `images/spi-2026-11-09/` (local-only). SHA-256 of every file matches
  `sha256sum /dev/mtdNro` on the device (`device-sha256.txt`). ONIE = `mtd4-onie.bin`
  `63c70b99...daba3b`; U-Boot `a22ff482...1897f6`; env `f3dfc3e9...a2a480`.
- Live evidence in `evidence/live-onie-2026-11-09/`: runtime FDT (+ decompiled),
  `/proc/iomem`, `/proc/interrupts`, `lspci -vvnn`/`-xxx`, dmesg.
  - RAM banks from U-Boot: 0-2G, 2-3G, 4-8G, 8-9G; vendor no-map: ECC poison
    `0x1e0000`/`0x1e1100`, APCEA `0xb0000000`+256M.
  - Timer 75 MHz (DT property), sbclk 500 MHz, MSI SPIs 160+800.
  - External PCIe: port0 ctrl `0xfd800000` ECAM `0xfb600000` INTx SPI41;
    port1 `0xfd820000`/`0xfb700000` SPI42; port4 `0xfd920000`/`0xfba00000` SPI45.
  - `mdio-fpga` maps `0xe0200000` = port 4's window, so **`1d0f:ec30` is the MDIO
    FPGA in front of the MACsec PHYs** (stock driver `cms50216.ko`, Credo
    CMS50216); `i2c_accel_fpga` maps `0xc82..0xc84` = port 1, `1d0f:8571`.
  - `spi_aux` (TPM + i2cfpga flash) has no interrupt in the vendor tree.
- **U-Boot prompt** (stop with `123`): has `booti`, `bootm`, `sysboot`, `pxe`,
  `bootmenu`, `source`, `ext4load`, `tftpboot`, `dhcp`, `unzip`, `saveenv`.
  U-Boot itself sits at ~`0x03ac0000-0x04000000` (relocaddr `0x03ed1000`).
- Gotcha: tio block-buffers its log file, so the autoboot prompt is not in the
  file until later output arrives. Watch the tmux pane, not the file.
- **NixOS port** lives in `nepeat/nix/nixos/module/cs8210` + `nixos/machine/cs8210`
  (flake output `cs8210`): mainline 7.3.3 + `alpine-v3-cs8210.dts` (rebuilt from
  the live tree) + one dw-spi poll-mode patch + vendored al_eth with the
  rev 4 -> 2 mapping. Kernel is cross-built from x86. Boot path is plain
  extlinux via U-Boot `sysboot` (DMC12's bootmenu builder was not needed).

### 2026-11-09 — first power-up, stock boot capture, storage enumeration

Capture: `serial-20261109-182401.txt` (local-only, see note at end). Outlet on at
18:25:15 PST; autoboot was not interrupted.

- Console baud confirmed 115200 8N1. U-Boot env agrees (`baudrate=115200`,
  `consoledev=ttyS0`). The 9600 SysCPLD-logger caveat did not apply to this port.
- Stock boot: `bootcmd=run boot_diagos; run check_boot_reason; run nos_bootcmd;
  run onie_bootcmd`. `nos_bootcmd=echo` (no NOS installed), so it falls through to
  ONIE from SPI flash. The DIAGOS on the SSD only boots when
  `onie_boot_reason=diag` (`diagos_bootcmd`: ext2load scsi 0:3 `/root/uImage` +
  `/root/celestica_cs8210-r0.dtb`). Autoboot stop is `Type 123<ENTER>`, 3 s.
- ONIE came up in **installer mode**, took DHCP 10.60.2.190 on eth0, started
  dropbear + telnetd, and began TFTP/HTTP installer discovery. I ran
  `onie-discovery-stop` (RAM-only) so nothing could auto-install over the SSD.
  The unit was left powered on at the `ONIE:/ #` prompt.
- Things ONIE did by itself that may have written the `uboot-env` flash
  partition (not verified either way): printed "Making NOS install boot mode
  sticky"; dropbear host keys exist in the env (`onie_dropbear_*_host_key`) and
  may have been generated and saved this boot. ONIE did not mount any `sda`
  partition.
- PCI: internal 2x al_eth, crypto, 8x AHCI (32 ports), RAID/crypto; external
  PCIE_0 Gen3 x4 `1d98:1b58` (Innovium, matches diag YAML), PCIE_1 Gen2 x1
  `1d0f:8571` (Amazon "memory controller"), PCIE_4 Gen1 x1 `1d0f:ec30`
  (Amazon ethernet; BAR sizing firmware-bug warning).
- Temperatures at idle in ONIE: 25-38 C across 24 hwmon sensors.

**Onboard storage, as seen electrically (all read-only probes):**

| device | evidence | contents |
| --- | --- | --- |
| M.2 SATA SSD, `sda` | ata16 only; S/N A069BCC5 = the imaged drive | DIAGOS, 7 partitions |
| SPI NOR `mx25u25635f`, 32 MiB (spi0.0) | U-Boot `SF:` + kernel m25p80 | mtd0 uboot, mtd1 uboot-env, mtd2 unused, mtd3 board_eeprom, mtd4 onie |
| SPI NOR `n25q256a`, 32 MiB (spi1.1) | kernel m25p80 | mtd5 `i2cfpga` |
| TPM 2.0 SLB9670 (spi1.0) | tpm_tis_spi | - |
| I2C EEPROMs (24c64 / 24c02, many) | at24 probes | FRU / SPD / module data |

So yes: there is onboard storage besides the SSD, namely **two 32 MiB SPI NOR
flashes** holding the whole boot chain and ONIE. They have not been dumped yet.

Not found: any second block device. The other 31 SATA ports report link down;
U-Boot prints `NAND: 0 MiB`; the DT `nand-flash` node (`annapurna-labs,al-nand`)
is `disabled`; the DT has no SD/eMMC/USB host node. Limit of this result: the
ONIE kernel has no MMC, USB or NVMe support built in and the NAND node is
disabled, so a physically populated NAND/eMMC that the vendor left unused would
be invisible here. The stock U-Boot env still carries Annapurna reference-board
NAND boot scripts, which proves nothing about population. A chip marking from
the compute card would settle it.

Note: the capture includes a full `fw_printenv`, which contains ONIE's dropbear
**private host keys**. The capture is therefore git-ignored (`/serial-*.txt`)
rather than committed.

### 2026-10-17 — Claude Code / Fable review

- Completed the requested broad review with `claude-fable-5-1`, plus a focused
  earlycon source check using the same model. The broad review's internal advisor
  timed out; Fable itself returned a completed assessment.
- Confirmed incorrect explicit-baud earlycon divisor and omitted vendor no-map
  reservations. Identified packaged 50-MHz versus logged 75-MHz timer discrepancy
  and the need for explicit temporary bootargs handoff.
- Independently checked the findings: rejected the missing DMC12 address claim
  and alleged variant-specific Marvell switch difference; did not accept the
  blanket inference that stock devices all used legacy interrupts.
- Saved [original review](reviews/2026-10-17-fable.md),
  [focused UART review](reviews/2026-10-17-fable-uart.md) and
  [triage with evidence and follow-ups](reviews/2026-10-17-triage.md).
- Documentation-only review. DTS, kernel config, init, scripts and artifacts
  remain unchanged; corrections are pending. No SSD or switch access this turn.

### 2026-10-17 — deeper offline research and first mainline build

- Found public MikroTik GPL patch with Alpine v3 HAL 2.6; recovered the exact
  revision-4-to-2 standard-Ethernet compatibility path and PCI coherency setup.
- Safely unpacked ONIE updater/FIT/rootfs as data, validated payload SHA-1 and
  FIT CRCs, and recovered platform startup/firmware installation details.
- Mapped both I2C trees and the board-variant discriminator. Recovered and
  emulated system/fan watchdog and inverted max-fan setter behavior with stubbed
  I2C, producing reproducible register traces. See [board-control.md](board-control.md).
- Recovered 16 Innovium IPD ABI/support type layouts and 26 ioctl dispatch targets
  from module DWARF/disassembly; complete command semantics remain unfinished.
- Added a compact serial-only DTS, kernel config, initramfs, build script and QEMU
  smoke test. Linux 7.2.11 reaches an interactive shell with four emulated A72
  CPUs at EL2. This does not validate the actual board or board DTB.
- Fixed the initramfs library-path symlink after the first emulator boot failed.
  Corrected uImage load/entry to 0x04000000 based on the modern ARM64 Image header;
  stock 0x04080000 assumes a different text offset. CRCs/alignment verified.
- Added U-Boot tools and QEMU to the devshell for packaging/offline validation.
- Renamed `scripts/inspect.py` to `inspect_ssd.py`: the old filename shadowed
  Python's standard-library inspect module when argparse imported dataclasses.
  The accidental import attempted read-only setup but failed because the old
  temporary raw image was already deleted; no loop or mount was created.
- No source SSD read, switch connection, flashing or external posting this turn.
  Updated findings, bounded source-search results and live checks in
  [offline-research.md](offline-research.md).

### 2026-10-17 — kernel and upstream portability review

- Validated vendor uImage CRCs, decompressed its ARM64 Image and recovered IKCONFIG.
- Compared both DTBs, module metadata, and vendor config against Linux 7.2.11.
- Inspected user's DMC12 repository at `5d02d1b3de04082aea3960cd74db0c24f5b3ed32`.
  Recorded reusable MSI, clock, PCIe and U-Boot packaging details and v2/v3 gaps.
- Recovered historical dmesg from the compressed image: 16 Cortex-A72 r0p3 CPUs,
  PSCI 0.2, roughly 7.75 GiB described RAM, NIC revision 4 handled via revision-2
  compatibility. These are stored logs, not new live hardware measurements.
- Recorded firmware handoff and load-address checks needed before a 7.x trial.
- All work stayed offline from the switch; temporary partition image removed.

### 2026-10-17 — initial SSD acquisition

- Created this device project at the user's request.
- Confirmed `/dev/sdb` model, serial, exact size, and that no partitions were mounted.
- Set the kernel block device read-only before acquiring; no writes to SSD media.
- Acquired the entire disk, including free/unpartitioned space, through SHA-256
  and `zstd -T4 -6 --check`, 20:41:01–20:45:43 UTC. No read errors and no zero
  substitution. Raw SHA-256:
  `4681fc3f7ab837ea51dea780834aa0336563f70adb361791d226b0b719fb21c2`.
- SMART autodetection failed through the USB adapter; explicit `-d sat` worked.
  SMART health reports PASSED, 74 power-on hours, 161 power cycles, zero reported
  reallocated sectors, uncorrectable errors, or interface CRC errors. No recorded
  SMART errors or self-tests. Vendor-specific attributes are not decoded reliably
  because this SSD is absent from the tool's database.
- The disk uses a DOS/MBR table with seven Linux filesystems and an extended
  partition. All seven were inventoried from the verified copy using read-only
  loop devices and `ro,noload,nosuid,nodev,noexec` mounts. No image programs ran.
- Full archive verification completed at 20:47:12 UTC. Compressed SHA-256:
  `0f046f54676b35a82e3bdd5dc184fb60b7da5a017fa4df26137505c2a7c66cd5`.
  Both raw and compressed checksums are also saved under `images/`.
- Saved original label photos and converted their HEIF content to viewable PNGs.
- Added zstd, smartmontools and e2fsprogs to the existing ai_re devshell for this
  acquisition and filesystem inspection. All project tooling ran in that shell
  after reading the repository instructions.

## next useful work

1. Link + traffic test once QSFP-DD optics/DACs arrive (CPU path on fpp*,
   then port-to-port forwarding).
2. Credo slice temperatures and the ASC10 voltage/temperature monitors.
3. Populate `/var/lib/cs8210-sdk/root` reproducibly (extract from the `onie`
   MTD on the box) instead of by hand; decide on enabling the service at boot.
4. Kernel MDIO/PHY and QSFP I2C drivers for the two FPGAs (register maps are
   in the reviews); real MACsec keying; switchdev-style offload on top.
5. Aux SPI flash gating, second NIC SerDes, quiet fans in U-Boot.

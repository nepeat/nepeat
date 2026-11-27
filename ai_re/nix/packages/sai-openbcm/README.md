# sai-openbcm: the C glue between our SAI and the Broadcom OpenBCM SDK

What is left of this package (2026-11-23): `src/sdk_glue.c` and `src/sdk_glue.h`, the only C of ours
that includes Broadcom SDK headers. Everything else it was (the C++ `libsai.so` on the fake and
openbcm backends, `openbcm-diag`, `openbcm-mdio-script`, `openbcm-asic-temp`, the table checkers,
their tests and VM tests) was replaced by the Rust SAI in `rust/sai` and deleted (gates G5 / G6,
`rust/sai/README.md`). The AS7760 and AS7772 run the Rust package
(`rust/sai/nix/sai-openbcm-rs.nix`, `hardware.openbcm` in `nix/modules/switch/openbcm.nix`) under the
names this package had. Its C++ frontend lived on, frozen, in the CS8210's C++ sai-teralynx package until
that was deleted too (H5, 2026-11-27; last in the tree at commit 767d9b0): the CS8210 runs the Rust SAI.

History: this README as it described the C++ SAI is `git show 50a6aa7:ai_re/nix/packages/sai-openbcm/README.md`
(design, profile keys, supported objects, ACL / CoPP, what orchagent reads, the external PHY and
MACsec tables, the MDIO script tool, `openbcm-asic-temp`). The Rust SAI keeps that behaviour (the
same profile keys, the same SDK calls: `rust/sai/README.md` "Parity"), except the differences listed
there.

## The glue

- `sdk_glue.h`: 75 functions, five structs, three callback types, the `OB_*` codes and the order
  enums (`ob_port_stat_t`, ...). The Rust side binds it with bindgen (`rust/sai/crates/sdk-glue-sys`,
  `SDK_GLUE_INC`) and checks every declaration against it with the target's C compiler; `sdk-glue`
  is its only caller.
- `sdk_glue.c` is compiled with exactly the defines the SDK was built with
  (`share/openbcm/cflags` of `packages/openbcm` `libsdk`: chip, endianness, word size, features), so
  struct layouts match; nothing else sees an SDK type. Built by `rust/sai/nix/libsai-openbcm.nix` (into
  `libsai.so`, with the SDK tools: `sai-openbcm-rs.nix`) and `rust/sai/nix/mdio-tool.nix`.
- Its rules (types in `sdk-glue`): no MDIO before misc init, one bring-up per process, BCM calls only
  after "init all"; **one SDK process per unit**: the boot takes `/run/openbcm-unit<N>.lock` (flock,
  held for the process's life) and fails with `OB_E_BUSY` when another holder (`openbcm-asic-temp`,
  the wrapped `bcm.user`, syncd) has it. The out-of-range checks of 27 functions: `rust/sai/crates/sdk-glue-sys/ctest`.
- Tested: the bindings on every build; the calls the Rust backend makes, against the C++ backend's,
  by the glue-call golden (`rust/sai/crates/sai-backend-openbcm/tests/golden`); on the chip: the
  AS7772 (G2 smoke, G4 under SONiC: `devices/edge-core-as7772-32x/evidence/rust-g2`).

## Safety rules

- After SDK init (and the rc script), **every front-panel port is disabled**
  (`sai-core::switch`, every backend): SAI's default `ADMIN_STATE` is
  false and SONiC sets it explicitly; the SDK leaves ports enabled after
  `init all`. Ports are only enabled by `SAI_PORT_ATTR_ADMIN_STATE=true`.
- **Default VLAN 1 keeps the SDK's membership** (all front-panel ports
  untagged + CPU) and is reported as 32 VLAN members on 32 default bridge
  ports, which is what orchagent expects to find and remove; we do not strip
  it ourselves.
- **KNET**: stale netifs and filters are destroyed at init (IDs survive SDK
  restarts; a stale filter once delivered frames to the wrong netdev). The
  SDK's own default filter to the RX API is kept.
- No config file, no init. No fake on a host with BDE devices. A config with
  `l3_alpm_enable` is refused while libsdk is built without ALPM (the soc layer
  would lay out the LPM for ALPM and every route add would land where the chip
  never looks: hardware try 2, 2026-11-15).
- **One SDK process per unit**: the SDK boot takes `/run/openbcm-unit<N>.lock`
  (flock, held for the process's life) and fails with BCM_E_BUSY when another
  holder (`openbcm-asic-temp`, the wrapped `bcm.user`, another syncd) has it.
- Tomahawk management ports (`PBMP_MANAGEMENT`, xe0/xe1 in the factory
  configs) are not exposed as SAI ports.
- **Temperature files** (`sai-core::temp`): written only after a
  successful switch create, removed on switch remove, at `exit()` and after
  a failed or implausible read (outside -40..150 C), so fand sees "no
  sensor" (fans to 100%), never an old reading. The fake never publishes on
  a host with BDE devices. No over-temperature action in SAI mode: "init
  all" armed the SDK's own handler (core PLL off at 125 C).
- **LAG members** carry no L2/L3 state of their own: `create_lag_member`
  fails (`OBJECT_IN_USE`) while the port has a bridge port or router
  interface (orchagent removes the default ones first). A port's hostif and
  KNET netdev are untouched by LAG membership.

## Known gaps and risks

These are the SDK's and the board's, so they hold for the Rust backend (same glue calls).

- **Hardware coverage** (2026-11-15): one 25G lab loop, both ends on the same
  switch. Not exercised: FEC controls, port speed changes that need FlexPort,
  `bcm_port_resource` vs `SOC_INFO` lanes, 100G cables, line rate.
- **Endianness**: only `sdk_glue.c` converts to SDK types; IPv4 addresses
  are assembled from network-order bytes into host integers (`bcm_ip_t`), MACs
  and IPv6 are byte arrays, so the code is byte-order agnostic. The SDK's own
  BE settings (`SYS_BE_PIO=1 SYS_BE_PACKET=0 SYS_BE_OTHER=1`) are the ones the
  bring-up proved with bcm.user and KNET. SAI and SDK headers are compiled for
  the target, no on-wire structs are exchanged.
- **KNET**: netifs are TX_LOCAL_PORT on the port (bypass switch logic) with the
  port's PVID; RX only for packets the ASIC sends to the CPU, which needs traps
  or CPU VLAN membership. LACP and LLDP are trapped (L2 user entries; "init
  all" already sends 01:80:c2:00:00:0x to the CPU); BGP, BFD, SSH and other
  traffic to the switch's own addresses have no trap of their own and reach
  the CPU through the IP2ME host/route entries (CoPP meters them with field
  entries, above); BGP and BFD work this way on the chip. Packets
  not taken by a KNET filter are dropped by our RX handler (no SAI packet API).
- **LAG**: `INGRESS_DISABLE` is the SDK's `BCM_TRUNK_MEMBER_INGRESS_DISABLE`:
  frames received on that member are handled as from a plain port (not
  dropped, learned on the port, which has no SAI bridge port, so no FDB
  event). Trunk hashing is fixed (RTAG 6); LAG hash attributes are stubs.
  On the chip: LAG + member egress disable through syncd, LACP PortChannels
  under orchagent (2026-11-15); INGRESS_DISABLE not exercised there.
- **orchagent on big-endian**: PortsOrch writes `attr.value.u32` for the
  `sai_uint16_t` PORT_VLAN_ID (port and LAG); sairedis serializes `.u16`,
  which on ppc64 is the upper half, i.e. 0. We reject VLAN 0
  (`INVALID_ATTR_VALUE`); fixed in orchagent by swss patch 0001 (nix/packages/sonic/swss).
- **Threads**: SDK linkscan, counter, L2X and RX threads run inside syncd; our
  callbacks only queue. `remove_switch` unregisters callbacks but cannot
  detach the SDK (no clean detach path); a second create in the same process
  re-runs `init all` only (untested on hardware).
- **DMA**: KNET + SDK RX use the BDE DMA pool (`dmasize=16M` factory value);
  untested under load with syncd.
- **L3 termination** (2026-11-15, after the first hardware try dropped every
  unicast frame to the router MAC): the router MAC is one my-station entry
  (`bcm_l2_station_add`, any VLAN) with IPv4, IPv6 and ARP_RARP termination
  (the ARP reply trap needs the last); SDK init sets `bcmPortControlIP4/IP6`
  on every front-panel port (PORT_TAB V4L3/V6L3_ENABLE); an L3 interface's
  VLAN has IPv4/IPv6 L3 enabled. Not an L2 entry with the L3 bit
  (`BCM_L3_ADD_TO_ARL`): it sits on the CPU port, and PORT_TAB's static-move
  drop would discard every frame with SA = router MAC, which on the
  same-switch loop is all of them. Works on the chip (2026-11-15); debug with
  `openbcm-diag` (sonic-nixos vrf-ecmp-test.md, "debug").
- **VRF / ECMP on hardware** (both work on the chip, 2026-11-15): the VRF binding is the
  SDK's (`l3a_vrf` on the L3 interface: VLAN_TAB VRF_ID or the L3_IIF entry of
  the internal VLAN); ECMP groups use the SDK's default group size
  (`max_paths` 0) and are replaced in place (`BCM_L3_REPLACE|WITH_ID`).
  The hash spread is unseen (on the loop both members egress one port); the free ECMP member count reads the
  SDK's L3 software table (`bcm_int/esw/firebolt.h`), an internal structure.
  ECMP hashing is the SDK default (no hash attributes programmed).
- **Not implemented**: QoS (DSCP/TC maps, TC->queue, schedulers, WRED),
  buffers, egress ACLs, ACL ranges/redirect/mirror, port breakout, warm boot
  (`RESTART_WARM` is only stored), mirror, tunnels, sFlow, STP instances.
  orchagent survives all of them (VM test): it reads the
  capacities ([What orchagent reads](#what-orchagent-reads)) and takes its "unsupported" paths.
  QoS is left out on purpose (owner: low priority): SONiC only programs it
  when CONFIG_DB has QOS tables, which the Nix base does not load.
- **ACL / CoPP on hardware** (2026-11-15, [acl-copp-test.md](../../../devices/sonic-nixos/acl-copp-test.md)):
  an L3 ACL bound to one port drops and counts; IP2ME policing (packet-mode
  meter, `DstL3Egress` = the CPU egress object) holds a 66k pps flood to
  ~110/s at 100 pps. Open: ARP passes its 600 pps cap (818/s; probably the
  flood copy to the CPU, which the CoPP entry does not meter). Not checked
  on the chip: IFP group widths for wider qualifier sets, in-ports on a LAG
  or several ports, and meter sharing between one CoPP group's entries (if the
  SDK refuses, each entry gets its own copy: per-entry, not per-group, rate).
- **pysairedis callbacks**: pysairedis' own notification trampolines build
  Python objects without the GIL and crash when libsairedis calls them from
  its notification thread (seen with Python 3.14). `rust/sai/nix/sai_smoke.py`
  registers a ctypes callback instead (it takes the GIL). Not our bug, but
  any Python test client hits it.
- **Symbols**: `libsdk.so` exports every SDK symbol (including `main` from
  socdiag.o, harmless in a library); a name clash with something else syncd
  loads is possible. `libsai.so` exports only `sai_*`.

## Sections that moved

Kept as headings so that links into this file still land; each says where the content is now.

### openbcm-asic-temp

Rust crate `rust/sai/crates/openbcm-asic-temp` (same command line, messages, exit codes, files; the
C tool's answers as a golden); unit: `nix/modules/switch/openbcm.nix`. The C++ text: History above.

### ACL and CoPP (field processor)

`rust/sai/crates/sai-backend-openbcm/src/field.rs` (the C++ policer-clone fallback included) and
`sai-core`'s ACL area; the C++ description: History above.

### What orchagent reads

`rust/sai/crates/sai-core` (`caps.rs`, the switch area); the C++ table: History above.

### External PHY table: link state behind a PHY the SDK does not manage

Format and engine: `rust/sai/crates/phy-seq` (`ext-phy 1`), runtime `rust/sai/crates/sai-extphy`; the
AS7772's table: `nix/modules/boards/as7772/ext-phy.nix`.

### MACsec in an external PHY (SAI MACsec API)

Format `rust/sai/crates/phy-seq` (`ext-macsec 1`), model `rust/sai/crates/sai-macsec`; the AS7772's
table: `nix/modules/boards/as7772/ext-macsec.nix`, `devices/edge-core-as7772-32x/macsec.md`.

### openbcm-mdio-script: PHYs without an SDK driver

Format `rust/sai/crates/mdio-script`, tool `rust/sai/crates/mdio-tool` (status files and failure
classes: `mdio-report`); unit wiring `nix/modules/switch/openbcm.nix` `mdioScript` (VM test
`nix/modules/switch/tests/openbcm-mdio-units.nix`).

## Licence

Our code (this directory): Apache-2.0. A build with the glue links Broadcom's SDK (`libsdk.so`,
Broadcom Switch Software License, source-available, see the SDK's Legal/LICENSE): the combined
`libsai.so` and the SDK tools are **not freely redistributable as binaries**. No vendor config files
are included.

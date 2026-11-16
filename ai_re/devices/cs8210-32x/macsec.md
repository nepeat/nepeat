# MACsec on the Credo PHYs: capabilities, key changing, MKA (2026-11-16)

**Status: designed and implemented offline; offline tests pass (static SAs, hitless rekey,
MKA bring-up and MKA rekey against a stub engine); hardware tests not run yet.** The 400G link
and yesterday's static-key results: [macsec-400g.md](macsec-400g.md). Code:
`nix/nixos/module/cs8210/macsec/` (`hardware.cs8210.macsec`, default off, not imported yet).
Tags: **V** verified (disassembly/decompile, DWARF, offline test), **I** inferred, **HW** needs
the hardware.

Sources (local, git-ignored, `analysis/macsec-ddk-20261116/`, `SHA256SUMS` there): Ghidra
decompiles of the SDK 0.1.7 `c-phy/` binaries (`libdriver_164_u.so` de83d2d0…,
`libcredoCmd.so` 233be30e…, `phy_server` d8f9d391…, `phy_shell` cd883a78…) and DWARF type
dumps of Credo's 2026 `libSerdes_owl800.so` (01109eeb…, from the public SAI 1.2.12 deb, see
[credo-sai/](credo-sai/README.md)), which links the same Rambus/Inside Secure EIP-163/164 DDK
statically with full debug info. The 2021 layouts are the 2026 ones minus IPsec (V: offsets in
the 2021 code, `_Static_assert`s in the shim).

## Capabilities

The engine is the DDK in `libdriver_164_u.so` (EIP-163 classifier "CfyE", EIP-164 transform
"SecY", SA builder), two devices per slice: `2(s-1)` ingress, `2(s-1)+1` egress. The vendor
CLI (`libcredoCmd.so`, Celestica's `cmds.c`) imports only a fraction of it.

| feature | engine (DDK) | vendor CLI | ours |
| --- | --- | --- | --- |
| cipher suites | GCM-AES-128, -256, -XPN-128, -XPN-256 (key 16/32 B, 24 rejected; `LONGSEQ` flag = 64-bit PN) V | all four (`auto_load_user.sh -e`), fixed key | all four static; MKA: 128/256 (wpa_supplicant 2.12 has no XPN) |
| used on 2026-11-15 | | GCM-AES-128 and -256, 32-bit PN, **`ROLLOVER` flag** | no `ROLLOVER` ever |
| confidentiality offset | 0..63 B (6-bit field; 802.1AE 0/30/50) V | 0 | 0/30/50 (policy) |
| integrity only | `fConfProtect=0` (E=0, C=0) V | no | `encrypt=0` / MKA `integrityOnly` |
| SecTAG | SC (include SCI), ES, SCB, PreSecTagAuth start/length (12 = DA+SA in the ICV), SecTagOffset per vPort, parser EtherType/version/KaY checks per device V | SC=1, ES=0, SCB=0, auth length 12 | SC=1 always (our RX finds SCs by SCI), ES/SCB policy |
| replay protect | per flow on/off + per-SA window (32 bit, updatable live) V | on, **window 0xffffffff** (= effectively off) | on, window from config / MKA (default 0) |
| validate frames | disabled / check / strict + allow tagged/untagged, validate untagged V | strict, untagged allowed | strict (untagged dropped) unless policy says otherwise |
| SAs per SC | ingress 4 (AN 0..3) at once; egress 1 active + 1 chained V | 1 | 4 / 1+next |
| SCs, vPorts, SAs, rules per device | runtime limits (`SecY_Device_Limits`, `CfyE_Device_Limits`) | | `cs8210-macsec hello` prints them **HW** |
| traffic selection | TCAM: 4 x 32-bit data words (MAC DA in 0-1, EtherType + outer VLAN TCI in 2, inner TCI in 3, frame bytes packed little-endian), packet type (other/MPLS/PBB/MACsec), tag count (up to 3), channel; priority 0-7, drop, control-packet flag; separate control-packet detector (8 DA/EtherType entries, DA ranges) V | one match-all rule per direction; its `da=` `num_tags=` `pkt_type=` arguments are parsed and ignored V | match-all data rule + EAPOL rule |
| EAPOL (0x888E) bypass | rule + BYPASS SA V | none (control-packet detector initialised all-zero) V | EtherType rule `Data[2]=0x8e88`, priority 7, control packet, BYPASS SA, both directions |
| next PN | get/set per SA, 64-bit with XPN V; egress record holds the last PN sent (I: SeqNum 0 gave PN 1 on the wire) | set once (none -> 0) | `cmx_sa_pn`, `cs8210-macsec pn` |
| PN threshold / exhaustion | device `SeqNrThreshold`(64) + PN-threshold and SA-expired summaries, `SA_EXPIRED_IRQ` flag V | threshold 0 (off) | polled (MKA, metrics `pn_used_ratio`) |
| GCM hash subkey | `auth` = H = AES_K(0^128), required (no callback) V | given in the command file | computed by the client |
| key storage | transform record in engine context RAM (egress 22 words: key, H, seq, salt^SSCI, SCI; ingress 20), written by `SecY_SA_Add`/`Chain` | | never read back; buffers wiped |
| counters | per SA (out 4, in 10), per SecY (out 3, in 8), per interface (out 9, in 8), per rule (TCAM hits), all **64 bit** V | prints the low 32 bits only | all, 64 bit |
| hitless rekey | ingress: next AN beside the current; egress: `SecY_SA_Chain` (new SA, auto-switch at expiry) + `SecY_SA_Switch` (switch now, sync + verify) + `SecY_SA_Remove`; `SecY_SA_Update` per-SA in-use V (Credo's 2026 `MACsec_SA_Rekeying` does the same) | none | yes (static: `rekey`; MKA: driver) |
| engine on/off | `enable_low_latency_bypass` per side (= vendor `macsec_enable`/`disable`) V | yes | `cmx_port_up` / `cmx_port_down` |

SA builder flags (`SAB_MACSEC_FLAG_*`, V): `LONGSEQ` 1 (XPN), `RETAIN_SECTAG` 2, `RETAIN_ICV` 4,
`ROLLOVER` 8, `UPDATE_ENABLE` 0x10, `SA_EXPIRED_IRQ` 0x20, `UPDATE_TIME` 0x40 (rejected),
`NO_LATE_CHECK` 0x80, `LATE_HDR_DROP` 0x100, `LATE_HDR_CHECK` 0x200, `DISABLE_UPDATE_SEQ_NUM` 0x400.

### Why yesterday's key change did not work (V)

- `macsec_sa_set` only edits the CLI's in-memory SA node. The transform record is built and
  written (`SABuilder_BuildSA`, `SecY_SA_Add`) only inside `macsec_vport_add`.
- `macsec_datapath_flush` is `CfyE/SecY_Device_InsertEOP` (drains the pipeline) and removes
  nothing. A second allocate + `vport_add` adds a second vPort, match-all rule and SA. Both rules
  have priority 0, the older (lower index) wins, so traffic stayed on the old SA and the new ones
  counted 0.
- The CLI imports no `SecY_SA_Remove`, `Chain`, `Switch` or `NextPN_*`.

### Other findings

- **`ROLLOVER`** (vendor loader, and our `scripts/credo_macsec.py` until today): by its name
  and use (control-word bit 0x400; Credo's 2026 code never sets it) it lets the 32-bit PN wrap,
  i.e. reuses GCM nonces: after ~134 s at 386 Gb/s with 1500 B frames, ~11 s at 64 B (I).
  Yesterday's keys were throwaway. The script no longer sets it.
- **phy_server runs every unknown command line with `system()`** (V, `_cmd_dispatch`), and its
  FIFO was created 0777 & ~umask in the 1777 chroot `/tmp`. `cs8210-switch-up` now makes it
  0600, and both clients send only `cmx_[a-z_]+( k=[A-Za-z0-9_.:-]+)*`.
- The SA counters only looked 32-bit: the CLI prints `.Lo` (corrects macsec-400g.md).
- When the last SA of a vPort goes, the DDK writes a BYPASS flow (`SAMFlow_Clear`), i.e. clear
  text on egress. Our design gates data with the rule's drop flag instead (below).

## Design

```
wpa_supplicant -D macsec_cs8210 ─┐            key file 0600, read once + unlinked
cs8210-macsec (status/rekey/...) ┴─ FIFO ──> phy_server ── vendor CLI (unchanged)
   (phy_shell protocol + flock)              └─ shim (LD_PRELOAD): cmx_* commands
                                                 └─ libdriver_164_u (CfyE/SecY/SABuilder)
                                                    └─ eip_register_* (g_slice) ─ libcredo ─ our libMdio ─ PHY
```

- **Shim** (`shim/cs8210-macsec-shim.c`): hooks phy_server's one `daemon()` call (right
  after the vendor registered its commands) and adds `cmx_hello`, `port_up/down/reset`,
  `policy`, `sa_add/enable/del`, `sa_pn`, `status`. It runs in phy_server's single CLI
  thread, so it is serialised with the vendor commands. Vendor symbols are weak references,
  libcredoCmd's `devices`/`g_slice` come from its `.symtab`. glibc 2.17 symbols only, libc
  only. Inert in any other process.
- **Per port and direction**: an EAPOL vPort (BYPASS SA, EtherType rule, priority 7) and a data
  vPort (match-all rule, priority 0) whose **drop flag is the gate**. Data is dropped unless the
  controlled port is enabled and an SA can carry it (or policy `open`). The drop goes on
  before the last SA is removed. `port_reset` deletes all SAs and keeps the gate shut;
  only `port_down` returns to the vendor's clear-text bypass.
- **TX**: an SA added with `enable=0` waits in shim memory; `enable` = `SecY_SA_Add` for the
  first one, else `SecY_SA_Chain` + `SecY_SA_Switch` (hitless); the old SA is "retired" until
  deleted. A failed switch blocks TX. **RX**: installed at once (next AN beside the current),
  enabled/disabled with `SecY_SA_Update`.
- **Keys** never in the FIFO, argv or logs; transform records and key buffers are wiped.
- **MKA**: wpa_supplicant 2.12 with a new driver (`wpa_supplicant/driver_macsec_cs8210.c`,
  built on the `driver_macsec_qca` pattern, EAPOL through `driver_wired_common`), PSK CAK
  from a root-only file; `macsec_init` = port up with the controlled port closed; exit =
  `port_reset` (fail closed).

Alternatives, and why not: Linux MACsec offload has no path (ipd has no macsec ops, and the PHY
is driven from userspace). The Credo SAI under qemu-user (SONiC's path) still has the stop-ship
items in [credo-sai/](credo-sai/README.md) 2.8: chip-reset paths, coexistence with phy_server.
Kernel software macsec on `fppN` only covers CPU traffic (~2.8 Gb/s); it stays as the interop
reference. Our own MKA implementation would only duplicate wpa_supplicant's KaY.

MKA limits (V, hostap 2.12 source): GCM-AES-128/256 only. The key server rekeys only on PN
exhaustion, when a member's transmit PN, sampled every 2 s hello, passes 0xC0000000, or on
membership change; there is no rekey timer. At 400G that is ~100 s between rekeys with 1500 B
frames but ~8 s at 64 B line rate, leaving ~2.6 s to 2^32. Without `ROLLOVER` an exhausted SA
stops (fails closed). XPN for MKA needs a KaY patch (802.1AEbw salt and SSCI distribution).

## Use

```
cs8210-macsec keygen --loop 1:16 --cipher gcm-aes-xpn-256 --out /var/lib/cs8210-macsec/static.json
cs8210-macsec up --config /var/lib/cs8210-macsec/static.json     # receivers first, then transmitters
cs8210-macsec status [--slice 1] [--json]                       # the "MACsec info"
cs8210-macsec rekey --config ... --link 1:16                    # new SAK on AN+1, both directions
cs8210-macsec pn --slice 1 --dir tx --an 1 [--set 0xC0000000]
cs8210-macsec down --slice 1 --slice 16                         # back to clear-text bypass
```
NixOS: `hardware.cs8210.macsec.enable` (shim + tool + metrics producer, takes effect at the next
`cs8210-switch` start), `.static.configFile`, `.mka.{ports,cakFile,cipher,...}`. Metrics:
`cs8210_macsec_*` (rule hits, SecY/IFC/SA counters, next PN, PN used ratio, SA age; no keys).

## Tests

- 2026-11-16, offline, PASS: `test/run-test.sh` (shim + client against a stub DDK with the same
  symbols/layouts: hello, keygen, up XPN-256, status, hitless rekey, pn set, port_reset, metrics,
  down, re-up GCM-AES-128; no key files or key bytes left behind; the stub flags misuse such as
  Chain on a non-active SA or `ROLLOVER`). `test/run-mka-test.sh` (two wpa_supplicant-cs8210
  on a veth pair fpp1/fpp16, PSK CAK, GCM-AES-256): 4 SAs installed, controlled port open; TX PN
  pushed to 0xC0000010 -> "OLPN exhaustion", new SAK, RX AN 1 installed before TX switched by
  Chain + Switch; on exit both slices closed with no SAs.
- Builds (aarch64): shim (GLIBC_2.17 only, NEEDED libc), `cs8210-macsec`,
  `wpa_supplicant-cs8210` 2.12.
- **Hardware: not run.** Plan below.

## Hardware plan (needs a window of ~90 min, ASIC free, only cages 1 and 16)

1. Re-check presence; start `cs8210-switch` with the shim and yesterday's copper drop-in;
   `hello` (limits), `status`.
2. EAPOL path in bypass: one EAPOL frame fpp1 -> fpp16 (does the ASIC trap 01:80:c2:00:00:03?).
3. Static, each cipher: `up`, CPU path ping/iperf, ASIC loop at line rate, 64-bit counters,
   first PN on the wire, interop: slice 16 in bypass + Linux `ip macsec` on fpp16 decrypting
   slice 1 (and back), XPN included; wrong key -> NotValid; last SA deleted -> no clear text.
4. Hitless rekey under line-rate traffic, both directions, several times: count loss
   (ASIC tx vs rx frames, InPktsLate/NotValid/NotUsingSA).
5. MKA on fpp1/fpp16: bring-up time, traffic, rekey by TX PN push, wrong CAK stays closed,
   port flap (`port disable/enable` in cls_shell) recovers.
6. `down`, stop `cs8210-switch`, remove the drop-in.

## Open / risks

- EAPOL to the CPU on the receiving port, through the ASIC: **HW**.
- From the vendor config, not otherwise checked: `SecTagOffset 12`, `PktExtension 3`.
  From Credo 2026: ingress DestPort CONTROLLED, DropType INTERNAL, `fAllowUntagged` off.
  "Priority 7 wins": EIP-163 policy field (3 bit) plus Credo's `MaxRulePriority 7` (I).
- Egress next PN = record + 1 (I). Engine MTU check (`SecY_Rules_MTUCheck`) not configured.
- A phy_server crash loses PHY control until `cs8210-switch` restarts.

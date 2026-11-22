# BCM82391 "Evora": public source, firmware and how to drive it

Research 2026-11-22 (offline tree + web; the device was not touched). **V** = verified in a file or page read today,
**I** = inferred. Board facts are in [extphy.md](extphy.md).

## Result
- **No public source and no public firmware for Evora exists** (V, as far as GitHub code search and the trees below
  show). The only public trace of the driver is its API header (`epdm.h`) and unused glue in OpenBCM.
- `phy82381.c` in OpenBCM is **Furia (BCM82381, the non-MACsec sibling), not Evora**; it does not accept 0x82391.
- The only Evora driver + firmware we have is inside the vendor `dumps/x/diag/root/usr/local/sbin/bcm.user`
  (private, stays out of git and `nix/`).

## 1. OpenBCM 6.5.27 (our SDK; source `/nix/store/lgykiyx22bsz6vjh9dap980nwlmpigbz-openbcm-sdk-6.5.27-patched`)
- `src/soc/phy/phy82381.c` (V): "Support for Furia external PHY", SDK wrapper over phymod chip **furia**
  (`libs/phymod/chip/furia`, present with firmware `tier1/furia_ucode.c`, 248 kB source, version D015).
  Probe: `phyident.c` matches IEEE ID OUI 0xd40129 (`PHY_BRCM_OUI6`), model 0x23, any rev -> `phy82381_drv`.
  Chip ID then read from clause 45 dev 1 **0x8a00** (low 16 bits) and **0x8a01[15:12]** (digit 5):
  accepts 82070/71/72/73, 82380/81, 82209, 82212, 82314/15. **0x82391 is not accepted**; the furia package table
  (`furia_pkg_cfg.c`) has no 8239x either.
- No `libs/phymod/chip/evora`, no "evora"/"82391" string in the tree (V). Same chip list (no evora) in
  sdk-6.5.16, 6.5.21, 6.5.26-DNX.1 of the same repo (V, GitHub API). Present retimer/gearbox chips: furia,
  furia_82212, sesto, madura, dino, huracan, quadra28.
- **EPDM glue is public, the library is not** (V): `src/appl/diag/cint/plp_epdm_apis_cint_data.c`,
  `bcm_plp_cint_static_func.h`, `epdm_cint_func_table` expose `bcm_plp_*` to cint under
  `-DPHYMOD_EPDM_APIS_SUPPORT`; `make/Make.targets:1631` links
  `$(PHYMOD_EPDM_LIB_PATH)/build/.../libepdm.a`. The headers they include (`epdm.h`, `epdm_sec.h`, ...) and
  libepdm are not in the tree. So a licensed EPDM release would drop into our build; nothing public fills it.
- Build gating (V): `make/Make.config:1483` default `BCM_PHY_LIST` contains `82381 82764 82864 82109 82780 ...`;
  line 1502 turns each into `-DINCLUDE_PHY_<n>`; `PHYMOD_PHY_LIST = 82381 82764 82864 82109` pulls the phymod
  chips (`-DPHYMOD_FURIA_SUPPORT`, ...). Our `nix/packages/openbcm/Make.local.as7760` sets only
  `FEATURE_LIST = L3 EDITLINE PHYMOD KNET` and the chip, not `BCM_PHY_LIST`, so **Furia is already compiled
  in** (our built `bcm.user` has the Furia strings). No `MACSEC`/`XFLOW_MACSEC` feature is on; neither would
  help (they are for other PHYs / switch-internal MACsec).
- config.bcm keys that attach an SDK-driven external PHY (V, `include/soc/property.h`, `phy82381.c`):
  `port_phy_addr_<port>=<mdio id>` (bus in bits 8,6:5), `phy_chain_{tx,rx}_lane_map_physical{p.1}`,
  `phy_chain_{tx,rx}_polarity_flip_physical{p.1}`, `phy_force_firmware_load` (default 0x11 = download
  internal image; 0x21 = program SPI EEPROM; 0x01 = none), `phy_ext_rom_boot`, `phy_sys_interface`,
  `phy_pcs_repeater`, `phy_gearbox_enable`, `port_phy_lane_mask`, `rate_ext_mdio_divisor/dividend`.
  They only help for a PHY the SDK has a driver for. **The vendor does not use them**: `config.bcm-100G` has
  no `port_phy_addr` (V); the PHYs are driven beside the SDK by the standalone library through the diag
  `extphy` command. We can do the same: no SDK attach needed, only MDIO access.

## 2. Other public trees
| Where | What | Evora? |
| --- | --- | --- |
| https://github.com/Broadcom-Network-Switching-Software/OpenBCM (sdk-6.5.16 .. 6.5.27, last push 2023-08; Broadcom Switch Software License, `Legal/LICENSE`) | full SDK, phymod chips above | no source, no firmware (V) |
| https://github.com/Broadcom-Network-Switching-Software/OpenNSA (last push 2020-03), OpenFirmware, OpenBCM `Firmware/` (pcie, BFD, BroadSync, IFA, MOD) | prebuilt SDK lib; switch-side firmware | no (V for names; OpenNSA binaries not disassembled) |
| https://github.com/dusten/SDKLT `src/phymod/include/phymod/{epdm.h,epdm_sec.h,epdm_bcm_common_defines.h}` (2018 SDKLT copy, Broadcom Switch Software License in the file header; the Broadcom SDKLT repo itself now 404s) | **the PLP/EPDM API header**: `bcm_plp_*(chip_name, ...)` dispatch for "evora", "dino", "quadra28", "koi", "orca" under `PLP_EVORA_SUPPORT`; structs `bcm_plp_access_t`, firmware-load and MACsec (`epdm_sec.h`) types | **API only**; `plp_evora_dispatch` is an extern, no implementation, no firmware (V) |
| GitHub code search: `bcm_plp_evora`, `plp_evora_download_prog_eeprom`, `evora_ucode`, `evora_cfg_seq`, `PHYMOD_EVORA_SUPPORT`, `phymodDispatchTypeEvora` | | 0 hits (V; search does not index every fork or large file) |
| SONiC `sonic-buildimage` | `device/accton`: as7712/7716/7716b/7726 only; no "evora"/"82391" hit | no AS7772, no driver (V) |
| ONL / DENT / ONIE | ONL `packages/platforms/accton/powerpc`: as4600, as5610, as5710, as6700; ONIE `machine/accton`: no as7772 | no (V) |
| Linux mainline | no bcm8238x/8239x driver (code search in torvalds/linux: no hit) | no (V) |
| Goldstone (`oopt-goldstone/goldstone-buildimage`, Wistron WTP-01-02-00 "Galileo") | board has BCM56960 + BCM82391; `.../builds/diag/galileo/fpga.py` names `FPGA_REG_BCM82391_INT1`, `FPGA_REG_PHY_RESET` | only the reset/interrupt register names; no PHY driver found (V for the file; rest of that tree not searched by hand) |
| FBOSS (`meta/fboss`) | build docs mention linking Broadcom `libepdm` | supplied by the user under Broadcom licence, not in the repo (V for the mention) |
| Edgecore support site / vendor NOS for the AS7772-32X | nothing found by web search; the model appears only in UL and KCC certification listings | unknown; a NOS image, if one exists, would carry the same binary library (I) |

PLP/EPDM releases come from Broadcom's customer portal under SLA (I; nothing public found). Arista uses the part
(NIST CMVP cert 3429), so EOS images contain a driver, but that is no easier than the binary we already hold.

## 3. Chip facts
From the product brief https://docs.broadcom.com/doc/82391-PB100 (V; title metadata "BCM82391/BCM82392/BCM82742"):
- **Dual 100G** MACsec retimer: two CAUI4-to-CAUI4 ports per chip (also 2 x 40G, 8 x 10G, 8 x 25G), GCM-AES-128/256,
  IEEE 1588, CL91/CL74/CL108 FEC, CL92/CL85 training, 156.25 MHz reference, **MDC/MDIO host interface at
  1.8 V/3.3 V**, 1.0 V core, 19 mm 324-ball BGA, **footprint compatible with BCM82381**, AC caps integrated.
  Siblings on the NIST CAVP listing 3272: BCM82396, BCM82398, BCM82399.
- So 32 cages = **16 chips**; the vendor's 8 buses x 4 addresses (`nphy=4`) = 32 addresses fits **one MDIO
  address per 100G port, two per chip, two chips per bus** (I). Furia is built the same way: its 82381 package
  table has two 4-lane dies.
- The brief does not state the MDIO clause, ID registers, straps or power-up mode. No full datasheet is public.

From code and the vendor binary:
- Clause 45 (I): every PLP sibling in OpenBCM is clause 45 dev 1, and our own probe used c45.
- **Chip ID register is not IEEE 1.2/1.3** in this family (V for siblings): Furia and Sesto use dev 1 **0x8a00**
  (ID low 16) / **0x8a01** (ID digit 5 in [15:12], revision); Madura and Dino use dev 1 **0x8b00/0x8b01**.
  Expected Evora value 0x2391 (I): the vendor binary compares against 0x2391 and 0x2392 (`cmplwi`, 2 sites each;
  V by opcode scan, the register address was not recovered). **Our 2026-11-21 probe read only 1.2/1.3**, so its
  "no ID" result does not yet say the PHYs are absent or in reset.
- Evora is a MAC-PHY, not a plain retimer (V, source paths in the vendor binary):
  `phymod/chip/evora/evora_pm/pcs_pmd/{falcon_evora,tscf_evora}`, `evora_pm/pms/pm4x25.c`, `portmod_chain.c`,
  `tier1/evora_cfg_seq.c`, `evora_reg_access.c`: Falcon SerDes + TSCF PCS + PM4x25 (CLMAC) on each side. This is
  why the vendor sets `CLMAC_TX_CTRL AVERAGE_IPG`.
- Firmware (V for strings, I for the conclusion): "Initialize Evora and Download Firmware",
  `plp_evora_download_prog_eeprom`, and the family strings "EEPROM Download cannot be performed when serboot
  pin is high", "Download method NONE is only to support flashing from EEPROM". As in Furia
  (`furia_micro_seq.c`: checks `boot_por` `mst_dwld_done`/`slv_dwld_done`, serboot strap, optional SPI
  EEPROM), the chip needs microcode from MDIO download or an SPI EEPROM. **It most likely does not pass
  traffic unconfigured** (I: it terminates PCS and MAC on both sides, and `extphy cfg ... speed=100000
  if_line=CAUI4_C2C if_sys=CAUI fec=1` is required even for bypass). Whether the board fits SPI EEPROMs is unknown.
- Reset: unknown here. The Galileo board gates its BCM82391s with an FPGA `PHY_RESET` register, which makes a
  CPLD-held reset on our board plausible (I; see [cpld.md](cpld.md) 0x20-0x2b).

## Recommendation: soonest route to a 100G bypass link
1. **Read-only ID probe at the family registers** (minutes): c45 dev 1 0x8a00/0x8a01 and 0x8b00/0x8b01 on the
   32 vendor addresses. 0x2391 confirms part, addressing and that the PHYs are out of reset.
2. **Boot the factory diag from NOR and run `rc.soc-100G`** on the loopback DACs: proves the hardware, and gives
   before/after values for CPLD2/3 0x20-0x2b and the ID registers. If the vendor shell can log MDIO
   (`debug` on the miim/phymod layers; not checked that SDK 6.5.9 prints every access), capture the whole
   `extphy init` + `extphy cfg` transaction stream including the firmware download.
3. **Replay or port** from our SDK: a small tool on libsdk that plays the captured clause-45 stream (firmware
   blob kept in `devices/.../dumps`, loaded at run time, never in `nix/`), or a minimal bypass init
   reverse-engineered from the vendor `bcm.user` with `epdm.h` and the public furia/madura/dino sources
   (same phymod structure, Falcon tier1 code is public) as the map. No SDK PHY attach or config.bcm PHY key is needed.
4. In parallel, ask Edgecore/Broadcom for the PLP EPDM "evora" release: OpenBCM already has the build hook
   (`PHYMOD_EPDM_APIS_SUPPORT`, `PHYMOD_EPDM_LIB_PATH`), and it is the only route to supported MACsec.

Not viable: `port_phy_addr` + `phy82381` (Furia driver rejects the ID and its firmware is for another chip;
do not let it download Furia microcode into an Evora). Running the vendor ppc32 6.5.9 `bcm.user` on our
kernel against the 6.5.27 BDE modules is untested and unlikely to match.

## Not verified
- The Evora chip-ID register address and value (inferred from siblings + immediates in the binary).
- Clause, two-addresses-per-chip layout, need for firmware, power-up mode, reset source: all inferred.
- Location/format/version of the Evora firmware in the vendor binary ("Evora phy driver v1.9" per `rc.soc-dbg`).
- OpenNSA binaries, Goldstone beyond one file, Edgecore's download portal (login) were not examined in depth.

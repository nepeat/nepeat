# BCM82391 "Evora": public sources and chip facts

Research 2026-11-21/22 (offline tree + web). **V** = verified in a file or page read then, **I** = inferred.
What we do instead: the vendor library re-hosted ([evora-rehost.md](evora-rehost.md)); hub: [extphy.md](extphy.md).

## Result
- **No public source and no public firmware for Evora** (V, as far as GitHub code search and the trees below
  show). Public: only the API headers (`epdm.h`, `epdm_sec.h`) and unused glue in OpenBCM.
- `phy82381.c` in OpenBCM is **Furia (BCM82381, the non-MACsec sibling)**; it does not accept 0x82391. **Never
  attach it with `port_phy_addr`**: it would download Furia microcode into an Evora.
- The only Evora driver + firmware we have is inside the vendor `bcm.user` (private, never in git or `nix/`).
- The only route to supported MACsec is a licensed PLP/EPDM "evora" release (Broadcom customer portal, I); OpenBCM
  already has the build hook for it. No request to Edgecore or Broadcom is recorded.

## OpenBCM 6.5.27 (our SDK)
- `src/soc/phy/phy82381.c` (V): wrapper over phymod chip **furia** (firmware `tier1/furia_ucode.c`, D015). Probe:
  `phyident.c` matches OUI 0xd40129, model 0x23; chip ID from dev 1 **0x8a00** / **0x8a01[15:12]**; accepts
  82070-73, 82380/81, 82209, 82212, 82314/15. No 8239x in `furia_pkg_cfg.c`.
- No `libs/phymod/chip/evora`, no "evora"/"82391" string (V); same in sdk-6.5.16, 6.5.21, 6.5.26-DNX.1 (V, GitHub
  API). Retimer/gearbox chips present: furia, furia_82212, sesto, madura, dino, huracan, quadra28.
- **EPDM glue is public, the library is not** (V): `src/appl/diag/cint/plp_epdm_apis_cint_data.c`,
  `bcm_plp_cint_static_func.h` expose `bcm_plp_*` to cint under `-DPHYMOD_EPDM_APIS_SUPPORT`;
  `make/Make.targets:1631` links `$(PHYMOD_EPDM_LIB_PATH)/build/.../libepdm.a`. Headers and libepdm are not in
  the tree.
- Build (V): default `BCM_PHY_LIST` (`make/Make.config:1483`) contains 82381, so **Furia is compiled into our
  `bcm.user`** (`Make.local.as7760` does not set the list). No `MACSEC`/`XFLOW_MACSEC` feature is on; neither
  would help.
- config.bcm keys that attach an SDK-driven external PHY (V, `include/soc/property.h`): `port_phy_addr_<port>`,
  `phy_chain_{tx,rx}_lane_map_physical{p.1}`, `phy_chain_{tx,rx}_polarity_flip_physical{p.1}`,
  `phy_force_firmware_load` (0x11 download, 0x21 = **program SPI EEPROM**, 0x01 none), `phy_ext_rom_boot`,
  `phy_sys_interface`, `phy_pcs_repeater`, `phy_gearbox_enable`, `port_phy_lane_mask`. They only help for a PHY
  the SDK has a driver for; the vendor uses none (`config.bcm-100G` has no `port_phy_addr`, V).

## Other public trees
| where | what | Evora? |
| --- | --- | --- |
| https://github.com/Broadcom-Network-Switching-Software/OpenBCM (sdk-6.5.16 .. 6.5.27, last push 2023-08) | full SDK | no source, no firmware (V) |
| OpenNSA (last push 2020-03), OpenFirmware, OpenBCM `Firmware/` | prebuilt SDK lib; switch-side firmware | no (V for names; binaries not disassembled) |
| https://github.com/dusten/SDKLT `src/phymod/include/phymod/{epdm.h,epdm_sec.h,epdm_bcm_common_defines.h}` (2018 copy, Broadcom Switch Software License; the Broadcom SDKLT repo 404s) | **the PLP/EPDM API header**: `bcm_plp_*(chip_name, ...)` for "evora", "dino", "quadra28", "koi", "orca"; `bcm_plp_access_t`, firmware-load and MACsec types | **API only**: `plp_evora_dispatch` is an extern (V) |
| GitHub code search: `bcm_plp_evora`, `plp_evora_download_prog_eeprom`, `evora_ucode`, `evora_cfg_seq`, `PHYMOD_EVORA_SUPPORT`, `phymodDispatchTypeEvora` | | 0 hits (V; search does not index every fork or large file) |
| SONiC `sonic-buildimage` `device/accton` | as7712/7716/7716b/7726 | no AS7772, no driver (V) |
| ONL / DENT / ONIE | ONL accton powerpc: as4600, as5610, as5710, as6700; ONIE: no as7772 | no (V) |
| Linux mainline | no bcm8238x/8239x driver | no (V) |
| Goldstone (`oopt-goldstone/goldstone-buildimage`, Wistron WTP-01-02-00 "Galileo": BCM56960 + BCM82391) | `.../builds/diag/galileo/fpga.py`: `FPGA_REG_BCM82391_INT1`, `FPGA_REG_PHY_RESET` | register names only (V for the file; rest not searched) |
| FBOSS (`meta/fboss`) | build docs mention linking Broadcom `libepdm` | supplied by the user under licence (V for the mention) |
| Edgecore support site / vendor NOS | nothing found; the model appears only in UL and KCC listings | unknown |

Arista uses the part (NIST CMVP cert 3429), so EOS images contain a driver; no easier than the binary we hold.

## Chip facts
Product brief https://docs.broadcom.com/doc/82391-PB100 (V; "BCM82391/BCM82392/BCM82742"):
- **Dual 100G** MACsec retimer: two CAUI4-to-CAUI4 ports per chip (also 2 x 40G, 8 x 10G, 8 x 25G), GCM-AES-128/256,
  IEEE 1588, CL91/CL74/CL108 FEC, CL92/CL85 training, 156.25 MHz reference, MDC/MDIO at 1.8 V/3.3 V, 1.0 V core,
  19 mm 324-ball BGA, footprint compatible with BCM82381. Siblings (NIST CAVP 3272): BCM82396, BCM82398, BCM82399.
- So 32 cages = **16 chips**: one MDIO address per 100G port, two per chip, two chips per bus (I).
- The brief does not state the MDIO clause, ID registers, straps or power-up mode. No full datasheet is public.

From code and the vendor binary:
- Chip ID register is not IEEE 1.2/1.3 in this family (V for siblings): Furia and Sesto dev 1 0x8a00/0x8a01,
  Madura and Dino 0x8b00/0x8b01. Evora: 0x8b00/0x8b01 = 0x2391/0x80a0 (verified live, [extphy.md](extphy.md)).
- Evora is a MAC-PHY, not a plain retimer (V, source paths in the vendor binary:
  `phymod/chip/evora/evora_pm/pcs_pmd/{falcon_evora,tscf_evora}`, `evora_pm/pms/pm4x25.c`, `portmod_chain.c`,
  `tier1/evora_cfg_seq.c`): Falcon SerDes + TSCF PCS + PM4x25 (CLMAC) per side. Hence the vendor's
  `CLMAC_TX_CTRL AVERAGE_IPG` setting.
- Firmware: RAM download over MDIO at every power-up (verified live: a reboot clears it, [sonic.md](sonic.md)).
  The library also has an SPI-EEPROM programming path (strings "EEPROM Download cannot be performed when serboot
  pin is high"); we never use it and the re-host refuses it. Whether the board fits SPI EEPROMs is unknown.
- It does not pass traffic unconfigured (I, not tested: no link was tried before the vendor init).
- Reset: none needed from a CPLD or GPIO on this board (verified: all 32 answer after power-on). The Galileo
  board gates its BCM82391s with an FPGA `PHY_RESET` register.

## Not verified
- OpenNSA binaries, Goldstone beyond one file, Edgecore's download portal (login).
- Two-addresses-per-chip layout (inferred from the brief and the bus layout; no chip marking read).

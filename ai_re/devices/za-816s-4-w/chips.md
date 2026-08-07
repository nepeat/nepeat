# Chip ID table — ZA-816S-4-W

Confidence: **high** = markings unambiguous / cross-checked; **med** = brand or
family known, exact part inferred; **low** = marking cryptic, best guess.

| Ref | Marking (as read) | Part / function | Conf | Photo |
|-----|-------------------|-----------------|------|-------|
| U2  | `Ingenic T31` `260902612B01-LC` `1512059015221?` | **Ingenic T31 (T31L "Lite")** — MIPS XBurst2 smart-camera SoC (ISP + H.264/265). `-LC` = low-cost/lite bin. | high | 8025, 8026 |
| U7  | `Hi3861 RNIV100` `ATFB1B1V8` `2035-CN 0309` | **HiSilicon Hi3861** Wi-Fi module — 2.4 GHz 802.11 b/g/n + RISC-V MCU. On its own green sub-PCB. | high | 8030 |
| U6  | `PUYA` `P25Q64SH` `2D1TC1D` | **PUYA P25Q64SH** — 64 Mbit (8 MB) SPI NOR flash, SOIC-8. **Firmware lives here.** | high | 8032 |
| U4  | `8002A` `CQ1Y1M.1N` | **8002A** mono audio power amp (class-AB, ~3 W), SOP-8. | high | 8031 |
| U5  | `8002A` `CQ1Y1M.1N` | Second **8002A** amp (near `SPK`). | high | 8025, 8026 |
| U8  | `BORN 2249` `BE2803LV-24` | Darlington/driver array, 24-pin SSOP — **ULN2803-family** style; likely stepper/relay/LED sink for pan-tilt or IR. | med | 8029 |
| T2/U? | `YX-TEK` `SMD1307G` `2307` (logo ЄІЗ) | **YX-TEK SMD1307** — motor driver (stepper/DC), near `MOTOR` connector. | med | 8028 |
| U?  | `AFD2` `11F1` | TSSOP-16 near power rails (`5V8`/`1V8`) — motor driver or codec. Marking too short to pin down. | low | 8027 |
| U1? | `5030` `7418` | QFN by inductors L1/L3 — **buck converter / PMIC** (generates 1V8/0V8/3V rails). | low | 8029 |
| U5_ (2nd board) | `JL...` `N032C NF` `070DH8GC` (tri-propeller logo) | **JieLi (Zhuhai Jieli 杰理)** BT/audio MCU on daughterboard. | med | 8034, 8035 |

## Notes on the uncertain ones

- **`AFD2 11F1`, `5030 7418`, `SMD1307G`, `BE2803LV-24`** — all cryptic house/
  short markings. Best path to confirm is tracing pins to the `MOTOR`, `IR-CUT`
  and power inductors, or reading the T31 SDK's board DTS/config once the flash
  is dumped (the driver bindings name the exact parts).
- The **JieLi** propeller logo is unmistakable; exact `N032C` variant (AC69xx /
  BR-series) needs the flash/firmware to confirm its job (chime? BLE pairing?).
- `40:6A:8E` OUI on the label MAC — worth an IEEE OUI lookup to name the ODM.

## How these were read

13 iPhone HEIC photos (`IMG_8024`–`IMG_8036`) converted to JPEG with `sips` and
read directly. Several were soft-focus; markings above are transcribed as best
legible — double-check against the physical parts under a loupe before ordering
replacements or trusting a low-confidence row.

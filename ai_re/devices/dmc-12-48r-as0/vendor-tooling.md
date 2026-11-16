# Factory hardware tooling vs `board-dmc12`

> Patch numbers in this file dated before 2026-11-15 21:00 are the pre-squash series; map: `../../nix/packages/uboot/README.md`.

From factory OS 1.0.526.0 (newest seen; the u01 SSD image) unless marked, with 457
differences. Live values are u01's (2026-11-15, `inventory/u01/`). "Module" means
`ai_re/nix/modules/boards/dmc12` as imported (first iteration).

## CPLD (Lattice MachXO3, I2C bus 4 / 0x68, raw, no Linux driver)

| reg | meaning | used by | u01 value |
| --- | --- | --- | --- |
| 0x01 | version (>134 = v7+, 64 PWM steps, else 16) | fancontrol, show | 0x88 |
| 0x11 | power: bit3 PSU1 present, bit4 PSU2, bit5 12 V DC input | show, interruptmon | 0x17 |
| 0x17 | status LED: 0x10 green, 0x20 green blink, 0x04 red, 0x01 yellow | setled | 0x20 |
| 0x18 | uplink PHY pair state (0x09/0x16/0x10/0x12 match; 0x11/0x0a/0x0e need a fix) | setphyspeed | 0x12 (1G copper, OK) |
| 0x20 | interrupt status, **active low**: b0/b1 TMP75 alerts, b2–b4 fan1–3, b5 uplink change, b6/b7 PSU1/2 | interruptmon | 0xbf (PSU1 alarm) |
| 0x21 | interrupt mask (1 = masked) | interruptmon | 0x00 |
| 0x28, 0x29 | board reset: write 0x29←1, then 0x28←1 | kernel `al-dmc12-reboot` | 0x00, 0xff |
| 0x2a | WDT flag (3 after a WDT reset; write 0 to clear, needed before fan writes) | U-Boot, fancontrol | 0x01 |
| 0x50 | fan PWM step (duty = step−1) | fancontrol, show | 0x2a |
| 0x60–0x62 | fan1–3 tach ×100 RPM | show | 0x57 each |

## Function by function

| function | factory | module | gap |
| --- | --- | --- | --- |
| **board reboot** | vendor kernel driver `amazon,al-dmc12-reboot` (`CONFIG_POWER_RESET_AL_DMC12`), bound at shutdown by `S99cpld_hook` (`new_device al-dmc12-reboot 0x68` on the mux-0x71 bus). It writes 0x29/0x28 and spins; otherwise it falls back to `alpine-reboot` via the **sp805** @0xfd88c000. Firmware PSCI is **v0.1, no SYSTEM_RESET**. | none | **Must add**: a restart handler (CPLD, or sp805 as a fallback). Check whether mainline `reboot` works at all on this firmware. |
| fan control | `fancontrol` PI loop over `sensors`, clears WDT flag; ≥526 floors at step 9 (AS0 alarms below ~2000 RPM) | ported (526 behaviour); alternative: fand via `modules/thermal/dmc12-fan.nix` (treats 0x2a bit 1 as the takeover flag, checks 0x01) | fand path untested on hardware |
| interrupts / alarms | `interruptmon`: gpiod line **gpiochip5 = PL061 @0xfd897000 line 1**, falling edge → read 0x20, mask in 0x21; also polls every 15 s; DN0 has no fan 3; DC input masks PSU bits | none | add: alarm logging + uplink re-sync. The DT needs the PL061 nodes, which the module's DTS lacks entirely |
| uplink PHY sync | `setphyspeed`: mux 0x70/0x71 ← 0x08, PHY @0x40 regs 0x16/0x09/0x00 by CPLD 0x18; re-run on interrupt b5 and every 15 s (≥526 only) | ported, same I2C ops, **boot-time only** | re-trigger on link change. Its raw mux writes fight the kernel's pca954x driver: do them via the mux channel bus, or with the mux idle state set |
| status LED | `setled` at end of boot | ported | none |
| power / INA219 | `show power`: mux 0x71←0x04, INA219 @0x44 config 0x9f01, reads shunt/bus V | ported (in `show`; it printed 0 until 2026-11-15: gawk parsed the hex as 0) | none; maybe an `ina219` DT node with `shunt-resistor = <1000>` (1 mΩ) instead of raw writes (`research-open-items.md` §5) |
| board EEPROM | `onieeeprom` → `/etc/eeprom.json` at sysinit (used by interruptmon model check, snmp) | ported (`/run/dmc12/eeprom.json`) | make interruptmon-equivalent read it from there |
| CPLD firmware update | `cpldupd`: MachXO3 (IDCODE 0x612BB043, LCMXO3LF-2100C) over its **I2C config port 4-0x10**; holds **GPIO 494 = SoC GPIO 22** (hitless-update hold) low meanwhile (`research-open-items.md` §2) | none | **do not port**; recovery tool only. Never write to 4-0x10 |
| FRU/MAC programming | `changemac`, `eepromer`, `fru_gen_bin` (526 only) | none | factory only; don't port |
| boot slots | U-Boot boots `/boot/primary`, then `/boot/secondary`, then `/`; `/boot/recoverymode` present ⇒ secondary once; init deletes the flag on every boot | own bootmenu | decide whether to keep a fallback slot |
| watchdogs | sp805 present, not armed by userspace; uC watchdog "enabled 10 s" yet never fired in 40 min idle | none | none needed today; record it if NixOS ever arms the sp805 |
| SoC temperature | `al-thermal` @0xfd860a00 (`AL_THERMAL_V2`) | none | optional: feed it to fan control or metrics |
| serial ports | udev P1–P48 = hub.port:iface (identical in 457/526 and the module); ser2net on 127.0.0.1:4001–4048 raw, 9600 8N1, P48 115200; users reach it via ssh → `bindserial` CLI | udev ported; no ser2net/CLI | our own choice (ser2net/conserver) |
| fleet glue | `setdhcpop61` (DHCP client-id = serial), snmp sysDescr from EEPROM, lldpd, `/etc` overlay on sda1 | none | don't port |

## Leftovers in the vendor DT (reference board, nothing behind them on u01)
NAND partitions (U-Boot: `NAND: 0 MiB`), `gpio-leds` "ETH 0/1 10G/25G" on SGPO,
and a second TPM node on SPI (`infineon,slb9670`). Nothing answers on I2C bus 3, so the 3-0020 TPM node is a reference-design leftover; the SPI SLB9670 on CS2 is the real one (the factory U-Boot extended its PCRs; `research-open-items.md` §6).

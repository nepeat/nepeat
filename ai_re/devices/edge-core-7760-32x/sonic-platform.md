# SONiC platform integration (pmon, LEDs, LLDP) — 2026-11-15

What `board-as7760` adds to `services.sonic` ([module](../../nix/modules/boards/as7760/sonic-platform.nix)):
the board's `sonic_platform` ([code](../../nix/modules/boards/as7760/sonic_platform/)), the pmon
daemons, the front-panel LED program at SDK init, LLDP. Generic parts:
[switch/sonic README](../../nix/modules/switch/sonic/README.md#platform-pmon-and-lldp).
**VM-tested only** (fake board, [evidence](../sonic-nixos/sonic-platform-vm-20261115.txt)); nothing here has run on
the switch yet.

## sources per object

| object | read from | writes |
| --- | --- | --- |
| 6 fan drawers x 2 rotors | hwmon `as7760_fan` (rpm, fault = drawer absent or rotor stopped); fand's `/run/fand/state.json` (expected rpm, duty) | **none**: `set_speed`/LEDs refuse; no thermal manager, so thermalctld runs no fan policy |
| thermals board_1..3, cpu_board | hwmon `lm75` at 0x48..0x4b | none (thresholds 45/50 C mirror fand's board zone) |
| thermal asic | `/run/asic/temperature` (reader or syncd) | none (95/100 C, fand's ASIC zone) |
| PSU 1/2 | CPLD#1 0x02 (bits from the AS7712/AS7716 siblings, **unverified here**) | none |
| 32 cages | CPLD#1 ModPrsL/ResetL ([qsfp-control.md](qsfp-control.md)); EEPROM via as7760-qsfp's bus map, combined I2C transfers | ResetL pulse (`sfputil reset`), SFF-8636 bytes 86-126 (TX disable, byte 93 low power), SFP A2h 110-127 |
| syseeprom | ONIE TlvInfo in NOR `hw-info` (/dev/mtdN, read-only) | none |

Direction of airflow: not reported (fan CPLD 0x10 polarity unknown). PSU
model/serial/voltage: N/A (no PMBus devices in the DT).

## LEDs

The public sonic-buildimage AS7712-32X `led_proc_init.soc` (line for line
the factory `portled.soc`, [vendor-blobs-private.txt](vendor-blobs-private.txt) §4) is
`SAI_OPENBCM_RC_FILE`: syncd runs it (`rcload`) right after `init all`.
It remaps the LEDUP0/1 port order, loads and starts both LED programs and
runs `led auto on`. That last command registers the diag shell's
`ledproc_linkscan_cb` (OpenBCM `src/appl/diag/ledproc.c`) with the SDK
linkscan, which then writes each port's link bit into LED data RAM
(0x80/0xa0 + physical port) on every link change; activity comes from the
hardware. So no SONiC `ledd` (the AS7712 sets `skip_ledd` too). While
syncd is stopped nothing updates the LEDs (they keep their last state).
`hardware.as7760.ledProgram = null` disables it.

## switch test (owner/parent; read-only except where marked)

Deploy, then on the switch (`SONIC_CLI_IFACE_MODE=alias` in login shells):

1. Units: `systemctl is-active sonic-xcvrd sonic-thermalctld sonic-psud sonic-syseepromd sonic-lldpd sonic-lldpmgrd sonic-lldp-syncd`
   -> 7x `active`; `systemctl show -p NRestarts --value <unit>` 0.
2. Fans untouched: `fand status | jq .outputs.fans.duty` before and 5 min
   after start: same curve value (53.3 at idle); `journalctl -u sonic-thermalctld | grep -i policy` empty.
3. `show platform fan`: 12 rows (Fan 1 front .. Fan 6 rear), Presence
   `Present`, Status `OK`, Speed ~55-70%, Target from fand.
4. `show platform temperature`: board_1..3, cpu_board ~28-32 C, asic ~40 C,
   High 45/95, Crit 50/100, Warning False.
5. `show platform syseeprom`: Product Name 7760-32X-A-AC-F, Serial
   776032X16521259, Base MAC 8C:EA:1B:89:C9:3C, `(checksum valid)`.
6. **PSU bits** (the unverified part): `C=$(basename $(readlink -f /sys/bus/i2c/devices/*-0076/channel-2 | head -1) | sed s/i2c-//)`
   (as in qsfp-control.md: the 0x76 under i2c@119000), `i2cget -y $C 0x60 0x02`.
   Both PSUs in and powered, expected `0x3c` in bits 0-5 (bits 0/1 low, 2-5 high; 6/7 unknown).
   `show platform psustatus`: PSU 1 and PSU 2 `OK`. Then, **only with the
   owner's OK**, unplug one PSU's AC cord: its power-good bits (PSU1 3+5 /
   PSU2 2+4) should clear and the row turn `NOT OK`; pull the PSU itself:
   bit 1 (PSU1) / bit 0 (PSU2) high, `NOT PRESENT`. Any other pattern: note
   it here and fix `BITS` in [psu.py](../../nix/modules/boards/as7760/sonic_platform/psu.py).
7. `show interfaces transceiver presence`: Ethernet1/1 and Ethernet32/1
   `Present` (the QSA + SFP28 DAC loop), others `Not present`;
   `show interfaces transceiver eeprom Ethernet1/1` = the DAC's vendor/part
   as `as7760-qsfp 1` prints them.
8. LEDs: front-panel LEDs of cages 1 and 32 lit (link up at 25G), others
   dark; `config interface shutdown Ethernet1/1` -> cage 1 LED off within
   ~1 s, `startup` -> on again. If all LEDs stay dark: `journalctl -u sonic-syncd | grep -i 'rc file\|led'`.
9. LLDP: `show lldp neighbors` -> on Ethernet0: SysName = the switch's
   hostname, PortID `local Ethernet32/1`; on Ethernet124: PortID
   `local Ethernet1/1`. `show lldp table`: 2 entries.
   `redis-cli -s /var/run/redis/redis.sock -n 0 hgetall LLDP_ENTRY_TABLE:Ethernet0` filled.
   (Both ends are PortChannel members in the LACP test config; LLDP runs on
   the member netdevs either way.)

Save the transcript as `devices/sonic-nixos/sonic-platform-hw-<date>.txt`.

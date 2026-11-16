#!/usr/bin/env python3
"""DMC-12 CPLD interrupt monitor: alarms to the journal + uplink PHY re-sync.

Port of the factory /usr/bin/interruptmon (OS 1.0.526.0). CPLD on I2C bus 4
at 0x68 (DT alias). Registers:
  0x20  interrupt status, active low: b0/b1 TMP75 alerts, b2-b4 fan 1-3
        failure, b5 uplink change, b6/b7 PSU1/PSU2 failure
  0x21  interrupt mask (1 = masked)
  0x11  power: b5 = 12 V DC input (then PSU alarms are meaningless)
The CPLD drives an active-low line, the PL061 GPIO named CPLD_INT_N in the
DT (factory: gpiochip5 line 1). On a falling edge and every --poll seconds
anyway (the factory "spurious interrupt"), handled alarms get masked until the
next config reload, link changes re-run `setphyspeed -q`.
An alarm is logged once when it appears and once when it clears, not on every reload
(a single-PSU unit's PSU1 alarm stays asserted forever: once a minute was journal churn).
"""
import argparse
import json
import subprocess
import sys
import time

import gpiod
from gpiod.line import Edge
from smbus2 import SMBus

CPLD = 0x68
REG_STATUS, REG_MASK, REG_POWER = 0x20, 0x21, 0x11
NAMES = {0: "TMP75 alert 0", 1: "TMP75 alert 1", 2: "fan 1 failure", 3: "fan 2 failure",
         4: "fan 3 failure", 6: "PSU1 failure", 7: "PSU2 failure"}
UPLINK = 5


def log(level, msg):
    # systemd reads <N> prefixes on stdout: 3 = err, 5 = notice, 7 = debug.
    print(f"<{level}>{msg}", flush=True)


def bits(value):
    return [(value >> i) & 1 for i in range(8)]


def product_suffix(path):
    try:
        with open(path) as f:
            return json.load(f)["ProductName"][-3:].upper()
    except (OSError, ValueError, KeyError) as e:
        log(3, f"cannot read hardware model from {path}: {e}")
        return None


def default_mask(bus, args):
    """Mask bits for disabled alarms, as the factory load_config() did."""
    dc_input = bits(bus.read_byte_data(CPLD, REG_POWER))[5]
    model = product_suffix(args.eeprom_json)
    temp = 0 if args.temperature else 1
    fans = 0 if args.fans else 1
    fan3 = 1 if model == "DN0" else fans  # DN0 chassis have no fan 3
    psu1 = 0 if args.psu1 and not dc_input else 1
    psu2 = 0 if args.psu2 and not dc_input else 1
    return [temp, temp, fans, fans, fan3, 0, psu1, psu2]


def to_byte(mask):
    return sum(b << i for i, b in enumerate(mask))


def write_mask(bus, mask, verbose=False):
    value = to_byte(mask)
    if bus.read_byte_data(CPLD, REG_MASK) != value:
        bus.write_byte_data(CPLD, REG_MASK, value)
        if verbose:   # the reload cycle flips handled alarms every minute: not worth a line
            log(7, f"interrupt mask 0x{value:02x}")


def uplink(args, quiet=True):
    cmd = [args.setphyspeed] + (["-q"] if quiet else [])
    try:
        subprocess.run(cmd, check=False)
    except OSError as e:
        log(3, f"setphyspeed failed: {e}")


def handle(bus, mask, args, reported):
    status = bits(bus.read_byte_data(CPLD, REG_STATUS))
    current = bits(bus.read_byte_data(CPLD, REG_MASK))
    for i in sorted(reported):
        # A masked alarm reads "not asserted": only an unmasked, deasserted bit is a clear.
        if not current[i] and status[i]:
            log(5, f"{NAMES[i]} cleared")
            reported.discard(i)
    for i in range(8):
        if status[i] | current[i]:
            continue  # not asserted (active low) or masked
        if i == UPLINK:
            uplink(args, quiet=False)  # never masked: every link change matters
        else:
            if i not in reported:
                log(3, f"{NAMES[i]}!")
                reported.add(i)
            mask[i] = 1  # quiet until the next reload, like the factory daemon


def find_line(name):
    import glob
    for path in sorted(glob.glob("/dev/gpiochip*")):
        try:
            with gpiod.Chip(path) as chip:
                offset = chip.line_offset_from_id(name)
                return path, offset
        except (OSError, ValueError):
            continue
    raise SystemExit(f"no GPIO line named {name}; is the board DTB loaded?")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--bus", type=int, default=4)
    p.add_argument("--line", default="CPLD_INT_N")
    p.add_argument("--poll", type=int, default=15, help="re-check interval, seconds")
    p.add_argument("--reload", type=int, default=60, help="unmask handled alarms after N seconds")
    p.add_argument("--setphyspeed", default="setphyspeed")
    p.add_argument("--eeprom-json", default="/run/dmc12/eeprom.json")
    for alarm in ("temperature", "fans", "psu1", "psu2"):
        p.add_argument(f"--no-{alarm}", dest=alarm, action="store_false")
    args = p.parse_args()

    uplink(args, quiet=False)
    chip_path, offset = find_line(args.line)
    with SMBus(args.bus) as bus:
        mask = default_mask(bus, args)
        write_mask(bus, mask, verbose=True)  # before the first read, to avoid false alarms
        reported = set()
        log(5, f"interrupt monitor started ({chip_path} line {offset})")
        settings = gpiod.LineSettings(edge_detection=Edge.FALLING)
        with gpiod.request_lines(chip_path, consumer="dmc12-interruptmon",
                                 config={offset: settings}) as request:
            last_poll = last_reload = time.monotonic()
            while True:
                if request.wait_edge_events(1.0):
                    request.read_edge_events()
                    handle(bus, mask, args, reported)
                    write_mask(bus, mask)
                now = time.monotonic()
                if now - last_poll >= args.poll:
                    last_poll = now
                    handle(bus, mask, args, reported)
                    write_mask(bus, mask)
                    uplink(args)
                if now - last_reload >= args.reload:
                    last_reload = now
                    mask = default_mask(bus, args)
                    write_mask(bus, mask)


if __name__ == "__main__":
    sys.exit(main())

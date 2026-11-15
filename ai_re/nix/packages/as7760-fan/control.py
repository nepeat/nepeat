#!/usr/bin/env python3
"""AS7760 fan duty with a temperature guard; exposes RPM/temperatures.

The monitor holds a fixed quiet duty (default 80%) while everything is
healthy and cool, and goes to 100% on any sensor/fan error or when a board
sensor reaches --hot; it returns to the quiet duty only after all sensors
have been at or below --cool for a minute. This is a guard, not a tuned
curve: the ASIC die temperature is not available, only four board sensors.
Stop the service before an intentional manual adjustment.
"""
import argparse
import json
import signal
import sys
import time
from pathlib import Path

HWMON = Path("/sys/class/hwmon")


def fan_device(root=HWMON):
    found = [p for p in root.glob("hwmon*")
             if (p / "name").read_text().strip() == "as7760_fan"]
    if len(found) != 1:
        raise RuntimeError(f"expected one AS7760 fan controller, found {len(found)}")
    return found[0]


def set_percent(percent, root=HWMON):
    if not 50 <= percent <= 100:
        raise ValueError("bring-up duty must be 50..100 percent")
    p = fan_device(root)
    value = (percent * 255 + 99) // 100
    (p / "pwm1").write_text(str(value) + "\n")
    actual = int((p / "pwm1").read_text())
    if actual < value or actual > 255:
        raise RuntimeError(f"PWM readback {actual} does not meet request {value}")
    return actual


def snapshot(root=HWMON):
    fan = fan_device(root)
    data = {"pwm": int((fan / "pwm1").read_text()), "rotors": [],
            "temperatures_mC": {}, "errors": []}
    for n in range(1, 13):
        data["rotors"].append({
            "label": (fan / f"fan{n}_label").read_text().strip(),
            "rpm": int((fan / f"fan{n}_input").read_text()),
            "fault": bool(int((fan / f"fan{n}_fault").read_text())),
        })
    for p in root.glob("hwmon*"):
        if (p / "name").read_text().strip() != "lm75":
            continue
        # Only our four DT-declared sensors behind the physical controller
        # at 0x119000, mux 0x76 channel 1. Bus numbers are dynamically assigned.
        node = (p / "device/of_node").resolve()
        if "i2c@119000/pca9548@76/i2c@1/temperature-sensor@" not in str(node):
            continue
        label = node.name
        try:
            temp = int((p / "temp1_input").read_text())
            if not -10000 <= temp <= 125000:
                raise ValueError("outside plausible range")
            data["temperatures_mC"][label] = temp
        except (OSError, ValueError) as exc:
            data["errors"].append(f"{label}: {exc}")
    if len(data["temperatures_mC"]) != 4:
        data["errors"].append("expected all four board temperature sensors")
    if any(x["fault"] or x["rpm"] < 500 for x in data["rotors"]):
        data["errors"].append("fan missing/stopped or RPM below 500")
    if any(t >= 60000 for t in data["temperatures_mC"].values()):
        data["errors"].append("board temperature at or above 60 C")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("status")
    setter = sub.add_parser("set")
    setter.add_argument("percent", type=int)
    mon = sub.add_parser("monitor")
    mon.add_argument("--duty", type=int, default=80, help="quiet duty in percent (50..100)")
    mon.add_argument("--hot", type=float, default=45.0, help="any board sensor at/above this (C): full speed")
    mon.add_argument("--cool", type=float, default=40.0, help="all sensors at/below this (C) for a minute: quiet again")
    args = parser.parse_args()
    if args.command == "status":
        print(json.dumps(snapshot(), indent=2))
    elif args.command == "set":
        print(json.dumps({"pwm": set_percent(args.percent)}))
    else:
        def stop(signum, frame):
            raise SystemExit(0)
        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)
        if not 50 <= args.duty <= 100 or args.cool >= args.hot:
            raise ValueError("need 50 <= duty <= 100 and cool < hot")
        try:
            counter = 0
            calm = 0          # consecutive healthy+cool readings
            target = 100      # start at full speed until proven calm
            set_percent(100)
            while True:
                data = snapshot()
                temps = data["temperatures_mC"].values()
                hot = any(t >= args.hot * 1000 for t in temps)
                cool = bool(temps) and all(t <= args.cool * 1000 for t in temps)
                if data["errors"] or hot:
                    calm = 0
                    new = 100
                else:
                    calm = calm + 1 if cool else 0
                    new = args.duty if calm >= 12 else target
                changed = new != target
                target = new
                set_percent(target)   # re-assert every cycle
                data["duty_percent"] = target
                if data["errors"] or changed or counter % 12 == 0:
                    print(json.dumps(data), flush=True)
                counter += 1
                time.sleep(5)
        finally:
            set_percent(100)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as exc:
        print(f"as7760-fan: {exc}", file=sys.stderr)
        sys.exit(1)

#!/usr/bin/env python3
"""Initial bring-up: hold full speed, expose RPM/temperatures and manual duty.

No automatic quiet curve: ASIC die temperature is not yet available. The
monitor resets duty to 100%; stop it before an intentional manual adjustment.
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
    sub.add_parser("monitor")
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
        try:
            counter = 0
            while True:
                set_percent(100)
                data = snapshot()
                if data["errors"] or counter % 12 == 0:
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

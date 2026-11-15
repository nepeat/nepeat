#!/usr/bin/env python3
"""Read and decode the front-panel module EEPROMs of the AS7760-32X.

  as7760-qsfp            table of all 32 ports
  as7760-qsfp 1 32       details for ports 1 and 32
  as7760-qsfp --json     everything, machine readable

Works for modules inserted after boot (talks to /dev/i2c-N directly).
QSFP/QSFP28 (SFF-8636) and SFP/SFP28 in an adapter (SFF-8472) are decoded,
the latter with digital diagnostics when the module implements them.
Other tools (as7760-metrics) import bus_for/read/static_fields/dynamic_fields.
"""
import argparse, fcntl, json, os, sys
from pathlib import Path

# _FORCE: the kernel's at24 driver binds 0x50 for modules present at boot;
# reading the EEPROM alongside it is harmless.
I2C_SLAVE = 0x0706
# Front-panel port -> (mux address on i2c@119100, mux channel). From the
# factory NOS's device table (its bus n = mux 0x72 + (n-12)//8, channel (n-12)%8).
FACTORY_BUS = {1: 16, 2: 17, 3: 18, 4: 19, 5: 21, 6: 20, 7: 23, 8: 22, 9: 12, 10: 13,
               11: 14, 12: 15, 13: 24, 14: 25, 15: 26, 16: 27, 17: 28, 18: 29, 19: 30,
               20: 31, 21: 40, 22: 41, 23: 42, 24: 43, 25: 32, 26: 33, 27: 34, 28: 35,
               29: 36, 30: 37, 31: 38, 32: 39}
IDS = {0x03: "SFP/SFP+/SFP28", 0x0c: "QSFP", 0x0d: "QSFP+", 0x11: "QSFP28", 0x18: "QSFP-DD"}
EXT = {0x00: "", 0x01: "100G AOC/25G AOC (FEC)", 0x02: "100GBASE-SR4/25GBASE-SR", 0x03: "100GBASE-LR4/25GBASE-LR",
       0x04: "100GBASE-ER4/25GBASE-ER", 0x06: "100G CWDM4", 0x07: "100G PSM4", 0x08: "100G ACC/25G ACC (FEC)",
       0x0b: "100GBASE-CR4/25GBASE-CR CA-L", 0x0c: "25GBASE-CR CA-S", 0x0d: "25GBASE-CR CA-N",
       0x10: "40GBASE-ER4", 0x11: "4x10GBASE-SR", 0x18: "100G AOC/25G AOC", 0x19: "100G ACC/25G ACC",
       0x1a: "100G-FR/DR", 0x25: "100GBASE-DR", 0x26: "100G-FR", 0x27: "100G-LR"}


def bus_for(port):
    n = FACTORY_BUS[port] - 12
    mux, chan = 0x72 + n // 8, n % 8
    parents = [p for p in Path("/sys/bus/i2c/devices").glob("i2c-*")
               if "i2c@119100" in os.path.realpath(p / "of_node")
               and not (p / "mux_device").exists()]
    if len(parents) != 1:
        raise RuntimeError("cannot find the i2c@119100 adapter")
    link = Path(f"/sys/bus/i2c/devices/{parents[0].name[4:]}-00{mux:02x}/channel-{chan}")
    return int(os.path.realpath(link).rsplit("-", 1)[1])


def read(bus, addr, offset, length):
    fd = os.open(f"/dev/i2c-{bus}", os.O_RDWR)
    try:
        fcntl.ioctl(fd, I2C_SLAVE, addr)
        os.write(fd, bytes([offset]))
        return os.read(fd, length)
    finally:
        os.close(fd)


def text(b):
    return b.decode("ascii", "replace").strip(" \x00")


def select_page0(bus):
    """SFF-8636: select upper page 0 (harmless on flat-memory modules)."""
    fd = os.open(f"/dev/i2c-{bus}", os.O_RDWR)
    try:
        fcntl.ioctl(fd, I2C_SLAVE, 0x50); os.write(fd, bytes([127, 0]))
    finally:
        os.close(fd)


def static_fields(bus, lo):
    """Identity fields that do not change while a module stays plugged in.

    lo: the first bytes (at least 96) of the lower page / A0h. Keys starting
    with "_" are decode hints for dynamic_fields(), not printed or exported.
    """
    ident = lo[0]
    if ident in (0x0c, 0x0d, 0x11):
        select_page0(bus)
        up = read(bus, 0x50, 128, 128)
        passive = (up[19] >> 4) in (0xa, 0xb)
        return dict(vendor=text(up[20:36]), part=text(up[40:56]), rev=text(up[56:58]),
                    serial=text(up[68:84]), date=text(up[84:92]),
                    compliance=EXT.get(up[64], f"ext {up[64]:#04x}") or f"spec {up[3]:#04x}",
                    copper_m=up[18] or None, breakout=bool(up[113] & 0x70) or None,
                    media="passive copper" if passive else "active/optical",
                    _passive=passive, _tx_power=bool(up[92] & 0x04))
    if ident == 0x03:
        if len(lo) < 128:
            lo = read(bus, 0x50, 0, 128)
        passive = bool(lo[8] & 0x04)
        # SFF-8472 byte 92: bit 6 DDM implemented, bit 5 internally
        # calibrated, bit 2 needs an address-change sequence (not supported).
        ddm = (lo[92] & 0x64) == 0x60 and not passive
        return dict(vendor=text(lo[20:36]), part=text(lo[40:56]), rev=text(lo[56:60]),
                    serial=text(lo[68:84]), date=text(lo[84:92]),
                    compliance=EXT.get(lo[36], f"ext {lo[36]:#04x}") or f"10G {lo[3]:#04x}",
                    copper_m=lo[18] or None,
                    media="passive copper" if passive else ("active copper" if lo[8] & 0x08 else "optical"),
                    note="SFP-type module in a QSFP cage (adapter): lane 1 only",
                    _passive=passive, _ddm=ddm)
    return {}


def dynamic_fields(bus, lo, static):
    """Monitor values (temperature, supply, per-lane power/bias).

    lo: the lower page / A0h, at least its first 58 bytes, read just now.
    """
    ident = lo[0]
    info = {}
    if ident in (0x0c, 0x0d, 0x11):
        info.update(temp_c=round(int.from_bytes(lo[22:24], "big", signed=True) / 256, 1),
                    vcc_v=round(int.from_bytes(lo[26:28], "big") / 10000, 2))
        if not static.get("_passive"):
            info["rx_power_mw"] = [round(int.from_bytes(lo[34 + 2 * i:36 + 2 * i], "big") / 10000, 3) for i in range(4)]
            info["tx_bias_ma"] = [round(int.from_bytes(lo[42 + 2 * i:44 + 2 * i], "big") / 500, 2) for i in range(4)]
            if static.get("_tx_power"):
                info["tx_power_mw"] = [round(int.from_bytes(lo[50 + 2 * i:52 + 2 * i], "big") / 10000, 3) for i in range(4)]
    elif ident == 0x03 and static.get("_ddm"):
        try:
            a2 = read(bus, 0x51, 96, 10)
        except OSError:
            return info
        info.update(temp_c=round(int.from_bytes(a2[0:2], "big", signed=True) / 256, 1),
                    vcc_v=round(int.from_bytes(a2[2:4], "big") / 10000, 2),
                    tx_bias_ma=[round(int.from_bytes(a2[4:6], "big") / 500, 2)],
                    tx_power_mw=[round(int.from_bytes(a2[6:8], "big") / 10000, 3)],
                    rx_power_mw=[round(int.from_bytes(a2[8:10], "big") / 10000, 3)])
    return info


def public(fields):
    return {k: v for k, v in fields.items() if not k.startswith("_")}


def decode(port):
    info = {"port": port, "present": False}
    try:
        bus = bus_for(port)
        info["bus"] = bus
        lo = read(bus, 0x50, 0, 128)
    except OSError:
        return info                      # no ACK: nothing plugged in
    ident = lo[0]
    info.update(present=True, id=ident, type=IDS.get(ident, f"unknown {ident:#04x}"))
    static = static_fields(bus, lo)
    info.update(public(static))
    info.update(dynamic_fields(bus, lo, static))
    return info


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ports", nargs="*", type=int)
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    ports = a.ports or sorted(FACTORY_BUS)
    if any(p not in FACTORY_BUS for p in ports):
        ap.error("ports are 1..32")
    data = [decode(p) for p in ports]
    if a.json:
        print(json.dumps(data, indent=2)); return
    if a.ports:
        for d in data:
            print(f"Port {d['port']}:" + ("" if d["present"] else " empty"))
            for k, v in d.items():
                if k not in ("port", "present") and v is not None:
                    print(f"  {k:12} {v:.2f}" if isinstance(v, float) else f"  {k:12} {v}")
        return
    print(f"{'port':>4}  {'type':14} {'vendor':16} {'part':18} {'what':30} {'len':>4}  temp")
    for d in data:
        if not d["present"]:
            continue
        print(f"{d['port']:>4}  {d['type']:14} {d.get('vendor',''):16} {d.get('part',''):18} "
              f"{d.get('compliance',''):30} {str(d.get('copper_m') or '-') + 'm':>4}  "
              + (f"{d['temp_c']:.1f}" if 'temp_c' in d else "-"))
    print(f"{sum(d['present'] for d in data)} of {len(data)} ports populated")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError) as e:
        print(f"as7760-qsfp: {e}", file=sys.stderr); sys.exit(1)

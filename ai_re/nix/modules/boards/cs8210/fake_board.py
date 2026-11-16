#!/usr/bin/env python3
"""Build a fake CS8210-32X for sonic_platform (hw.py's CS8210_PLATFORM_ROOT).

  fake_board.py <root>

  devicetree + sysfs   I2C adapters, the 13 LM75 hwmons and the switch-board
                       EEPROM, each with its DT node (of_node), plus a decoy
                       0x56 EEPROM (the I2C-FPGA card's FRU) on another channel
  fand                 /run/fand/state.json: the 13 board sensors + "asic",
                       outputs top1..7 / bottom1..7 with front/rear rotors
  ASIC file            /run/asic/temperature (what syncd's publisher writes)
  I2C                  i2c/sys_cpld.bin with 0x08 = 0x45 and 0xa8 = 0x00 (the
                       live values of 2026-11-15), fan CPLDs (v3, PWM 127)
  I2C FPGA             PCI 0002:01:00.0 (1d0f:8571, decode on) and cages:
                         1  QSFP-DD CMIS 5.0, paged (400G-DR4 optic)
                        16  QSFP-DD CMIS, flat memory (passive DAC)
                        17  QSFP28 SFF-8636, channel left at power-up values
                        20  SFP+ DAC in a QSA (A0h + A2h)
                        30  QSFP-DD present but held in reset
The syseeprom TLV carries the identity fields of devices/cs8210-32x/PROGRESS.md
(their TLV codes are our reconstruction, not a dump). Module EEPROMs are
synthetic (vendor FAKE-VENDOR). Used by sonic_platform_test.py.
"""
import json
import os
import struct
import sys
import zlib

DT = "/sys/firmware/devicetree/base/soc/io-bus@fc000000/i2c@1880000/i2c-mux@70/i2c@2/i2c-mux@74"


def put(root, rel, data):
    p = os.path.join(root, rel.lstrip("/"))
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb" if isinstance(data, (bytes, bytearray)) else "w") as f:
        f.write(data)


def link(root, rel_link, rel_target):
    t = os.path.join(root, rel_target.lstrip("/"))
    os.makedirs(t, exist_ok=True)
    lp = os.path.join(root, rel_link.lstrip("/"))
    os.makedirs(os.path.dirname(lp), exist_ok=True)
    os.symlink(t, lp)


def root_port_config():
    """4 KiB config space of the I2C FPGA's root port (0002:00:00.0, live
    2026-11-16): capability list -> PM at 0x40 -> MSI 0x50 -> PCIe at 0x70
    (DevSta 0x7a = 0, LnkCap 0x7c DLLLA reporting, LnkSta 0x82 = 0x3012,
    data link active); AER at 0x100 (UESta 0x104 = 0)."""
    c = bytearray(4096)
    c[0:4] = struct.pack("<HH", 0x1c36, 0x0031)
    c[6] = 0x10                                              # status: capability list
    c[0x34] = 0x40
    c[0x40:0x42] = bytes([0x01, 0x50])
    c[0x50:0x52] = bytes([0x05, 0x70])
    c[0x70:0x72] = bytes([0x10, 0x00])
    c[0x7c:0x80] = struct.pack("<I", 0x00736c12)
    c[0x82:0x84] = struct.pack("<H", 0x3012)
    c[0x100:0x104] = struct.pack("<I", 0x0001 | 2 << 16 | 0x148 << 20)
    return bytes(c)


def tlv():
    fields = [(0x21, b"CS8210-32X-UN-00"), (0x22, b"R3181-F9003-01"), (0x23, b"R3181-G0001-02TH0321110078"),
              (0x24, bytes.fromhex("0c48c67a55ce")), (0x25, b"04/28/2021 00:00:00"), (0x26, b"\x02"),
              (0x27, b"R0B-2"), (0x28, b"arm64-celestica_cs8210-r0"), (0x2a, b"\x00\x04"),
              (0x2b, b"Celestica"), (0x2c, b"TH"), (0x2d, b"Celestica")]
    body = b"".join(bytes([c, len(v)]) + v for c, v in fields)
    data = b"TlvInfo\x00\x01" + struct.pack(">H", len(body) + 6) + body + b"\xfe\x04"
    return data + struct.pack(">I", zlib.crc32(data) & 0xffffffff)


def text(s, n):
    return s.encode().ljust(n, b" ")


def cmis(flat, model, serial, media_type, media_id, lanes, connector):
    """Lower page + upper page 0 (+ pages 1..0x11 when paged), optoe layout."""
    img = bytearray(256 if flat else 128 + 128 * 0x12)
    img[0], img[1], img[2], img[3] = 0x18, 0x50, 0x80 if flat else 0x00, 0x06   # QSFP-DD, CMIS 5.0, ModuleReady
    img[14:16] = struct.pack(">h", 35 * 256 + 64)            # 35.25 C
    img[16:18] = struct.pack(">H", 33000)                     # 3.3 V
    img[39:41] = bytes([1, 2])                                # active firmware 1.2
    img[85] = media_type
    img[86:90] = bytes([0x11, media_id, lanes, 0x01])         # app 1: 400GAUI-8 C2M -> media
    img[90] = 0xff                                            # end of the application list
    up = 128
    img[up] = 0x18
    img[up + 1:up + 17] = text("FAKE-VENDOR", 16)
    img[up + 17:up + 20] = bytes([0x00, 0x11, 0x22])
    img[up + 20:up + 36] = text(model, 16)
    img[up + 36:up + 38] = text("A1", 2)
    img[up + 38:up + 54] = text(serial, 16)
    img[up + 54:up + 62] = text("26111500", 8)
    img[up + 72] = 0x3f if flat else 0x10                     # power class / max power
    img[up + 74] = 0x01 if flat else 0x00                     # cable length (DAC 1 m)
    img[up + 75] = connector
    if not flat:
        p1 = 128 + 128 * 1
        img[p1 + 2:p1 + 4] = bytes([1, 0])                    # hardware revision 1.0
    return bytes(img)


def qsfp28_sff8636():
    img = bytearray(256)
    img[0], img[1], img[2] = 0x11, 0x07, 0x04                # QSFP28, rev, flat memory (DAC)
    img[22:24] = struct.pack(">h", 0)
    up = 128
    img[up + 0], img[up + 2] = 0x11, 0x23                     # id, no separable connector
    img[up + 3] = 0x80                                        # extended compliance (byte 192)
    img[up + 20:up + 36] = text("FAKE-VENDOR", 16)
    img[up + 40:up + 56] = text("QSFP-4SFP25-CU3M", 16)
    img[up + 56:up + 58] = text("05", 2)
    img[up + 63] = sum(img[up:up + 63]) & 0xff
    img[up + 64] = 0x0b                                       # 100GBASE-CR4 / 25GBASE-CR CA-L
    img[up + 68:up + 84] = text("FAKESN0017", 16)
    img[up + 84:up + 92] = text("260627", 8)
    img[up + 95] = sum(img[up + 64:up + 95]) & 0xff
    return bytes(img)


def sfp_dac():
    a0 = bytearray(512)
    a0[0], a0[1], a0[2] = 0x03, 0x04, 0x21                   # SFP, ext id, copper pigtail
    a0[8] = 0x04                                             # passive cable
    a0[12] = 0x67                                            # 10.3 Gb/s
    a0[18] = 3                                               # 3 m
    a0[20:36] = text("FAKE-VENDOR", 16)
    a0[40:56] = text("SFP-H10GB-CU3M", 16)
    a0[56:60] = text("A", 4)
    a0[63] = sum(a0[0:63]) & 0xff
    a0[68:84] = text("FAKESN0020", 16)
    a0[84:92] = text("260207", 8)
    a0[95] = sum(a0[64:95]) & 0xff
    a0[256:512] = b"\xff" * 256                              # A2h: ACKs, all 0xff (no DDM)
    return bytes(a0)


LM75 = [  # name, DT node below 0x74, milli-C, fand value
    ("i2cfpga_card", "i2c@0/i2c-mux@71/i2c@0/temperature-sensor@48", 36000),
    ("switchboard_left", "i2c@0/i2c-mux@71/i2c@1/temperature-sensor@48", 33500),
    ("switchboard_center", "i2c@0/i2c-mux@71/i2c@1/temperature-sensor@4b", 34000),
    ("switchboard_right", "i2c@0/i2c-mux@72/i2c@7/temperature-sensor@49", 32500),
    ("macsec_left", "i2c@1/i2c-mux@73/i2c@5/temperature-sensor@49", 38500),
    ("macsec_center", "i2c@1/i2c-mux@73/i2c@5/temperature-sensor@4a", 39000),
    ("macsec_right", "i2c@1/i2c-mux@73/i2c@5/temperature-sensor@4c", 38000),
    ("top_fanboard_right", "i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4d", 29000),
    ("top_fanboard_center", "i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4e", 29500),
    ("top_fanboard_left", "i2c@2/i2c-mux@73/i2c@0/temperature-sensor@4f", 29000),
    ("bottom_fanboard_right", "i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4d", 28000),
    ("bottom_fanboard_center", "i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4e", 28500),
    ("bottom_fanboard_left", "i2c@3/i2c-mux@73/i2c@0/temperature-sensor@4f", 28000),
]
ADAPTERS = {9: "i2c@0/i2c-mux@71/i2c@2", 31: "i2c@2", 40: "i2c@3", 15: "i2c@0/i2c-mux@71/i2c@7",
            8: "i2c@0/i2c-mux@71/i2c@0"}


def fand_state():
    sensors = {name: {"value": round(m / 1000.0 + 0.5, 1), "error": None, "required": True,
                      "path": "fake", "age": 1.0, "in_grace": False} for name, _, m in LM75}
    sensors["asic"] = {"value": 51.3, "error": None, "required": True, "path": None, "age": 2.0, "in_grace": False}
    outputs = {}
    for board in ("top", "bottom"):
        for n in range(1, 8):
            rotors = []
            for label, rpm, mx in (("front", 11200, 19000), ("rear", 12300, 21000)):
                rotors.append({"input": "0x%02x" % (16 + 16 * n + (label == "rear")), "label": label,
                               "rpm": float(rpm), "expected": round(mx * 0.6), "max_rpm": mx,
                               "percent_of_max": round(rpm * 100 / mx, 1), "calibrated": False,
                               "fault": False, "error": None})
            outputs["%s%d" % (board, n)] = {"duty": 49.8, "demand": 49.8, "hardware_min": 49.8, "clamped": True,
                                            "raw": 127, "observed_raw": 127, "path": None, "faults": [],
                                            "warnings": [], "rotors": rotors}
    outputs["cpld_force"] = {"duty": 49.8, "rotors": [], "faults": [], "warnings": []}
    return {"version": "fake", "uptime": 100.0, "dry_run": False, "ticks": 20, "failsafe": False,
            "zones": {}, "sensors": sensors, "outputs": outputs}


def build(root):
    # I2C adapters (mux channels) with their DT nodes
    for bus, node in ADAPTERS.items():
        link(root, "/sys/bus/i2c/devices/i2c-%d/of_node" % bus, DT + "/" + node)
    # board LM75s
    for i, (name, node, milli) in enumerate(LM75):
        dev = "/sys/devices/fake/i2c/%d-00%s" % (50 + i, node[-2:])
        link(root, dev + "/of_node", DT + "/" + node)
        h = "/sys/class/hwmon/hwmon%d" % i
        put(root, h + "/name", "lm75\n")
        put(root, h + "/temp1_input", "%d\n" % milli)
        os.symlink(os.path.join(root, dev.lstrip("/")), os.path.join(root, h.lstrip("/"), "device"))
    # switch-board TlvInfo EEPROM (at24, 0x71 ch7) and the decoy on ch0
    link(root, "/sys/bus/i2c/devices/15-0056/of_node", DT + "/i2c@0/i2c-mux@71/i2c@7/eeprom@56")
    put(root, "/sys/bus/i2c/devices/15-0056/eeprom", tlv().ljust(8192, b"\xff"))
    link(root, "/sys/bus/i2c/devices/8-0056/of_node", DT + "/i2c@0/i2c-mux@71/i2c@0/eeprom@56")
    put(root, "/sys/bus/i2c/devices/8-0056/eeprom", b"\x01" + b"\x00" * 8191)
    # fand, ASIC file
    put(root, "/run/fand/state.json", json.dumps(fand_state(), indent=1))
    put(root, "/run/asic/temperature", "52.0\n")
    # CPLDs
    sysc = bytearray(b"\xff" * 256)
    sysc[0x00], sysc[0x01], sysc[0x05], sysc[0x08], sysc[0x09], sysc[0xa8] = 0x15, 0x10, 0x40, 0x45, 0x05, 0x00
    put(root, "/i2c/sys_cpld.bin", bytes(sysc))
    for board, front, rear in (("top", 112, 123), ("bottom", 110, 121)):
        r = bytearray(256)
        r[0x00], r[0x0c] = 0x03, 0x18
        for n in range(1, 8):
            b = 0x10 + 0x10 * n
            r[b], r[b + 1], r[b + 2] = front, rear, 127
        put(root, "/i2c/fan_%s.bin" % board, bytes(r))
    # I2C FPGA on PCI, behind its root port (as in sysfs: the bus entry is a
    # link into the device tree, the parent directory is the root port)
    rp = "/sys/devices/platform/soc/fb700000.pcie/pci0002:00/0002:00:00.0"
    pci = rp + "/0002:01:00.0"
    put(root, rp + "/config", root_port_config())
    put(root, rp + "/vendor", "0x1c36\n")
    put(root, rp + "/device", "0x0031\n")
    link(root, "/sys/bus/pci/devices/0002:00:00.0", rp)
    link(root, "/sys/bus/pci/devices/0002:01:00.0", pci)
    cfg = bytearray(64)
    cfg[0:4] = struct.pack("<HH", 0x1d0f, 0x8571)
    cfg[4] = 0x06                                            # memory decode + bus master on
    put(root, pci + "/config", bytes(cfg))
    put(root, pci + "/vendor", "0x1d0f\n")
    put(root, pci + "/device", "0x8571\n")
    put(root, pci + "/enable", "1\n")
    put(root, pci + "/resource", "0x000000fe00000000 0x000000fe0000ffff 0x0000000000040200\n")
    put(root, pci + "/resource0", b"")
    os.makedirs(os.path.join(root, "run/lock"), exist_ok=True)
    put(root, "/fpga/port1.bin", cmis(False, "FAKE-QDD-DR4", "FAKESN0001", 0x02, 0x1c, 0x84, 0x0c))
    put(root, "/fpga/port16.bin", cmis(True, "FAKE-QDD-DAC1M", "FAKESN0016", 0x03, 0x01, 0x88, 0x23))
    put(root, "/fpga/port17.bin", qsfp28_sff8636())
    put(root, "/fpga/port20.bin", sfp_dac())
    put(root, "/fpga/port30.bin", cmis(True, "FAKE-QDD-INRESET", "FAKESN0030", 0x03, 0x01, 0x88, 0x23))
    put(root, "/fpga/sideband.json", json.dumps({"reset": 1 << 29, "lpmode": 0, "int": 0, "uninit": [17]}))


if __name__ == "__main__":
    build(sys.argv[1])

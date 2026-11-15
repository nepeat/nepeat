#!/usr/bin/env python3
"""Build a fake AS7760-32X for sonic_platform (hw.py's AS7760_PLATFORM_ROOT).

  fake_board.py <root>      hwmon (fans, LM75s), fand state, ASIC temperature,
                            hw-info MTD with an ONIE TlvInfo, CPLD#1 and two
                            module EEPROMs (port 1: SFP28 DAC in a QSA, as in
                            the lab loop; port 32: paged QSFP28 SR4 optic)

The TLV carries this board's identity fields (dumps/mtd1.bin, HISTORY.md
2026-11-14). Module EEPROMs are synthetic (vendor FAKE-VENDOR).
Used by sonic_platform_test.py and the VM test (switch/sonic/tests/vm-platform.nix).
"""
import json
import os
import struct
import sys
import zlib


def put(root, rel, data):
    p = os.path.join(root, rel.lstrip("/"))
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb" if isinstance(data, bytes) else "w") as f:
        f.write(data)


def tlv():
    fields = [(0x21, b"7760-32X-A-AC-F"), (0x22, b"FP3AO7632004A"), (0x23, b"776032X16521259"),
              (0x24, bytes.fromhex("8cea1b89c93c")), (0x25, b"11/19/2018 16:34:47"), (0x26, b"\x0b"),
              (0x27, b"R01I"), (0x28, b"powerpc-accton_as7760_32x-r0"), (0x29, b"2015.02.00.30"),
              (0x2a, b"\x00\x04"), (0x2b, b"Accton"), (0x2c, b"TW"), (0x2d, b"Edgecore"),
              (0x2e, b"0.2.2.9")]
    body = b"".join(bytes([c, len(v)]) + v for c, v in fields)
    total = len(body) + 6                                    # + CRC TLV
    data = b"TlvInfo\x00\x01" + struct.pack(">H", total) + body + b"\xfe\x04"
    return data + struct.pack(">I", zlib.crc32(data) & 0xffffffff)


def text(s, n):
    return s.encode().ljust(n, b" ")


def sfp28_dac():
    a0 = bytearray(256)
    a0[0], a0[1], a0[2] = 0x03, 0x04, 0x21                   # SFP, ext id, copper pigtail
    a0[8] = 0x04                                             # passive cable
    a0[12] = 0xff                                            # nominal rate: see byte 66
    a0[18] = 1                                               # 1 m
    a0[20:36] = text("FAKE-VENDOR", 16)
    a0[36] = 0x0c                                            # 25GBASE-CR CA-S
    a0[40:56] = text("FAKE-SFP28-DAC", 16)
    a0[56:60] = text("A", 4)
    a0[63] = sum(a0[0:63]) & 0xff
    a0[66] = 0x67                                            # 25.75 Gb/s / 250 Mb/s
    a0[68:84] = text("FAKESN0001", 16)
    a0[84:92] = text("261115", 8)
    a0[92] = 0x00                                            # no digital diagnostics
    a0[95] = sum(a0[64:95]) & 0xff
    return bytes(a0)


def qsfp28_sr4():
    img = bytearray(256 + 3 * 128)                           # lower, page 0, pages 1-3
    lo = img
    lo[0], lo[1], lo[2] = 0x11, 0x07, 0x00                   # QSFP28, rev, paged memory
    lo[22:24] = struct.pack(">h", 30 * 256 + 128)            # 30.5 C
    lo[26:28] = struct.pack(">H", 33000)                     # 3.3 V
    for i in range(4):
        lo[34 + 2 * i:36 + 2 * i] = struct.pack(">H", 8000)  # rx 0.8 mW
        lo[42 + 2 * i:44 + 2 * i] = struct.pack(">H", 3500)  # bias 7 mA
        lo[50 + 2 * i:52 + 2 * i] = struct.pack(">H", 7000)  # tx 0.7 mW
    up = 128
    img[up + 0], img[up + 1], img[up + 2] = 0x11, 0x00, 0x0c  # id, ext id, MPO
    img[up + 3] = 0x80                                       # extended compliance (byte 192)
    img[up + 12] = 0xff
    img[up + 20:up + 36] = text("FAKE-VENDOR", 16)
    img[up + 40:up + 56] = text("FAKE-QSFP28-SR4", 16)
    img[up + 56:up + 58] = text("01", 2)
    img[up + 58:up + 60] = struct.pack(">H", 850 * 20)
    img[up + 63] = sum(img[up:up + 63]) & 0xff
    img[up + 64] = 0x02                                      # 100GBASE-SR4
    img[up + 68:up + 84] = text("FAKESN0032", 16)
    img[up + 84:up + 92] = text("261115", 8)
    img[up + 92] = 0x0c                                      # avg rx power, tx power
    img[up + 95] = sum(img[up + 64:up + 95]) & 0xff
    p3 = 256 + 2 * 128                                       # page 3, bytes 128-255
    img[p3:p3 + 8] = struct.pack(">hhhh", 75 * 256, -5 * 256, 70 * 256, 0)       # temp hi/lo alarm/warn
    img[p3 + 16:p3 + 24] = struct.pack(">HHHH", 36300, 29700, 34650, 31350)       # vcc
    img[p3 + 48:p3 + 56] = struct.pack(">HHHH", 34673, 501, 27542, 1000)         # rx power
    img[p3 + 56:p3 + 64] = struct.pack(">HHHH", 7500, 1000, 7000, 1500)          # bias
    img[p3 + 64:p3 + 72] = struct.pack(">HHHH", 34673, 501, 27542, 1000)         # tx power
    return bytes(img)


def build(root):
    fan = "/sys/class/hwmon/hwmon0"
    put(root, fan + "/name", "as7760_fan\n")
    put(root, fan + "/pwm1", "136\n")
    rotors = []
    for ch in range(1, 13):
        side = "front" if ch % 2 else "rear"
        label, rpm = "fan%d_%s" % ((ch + 1) // 2, side), 12400 if ch % 2 else 10600
        put(root, "%s/fan%d_input" % (fan, ch), "%d\n" % rpm)
        put(root, "%s/fan%d_fault" % (fan, ch), "0\n")
        put(root, "%s/fan%d_label" % (fan, ch), label + "\n")
        mx = 21000 if side == "front" else 18000
        rotors.append({"input": "fan%d_input" % ch, "label": label, "rpm": rpm, "expected": round(mx * 0.6),
                       "max_rpm": mx, "percent_of_max": round(rpm * 100 / mx, 1), "calibrated": False,
                       "fault": False, "error": None})
    state = {"version": "fake", "failsafe": False, "zones": {},
             "outputs": {"fans": {"duty": 53.3, "raw": 136, "rotors": rotors}}}
    put(root, "/run/fand/state.json", json.dumps(state, indent=1))
    for i, (addr, milli) in enumerate([(0x48, 30500), (0x49, 29000), (0x4a, 28500), (0x4b, 31500)]):
        dev = "/sys/devices/i2c-9/9-%04x" % addr
        os.makedirs(os.path.join(root, dev.lstrip("/")), exist_ok=True)
        h = "/sys/class/hwmon/hwmon%d" % (i + 1)
        put(root, h + "/name", "lm75\n")
        put(root, h + "/temp1_input", "%d\n" % milli)
        os.symlink(os.path.join(root, dev.lstrip("/")), os.path.join(root, h.lstrip("/"), "device"))
    put(root, "/run/asic/temperature", "41.5\n")
    put(root, "/sys/class/mtd/mtd1/name", "hw-info\n")
    put(root, "/sys/class/mtd/mtd1ro/name", "hw-info\n")
    put(root, "/dev/mtd1", tlv().ljust(4096, b"\xff"))
    cpld = bytearray(b"\xff" * 256)
    cpld[0x00], cpld[0x01] = 0x03 << 2, 0x09
    cpld[0x02] = 0xe9        # PSU1 present (bit 1 low) + power good (bits 3, 5); PSU2 absent (bit 0 high)
    put(root, "/i2c/cpld1.bin", bytes(cpld))
    put(root, "/i2c/port1.bin", sfp28_dac())
    put(root, "/i2c/port32.bin", qsfp28_sr4())


if __name__ == "__main__":
    build(sys.argv[1])

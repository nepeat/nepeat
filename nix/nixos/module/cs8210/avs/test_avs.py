"""Unit tests for cs8210-asic-avs (stdlib only): python3 test_avs.py <path to cs8210-asic-avs>.

Vectors: this unit's fuse words as the vendor U-Boot prints them
(serial-20261115-boot-7.3.4.txt: vid[0..3], "RoV = 0", "LotID = PS7R09",
"Target VDD Core -> 800 mV", "AVS value -> 0x0333"), and synthetic ones
built by the inverse of the vendor bit loop for every RoV / lot branch.
"""
import importlib.machinery
import importlib.util
import random
import sys
import unittest

path = sys.argv.pop(1) if len(sys.argv) > 1 else "cs8210-asic-avs.py"
loader = importlib.machinery.SourceFileLoader("avs", path)
spec = importlib.util.spec_from_loader("avs", loader)
avs = importlib.util.module_from_spec(spec)
loader.exec_module(avs)

UNIT = [0x64BDB285, 0x10602A70, 0x00200644, 0x10200000]


def encode(lot, rov, base=UNIT):
    """Inverse of the vendor loop: put lot (6 x 7 bits) and RoV into base."""
    w = list(base)
    for i in range(42):
        bit = (lot[i // 7] >> (6 - i % 7)) & 1
        w[i >> 5] = (w[i >> 5] & ~(1 << (i & 31))) | (bit << (i & 31))
    w[2] = (w[2] & ~(0xF << 13)) | (rov << 13)
    return w


def vendor_lot_literal(words):
    """The decompiled loop of 0x01104600, transliterated (uVar8/uVar9/bVar1)."""
    out, u8 = [], 0
    for u10 in range(0x2A):
        u9 = ((words[u10 >> 5] >> (u10 & 0x1F)) & 1) << ((6 - u10 % 7) & 0x1F)
        b1 = u8 & 0xFF
        u8 = u8 | (u9 & 0xFF)
        if u10 % 7 == 6:
            out.append((b1 | u9) & 0xFF)
            u8 = 0
    return bytes(out)


class Decode(unittest.TestCase):
    def test_this_unit(self):
        d = avs.decode(UNIT)
        self.assertEqual(d["rov"], 0)
        self.assertEqual(d["lot"], b"PS7R09")
        self.assertEqual(d["target_mv"], 800)
        self.assertEqual((d["code"], d["mv"]), (0x0333, 800))
        avs.check_target(d)

    def test_roundtrip_and_literal_loop(self):
        rnd = random.Random(8210)
        for _ in range(2000):
            words = [rnd.getrandbits(32) for _ in range(4)]
            self.assertEqual(avs.lot_id(words), vendor_lot_literal(words))
            lot = bytes(rnd.getrandbits(7) for _ in range(6))
            r = rnd.getrandbits(4)
            e = encode(lot, r, words)
            self.assertEqual(avs.lot_id(e), lot)
            self.assertEqual(avs.rov(e), r)

    def test_rov0_lots(self):
        for lot in avs.LOTS_780:
            d = avs.decode(encode(lot, 0))
            self.assertEqual((d["target_mv"], d["code"]), (780, 0x31F), lot)
        for lot in (b"PS7R09", b"PNHB05", b"PNHB12", b"PPHN58", b"pnhb06"):
            self.assertEqual(avs.decode(encode(lot, 0))["target_mv"], 800, lot)

    def test_rov_table(self):
        want = {0: 800, 2: 820, 5: 840, 8: 780, 10: 760}
        codes = {760: 0x30A, 780: 0x31F, 800: 0x333, 820: 0x348, 840: 0x35C}
        for r in range(16):
            for lot in (b"PS7R09", b"PNHB06"):
                d = avs.decode(encode(lot, r))
                t = want.get(r, 800)
                if r == 0 and lot == b"PNHB06":
                    t = 780
                self.assertEqual(d["target_mv"], t, (r, lot))
                self.assertEqual((d["code"], d["mv"]), (codes[t], t), (r, lot))
                avs.check_target(d)

    def test_closest_vendor_rounding(self):
        for code, mv in avs.TABLE:
            self.assertEqual(avs.closest(mv), (code, mv))
            self.assertAlmostEqual(code / 1.024, mv, delta=1.0)
        self.assertEqual(avs.closest(790), (0x31F, 780))   # tie: the vendor keeps the lower
        self.assertEqual(avs.closest(791), (0x333, 800))
        self.assertEqual(avs.closest(770), (0x30A, 760))
        self.assertEqual(avs.closest(771), (0x31F, 780))
        self.assertEqual(avs.closest(830), (0x348, 820))

    def test_sanity_limits(self):
        for bad in ({"code": 0x2F6, "mv": 740}, {"code": 0x370, "mv": 860}, {"code": 0x334, "mv": 800}):
            with self.assertRaises(avs.Refuse):
                avs.check_target(bad)


class Formats(unittest.TestCase):
    def test_linear(self):
        # this regulator, 2026-11-17 (avs.md)
        self.assertAlmostEqual(avs.linear16(0x0333) * 1000, 799.8, places=1)
        self.assertAlmostEqual(avs.linear16(0x03CD), 0.9502, places=3)
        self.assertEqual(avs.linear11(0xD300), 12.0)
        self.assertEqual(avs.linear11(0xE2A8), 42.5)
        self.assertEqual(avs.linear11(0xF24A), 146.5)
        self.assertEqual(avs.linear11(0xFFFF), -0.5)   # exp -1, mantissa -1

    def test_words(self):
        self.assertEqual(avs.parse_words(avs.fmt_words(UNIT)), UNIT)
        with self.assertRaises(ValueError):
            avs.parse_words("1,2,3")


class Whitelist(unittest.TestCase):
    def test_refuses_other_commands(self):
        pm = avs.Pmbus.__new__(avs.Pmbus)
        pm._xfer = lambda *a: self.fail("reached the bus")
        for cmd in (0x03, 0x11, 0x12, 0x15, 0x16, 0x23, 0xD0):
            with self.assertRaises(ValueError):
                pm.read(cmd)
        for cmd, val in ((0x00, 1), (0x01, 0x80), (0x15, 0), (0x24, 0x400)):
            with self.assertRaises(ValueError):
                pm.write(cmd, val)


if __name__ == "__main__":
    unittest.main(verbosity=1)

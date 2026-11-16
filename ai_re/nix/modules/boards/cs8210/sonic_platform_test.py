#!/usr/bin/env python3
"""Unit test of the CS8210 sonic_platform on a fake board (fake_board.py).

  python3 -I sonic_platform_test.py   (sonic_platform, sonic_platform_base importable)
Run by the package's passthru.tests.unit.
"""
import json
import os
import shutil
import sys
import tempfile
import time
import unittest

ROOT = tempfile.mkdtemp(prefix="cs8210-fake-")
os.environ["CS8210_PLATFORM_ROOT"] = ROOT
os.environ.pop("CS8210_QSFP_PREPARE", None)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_board  # noqa: E402

fake_board.build(ROOT)

import sonic_platform.platform  # noqa: E402
from sonic_platform import fpga, hw  # noqa: E402


def fake(rel):
    return os.path.join(ROOT, rel.lstrip("/"))


def rd(p, mode="r"):
    with open(p, mode) as f:
        return f.read()


def wr(p, s):
    with open(p, "wb" if isinstance(s, bytes) else "w") as f:
        f.write(s)


def writes():
    """BAR0 writes so far: [(offset, value)], and clear the log."""
    p = fake("/fpga/writes.txt")
    try:
        out = [tuple(int(x, 16) for x in line.split()) for line in rd(p).splitlines()]
    except OSError:
        out = []
    if os.path.exists(p):
        os.remove(p)
    return out


class Board(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.chassis = sonic_platform.platform.Platform().get_chassis()

    def setUp(self):
        hw.QSFP_PREPARE = False
        writes()

    def test_inventory(self):
        c = self.chassis
        self.assertEqual(c.get_name(), "CS8210-32X")
        self.assertEqual(c.get_num_fan_drawers(), 14)
        self.assertEqual(sum(d.get_num_fans() for d in c.get_all_fan_drawers()), 28)
        self.assertEqual(c.get_num_thermals(), 14)
        self.assertEqual(c.get_num_psus(), 4)
        self.assertEqual(c.get_num_sfps(), 32)
        self.assertIsNone(c.get_sfp(0))
        self.assertEqual(c.get_sfp(32).get_name(), "QSFP-DD32")
        self.assertIsNone(c.get_thermal_manager())
        self.assertFalse(c.set_status_led("green"))

    def test_fans_from_fand(self):
        c = self.chassis
        names = [f.get_name() for f in c.get_all_fans()]
        self.assertEqual(names[:2], ["Fan top1 front", "Fan top1 rear"])
        self.assertEqual(names[-1], "Fan bottom7 rear")
        self.assertEqual(c.get_fan_drawer(7).get_name(), "FanTray bottom1")
        front, rear = c.get_fan_drawer(0).get_all_fans()
        self.assertEqual(front.source(), "fand")
        self.assertEqual(front.get_speed(), 59)            # 11200 / 19000
        self.assertEqual(rear.get_speed(), 59)             # 12300 / 21000
        self.assertEqual(front.get_target_speed(), 60)     # fand expected 11400 / 19000
        self.assertFalse(front.is_under_speed())
        for d in c.get_all_fan_drawers():
            self.assertTrue(d.get_presence())
            self.assertTrue(d.get_status())
            for f in d.get_all_fans():
                self.assertFalse(f.set_speed(100))
                self.assertFalse(f.set_status_led(f.STATUS_LED_COLOR_RED))

    def test_fan_fault_stale_and_cpld_fallback(self):
        c = self.chassis
        s = fake("/run/fand/state.json")
        orig = rd(s)
        st = json.loads(orig)
        r = st["outputs"]["bottom3"]["rotors"][1]
        r["rpm"], r["fault"], r["error"] = 0.0, True, "rear: 0 rpm is below the minimum 3000 rpm"
        wr(s, json.dumps(st))
        try:
            d = c.get_fan_drawer(9)                        # bottom3
            front, rear = d.get_all_fans()
            self.assertFalse(rear.get_presence())
            self.assertFalse(rear.get_status())
            self.assertTrue(d.get_presence())              # front still turns
            self.assertFalse(d.get_status())
            old = time.time() - 120                        # fand stopped 2 min ago
            os.utime(s, (old, old))
            self.assertEqual(rear.source(), "cpld")        # stale: fan CPLD read directly
            self.assertEqual(rear.get_speed(), 58)         # raw 121 x 100 / 21000
            self.assertTrue(rear.get_status())
            self.assertEqual(rear.get_target_speed(), 50)  # PWM 127 / 255
        finally:
            wr(s, orig)
        cpld = rd(fake("/i2c/fan_top.bin"), "rb")
        self.assertEqual(cpld[0x22], 127)                  # nothing wrote the PWM

    def test_thermals(self):
        t = {x.get_name(): x for x in self.chassis.get_all_thermals()}
        self.assertEqual(len(t), 14)
        self.assertIn("macsec_center", t)
        self.assertEqual(t["macsec_center"].get_temperature(), 39.5)   # fand's value
        self.assertEqual(t["macsec_center"].get_source(), "fand")
        self.assertEqual(t["asic"].get_temperature(), 51.3)
        self.assertEqual(t["asic"].get_source(), "fand")
        self.assertEqual(t["switchboard_center"].get_high_threshold(), 55.0)
        self.assertRaises(NotImplementedError, t["switchboard_center"].get_high_critical_threshold)
        self.assertEqual(t["asic"].get_high_threshold(), 95.0)
        self.assertEqual(t["asic"].get_high_critical_threshold(), 100.0)
        s = fake("/run/fand/state.json")
        os.rename(s, s + ".off")
        try:
            self.assertEqual(t["macsec_center"].get_temperature(), 39.0)   # hwmon by DT node
            self.assertEqual(t["macsec_center"].get_source(), "hwmon")
            self.assertEqual(t["bottom_fanboard_left"].get_temperature(), 28.0)
            self.assertEqual(t["asic"].get_temperature(), 52.0)            # syncd's file
            self.assertEqual(t["asic"].get_source(), "file")
            a = fake("/run/asic/temperature")
            os.rename(a, a + ".off")
            try:
                self.assertIsNone(t["asic"].get_temperature())
                self.assertIsNone(t["asic"].get_source())
                self.assertFalse(t["asic"].get_presence())
            finally:
                os.rename(a + ".off", a)
        finally:
            os.rename(s + ".off", s)

    def test_psus_live_dump(self):
        p = self.chassis.get_all_psus()
        self.assertEqual([x.get_name() for x in p], ["PSU 1", "PSU 2", "PSU 3", "PSU 4"])
        # 0x08 = 0x45, 0xa8 = 0x00 (2026-11-15): PSU1 OK; 2-4 "present" with no power
        self.assertTrue(p[0].get_presence())
        self.assertTrue(p[0].get_powergood_status())
        self.assertTrue(p[0].get_input_good())
        for x in p[1:]:
            self.assertTrue(x.get_presence())
            self.assertFalse(x.get_powergood_status())
            self.assertFalse(x.get_input_good())
        cpld = fake("/i2c/sys_cpld.bin")
        orig = rd(cpld, "rb")
        b = bytearray(orig)
        b[0x08] = 0x65                                     # PSU2 b5 high: absent
        wr(cpld, bytes(b))
        try:
            self.assertFalse(p[1].get_presence())
            self.assertTrue(p[0].get_status())
        finally:
            wr(cpld, orig)
        os.rename(cpld, cpld + ".off")
        try:
            self.assertFalse(p[0].get_presence())          # CPLD unreadable: nothing claimed
        finally:
            os.rename(cpld + ".off", cpld)
        self.assertFalse(p[0].set_status_led("green"))

    def test_syseeprom(self):
        c = self.chassis
        info = c.get_system_eeprom_info()
        self.assertEqual(info["0x21"], "CS8210-32X-UN-00")
        self.assertEqual(info["0x24"], "0C:48:C6:7A:55:CE")
        self.assertEqual(c.get_model(), "R3181-F9003-01")
        self.assertEqual(c.get_serial(), "R3181-G0001-02TH0321110078")
        e = c.get_eeprom()
        self.assertTrue(e.p.endswith("/15-0056/eeprom"))   # not the decoy FRU at 8-0056
        self.assertTrue(e.is_checksum_valid(e.read_eeprom())[0])

    def test_i2c_buses_by_node(self):
        self.assertEqual(hw.adapter_by_node(hw.I2C_DEVICES["sys_cpld"][0]), 9)
        self.assertEqual(hw.adapter_by_node(hw.I2C_DEVICES["fan_top"][0]), 31)
        self.assertEqual(hw.adapter_by_node(hw.I2C_DEVICES["fan_bottom"][0]), 40)

    def test_sfp_cmis_paged(self):
        s = self.chassis.get_sfp(1)
        self.assertTrue(s.get_presence())
        self.assertFalse(s.get_reset_status())
        info = s.get_transceiver_info()
        self.assertEqual(info["manufacturer"].strip(), "FAKE-VENDOR")
        self.assertEqual(info["model"].strip(), "FAKE-QDD-DR4")
        self.assertEqual(info["serial"].strip(), "FAKESN0001")
        self.assertIn("QSFP-DD", info["type"])
        self.assertEqual(info["hardware_rev"], "1.0")      # upper page 1 through page select
        self.assertEqual(rd(fake("/fpga/port1.bin"), "rb")[127], 0)   # page 0 again
        self.assertAlmostEqual(float(s.get_temperature()), 35.25, places=2)
        w = writes()
        self.assertTrue(w)
        for off, val in w:                                 # only cmd/start/write buffer
            self.assertTrue(fpga.allowed(off, True), hex(off))
            if fpga.WBUF <= off < fpga.RBUF:
                self.assertLess(val, 0x100)                # one byte: a page number
        self.assertIsNone(s.read_eeprom(128 * 0x30, 1))    # page the module lacks: NACK

    def test_sfp_cmis_flat_dac(self):
        s = self.chassis.get_sfp(16)
        info = s.get_transceiver_info()
        self.assertEqual(info["model"].strip(), "FAKE-QDD-DAC1M")
        self.assertIsNone(s.read_eeprom(256, 1))           # flat: no page 1
        self.assertEqual([o for o, _ in writes() if fpga.WBUF <= o < fpga.RBUF], [])   # no page select

    def test_sfp_read_only(self):
        c = self.chassis
        before = {p: rd(fake("/fpga/port%d.bin" % p), "rb") for p in (1, 16, 17, 20)}
        sb = rd(fake("/fpga/sideband.json"))
        for p in (1, 16, 17, 20):
            s = c.get_sfp(p)
            self.assertFalse(s.set_lpmode(True))
            self.assertFalse(s.reset())
            self.assertFalse(s.write_eeprom(0, 1, bytearray(b"\x00")))
            self.assertFalse(s.tx_disable(True))
        for p, img in before.items():
            self.assertEqual(rd(fake("/fpga/port%d.bin" % p), "rb"), img)
        self.assertEqual(rd(fake("/fpga/sideband.json")), sb)
        self.assertFalse(any(o < fpga.CH_BASE for o, _ in writes()))  # no global register written

    def test_sfp_qsa(self):
        s = self.chassis.get_sfp(20)
        info = s.get_transceiver_info()
        self.assertEqual(info["model"].strip(), "SFP-H10GB-CU3M")
        self.assertEqual(s.read_eeprom(256, 4), bytearray(b"\xff" * 4))   # A2h at 0x51
        self.assertFalse(s.get_lpmode())

    def test_sfp_in_reset_and_empty(self):
        c = self.chassis
        s = c.get_sfp(30)
        self.assertTrue(s.get_presence())
        self.assertTrue(s.get_reset_status())
        self.assertFalse(s.get_status())
        self.assertIsNone(s.read_eeprom(0, 1))
        e = c.get_sfp(5)
        self.assertFalse(e.get_presence())
        self.assertIsNone(e.read_eeprom(0, 1))
        self.assertIsNone(e.get_transceiver_info())

    def test_sfp_channel_init_gate(self):
        s = self.chassis.get_sfp(17)                       # channel at power-up values
        s.invalidate()
        self.assertIsNone(s.read_eeprom(0, 1))
        self.assertEqual(writes(), [])                     # refused before any write
        hw.QSFP_PREPARE = True
        self.assertEqual(s.read_eeprom(0, 1), bytearray(b"\x11"))
        cb = fpga.chb(17)
        w = writes()
        self.assertEqual(w[:3], [(cb + fpga.C_MRESET, 0x80000000), (cb + fpga.C_PROFSEL, 0),
                                 (cb + fpga.C_PROFILE, fpga.PROBE_PROFILE)])
        self.assertEqual(s.get_transceiver_info()["model"].strip(), "QSFP-4SFP25-CU3M")

    def test_pci_decode_gate(self):
        c = self.chassis
        f = fpga.fpga()
        cfgp = fake("/sys/bus/pci/devices/0002:01:00.0/config")
        orig = rd(cfgp, "rb")
        b = bytearray(orig)
        b[4] = 0x00                                        # decode off, as after U-Boot
        wr(cfgp, bytes(b))
        en = fake("/sys/bus/pci/devices/0002:01:00.0/enable")
        wr(en, "0\n")
        try:
            self.assertFalse(c.get_sfp(1).get_presence())  # nothing read, nothing claimed
            self.assertIn("memory decode is off", f.problem)
            self.assertEqual(rd(en), "0\n")               # and enable not written
            hw.QSFP_PREPARE = True
            self.assertTrue(c.get_sfp(1).get_presence())
            self.assertEqual(rd(en), "1")
            self.assertIsNone(f.problem)
        finally:
            wr(cfgp, orig)
            wr(en, "1\n")

    def test_allowlist(self):
        self.assertTrue(fpga.allowed(fpga.R_PRESENT, False))
        for off in (fpga.R_LPMODE, fpga.R_RESET, 0x168, 0x150, fpga.R_VERSION):
            self.assertFalse(fpga.allowed(off, True), hex(off))
        cb = fpga.chb(1)
        self.assertTrue(fpga.allowed(cb + fpga.C_START, True))
        self.assertFalse(fpga.allowed(cb + fpga.C_PROFILE, True))
        self.assertTrue(fpga.allowed(cb + fpga.C_PROFILE, True, prepare=True))
        self.assertFalse(fpga.allowed(cb + fpga.C_STATUS, True, prepare=True))
        self.assertFalse(fpga.allowed(fpga.WBUF + fpga.BUF_MAX, True))
        self.assertFalse(fpga.allowed(fpga.R_PRESENT + 2, False))

    def test_change_event(self):
        c = self.chassis
        c._present = c._presence()
        self.assertEqual(c.get_change_event(100), (True, {"sfp": {}}))
        p = fake("/fpga/port16.bin")
        os.rename(p, p + ".out")
        try:
            t0 = time.monotonic()
            self.assertEqual(c.get_change_event(3000), (True, {"sfp": {"16": "0"}}))
            self.assertLess(time.monotonic() - t0, 2)
        finally:
            os.rename(p + ".out", p)
        self.assertEqual(c.get_change_event(3000), (True, {"sfp": {"16": "1"}}))
        self.assertEqual(c.get_port_or_cage_type(1), 0x800)    # QSFP-DD


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        shutil.rmtree(ROOT, ignore_errors=True)

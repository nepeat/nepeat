#!/usr/bin/env python3
"""Unit test of the AS7760 sonic_platform on a fake board (fake_board.py).

  python3 sonic_platform_test.py      (sonic_platform, sonic_platform_base on sys.path)
Run by the package's passthru.tests.unit and by the VM test.
"""
import os
import shutil
import sys
import tempfile
import time
import unittest

ROOT = tempfile.mkdtemp(prefix="as7760-fake-")
os.environ["AS7760_PLATFORM_ROOT"] = ROOT
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_board  # noqa: E402

fake_board.build(ROOT)

import sonic_platform.platform  # noqa: E402
from sonic_platform import hw  # noqa: E402


def fake(rel):
    return os.path.join(ROOT, rel.lstrip("/"))


def rd(p, mode="r"):
    with open(p, mode) as f:
        return f.read()


def wr(p, s):
    with open(p, "w") as f:
        f.write(s)


class Board(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.chassis = sonic_platform.platform.Platform().get_chassis()

    def test_inventory(self):
        c = self.chassis
        self.assertEqual(c.get_num_fan_drawers(), 6)
        self.assertEqual(sum(d.get_num_fans() for d in c.get_all_fan_drawers()), 12)
        self.assertEqual(c.get_num_thermals(), 5)
        self.assertEqual(c.get_num_psus(), 2)
        self.assertEqual(c.get_num_sfps(), 32)
        self.assertIsNone(c.get_sfp(0))
        self.assertEqual(c.get_sfp(32).get_name(), "QSFP32")

    def test_no_fan_policy(self):
        self.assertIsNone(self.chassis.get_thermal_manager())

    def test_fans_read_only(self):
        pwm = fake("/sys/class/hwmon/hwmon0/pwm1")
        before = (rd(pwm), os.stat(pwm).st_mtime_ns)
        for d in self.chassis.get_all_fan_drawers():
            self.assertTrue(d.get_presence())
            self.assertTrue(d.get_status())
            for f in d.get_all_fans():
                self.assertTrue(f.get_presence())
                self.assertFalse(f.set_speed(100))
                self.assertFalse(f.set_status_led(f.STATUS_LED_COLOR_RED))
        self.assertFalse(self.chassis.set_status_led("green"))
        self.assertEqual((rd(pwm), os.stat(pwm).st_mtime_ns), before)

    def test_fan_speeds(self):
        front, rear = self.chassis.get_fan_drawer(0).get_all_fans()
        self.assertEqual(front.get_name(), "Fan 1 front")
        self.assertEqual(front.get_speed(), 59)            # 12400 / 21000
        self.assertEqual(rear.get_speed(), 59)             # 10600 / 18000
        self.assertEqual(front.get_target_speed(), 60)     # fand expected 12600 / 21000
        self.assertFalse(front.is_under_speed())
        self.assertFalse(front.is_over_speed())

    def test_fan_fault_and_no_fand(self):
        f = self.chassis.get_fan_drawer(2).get_all_fans()[1]   # fan 3 rear = hwmon fan6
        p, s = fake("/sys/class/hwmon/hwmon0/fan6_input"), fake("/run/fand/state.json")
        q = fake("/sys/class/hwmon/hwmon0/fan6_fault")
        os.rename(s, s + ".off")
        try:
            self.assertEqual(f.get_target_speed(), 53)     # pwm1 136/255, fand not running
            wr(p, "0\n"); wr(q, "1\n")
            self.assertFalse(f.get_presence())
            self.assertFalse(f.get_status())
            self.assertTrue(self.chassis.get_fan_drawer(2).get_presence())   # front rotor turns
            self.assertFalse(self.chassis.get_fan_drawer(2).get_status())
        finally:
            os.rename(s + ".off", s)
            wr(p, "10600\n"); wr(q, "0\n")

    def test_thermals(self):
        t = {x.get_name(): x for x in self.chassis.get_all_thermals()}
        self.assertEqual(sorted(t), ["asic", "board_1", "board_2", "board_3", "cpu_board"])
        self.assertEqual(t["board_1"].get_temperature(), 30.5)
        self.assertEqual(t["cpu_board"].get_temperature(), 31.5)
        self.assertEqual(t["asic"].get_temperature(), 41.5)
        self.assertEqual(t["board_2"].get_high_threshold(), 45.0)
        self.assertEqual(t["asic"].get_high_critical_threshold(), 100.0)
        a = fake("/run/asic/temperature")
        os.rename(a, a + ".off")
        try:
            self.assertIsNone(t["asic"].get_temperature())   # writer removed it: no reading
        finally:
            os.rename(a + ".off", a)

    def test_psus(self):
        p1, p2 = self.chassis.get_all_psus()
        self.assertEqual(p1.get_name(), "PSU 1")
        self.assertTrue(p1.get_presence())
        self.assertTrue(p1.get_powergood_status())
        self.assertFalse(p2.get_presence())
        self.assertFalse(p2.get_powergood_status())
        self.assertFalse(p1.set_status_led("green"))

    def test_syseeprom(self):
        info = self.chassis.get_system_eeprom_info()
        self.assertEqual(info["0x21"], "7760-32X-A-AC-F")
        self.assertEqual(info["0x23"], "776032X16521259")
        self.assertEqual(info["0x24"], "8C:EA:1B:89:C9:3C")
        self.assertEqual(self.chassis.get_serial(), "776032X16521259")
        e = self.chassis.get_eeprom()
        raw = e.read_eeprom()
        self.assertTrue(e.is_checksum_valid(raw)[0])
        self.assertTrue(e.p.endswith("/dev/mtd1"))

    def test_sfp_dac_in_qsa(self):
        s = self.chassis.get_sfp(1)
        self.assertTrue(s.get_presence())
        self.assertFalse(s.get_reset_status())
        info = s.get_transceiver_info()
        self.assertEqual(info["manufacturer"].strip(), "FAKE-VENDOR")
        self.assertEqual(info["model"].strip(), "FAKE-SFP28-DAC")
        self.assertEqual(info["serial"].strip(), "FAKESN0001")
        self.assertFalse(s.get_lpmode())
        self.assertFalse(s.set_lpmode(True))               # SFP: no low-power mode

    def test_sfp_qsfp28_paged(self):
        s = self.chassis.get_sfp(32)
        info = s.get_transceiver_info()
        self.assertEqual(info["model"].strip(), "FAKE-QSFP28-SR4")
        self.assertIn("100GBASE-SR4", str(info.get("specification_compliance", "")))
        dom = s.get_transceiver_dom_real_value()
        self.assertAlmostEqual(float(dom["temperature"]), 30.5, places=1)
        thr = s.get_transceiver_threshold_info()           # page 3 through page select
        self.assertAlmostEqual(float(thr["temphighalarm"]), 75.0, places=1)
        img = rd(fake("/i2c/port32.bin"), "rb")
        self.assertEqual(img[127], 0)                      # page 0 selected again
        self.assertIsNone(s.read_eeprom(5000, 1))
        self.assertFalse(s.get_lpmode())
        self.assertTrue(s.set_lpmode(True))
        self.assertTrue(s.get_lpmode())
        self.assertEqual(rd(fake("/i2c/port32.bin"), "rb")[93] & 3, 3)
        self.assertTrue(s.set_lpmode(False))
        self.assertEqual(rd(fake("/i2c/port32.bin"), "rb")[93] & 3, 1)
        before = rd(fake("/i2c/port32.bin"), "rb")
        self.assertFalse(s.write_eeprom(148, 4, bytearray(b"EVIL")))   # vendor name
        self.assertFalse(s.write_eeprom(3 * 128 + 128, 2, bytearray(2)))  # thresholds
        self.assertEqual(rd(fake("/i2c/port32.bin"), "rb"), before)

    def test_empty_cage(self):
        s = self.chassis.get_sfp(5)
        self.assertFalse(s.get_presence())
        self.assertIsNone(s.read_eeprom(0, 1))
        self.assertIsNone(s.get_transceiver_info())

    def test_reset_and_change_event(self):
        c = self.chassis
        self.assertTrue(c.get_sfp(1).reset())
        self.assertEqual(rd(fake("/i2c/resets.log")), "1\n")
        self.assertEqual(c.get_change_event(100), (True, {"sfp": {}}))
        p = fake("/i2c/port32.bin")
        os.rename(p, p + ".out")
        t0 = time.monotonic()
        self.assertEqual(c.get_change_event(3000), (True, {"sfp": {"32": "0"}}))
        self.assertLess(time.monotonic() - t0, 2)
        self.assertFalse(c.get_sfp(32).get_presence())
        os.rename(p + ".out", p)
        self.assertEqual(c.get_change_event(3000), (True, {"sfp": {"32": "1"}}))
        self.assertIn("FAKE-QSFP28-SR4", c.get_sfp(32).get_transceiver_info()["model"])

    def test_cpld_bits(self):
        prs = hw.cpld_bits(hw.MODPRS_L)
        self.assertEqual([p for p in prs if prs[p]], [1, 32])


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        shutil.rmtree(ROOT, ignore_errors=True)

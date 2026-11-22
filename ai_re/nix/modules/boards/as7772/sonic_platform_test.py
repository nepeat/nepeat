#!/usr/bin/env python3
"""Unit test of the AS7772 deltas of the AS7760 sonic_platform (sensor list)
on the AS7760 fake board plus this board's extra sensors. Everything else is
the AS7760 code, tested by ../as7760/sonic_platform_test.py.
"""
import os
import sys
import tempfile
import unittest

ROOT = tempfile.mkdtemp(prefix="as7772-fake-")
os.environ["AS7760_PLATFORM_ROOT"] = ROOT
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_board  # noqa: E402

fake_board.build(ROOT)
fake_board.add_hwmon(ROOT, 5, "lm75", 0x4c, {1: 24000})
fake_board.add_hwmon(ROOT, 6, "lm75", 0x4d, {1: 25500})
fake_board.add_hwmon(ROOT, 7, "max1617", 0x18, {1: 29000, 2: 39000})

import sonic_platform.platform  # noqa: E402


class Board(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.chassis = sonic_platform.platform.Platform().get_chassis()

    def test_inventory(self):
        c = self.chassis
        self.assertEqual(c.get_num_thermals(), 9)
        self.assertEqual(c.get_num_fan_drawers(), 6)
        self.assertEqual(c.get_num_psus(), 2)
        self.assertEqual(c.get_num_sfps(), 32)

    def test_thermals(self):
        t = {x.get_name(): x for x in self.chassis.get_all_thermals()}
        self.assertEqual(sorted(t), ["asic", "board_1", "board_2", "board_3", "board_4", "board_5", "cpu_area",
                                     "cpu_board", "cpu_die"])
        self.assertEqual(t["board_1"].get_temperature(), 30.5)
        self.assertEqual(t["board_4"].get_temperature(), 24.0)
        self.assertEqual(t["board_5"].get_temperature(), 25.5)
        self.assertEqual(t["cpu_area"].get_temperature(), 29.0)     # MAX1617 local
        self.assertEqual(t["cpu_die"].get_temperature(), 39.0)      # MAX1617 remote diode (temp2)
        self.assertEqual(t["cpu_die"].get_high_threshold(), 95.0)
        self.assertEqual(t["cpu_die"].get_high_critical_threshold(), 105.0)
        self.assertEqual(t["board_5"].get_high_threshold(), 45.0)
        self.assertEqual(t["asic"].get_temperature(), 41.5)
        self.assertEqual([x.get_position_in_parent() for x in self.chassis.get_all_thermals()], list(range(1, 10)))
        os.remove(os.path.join(ROOT, "sys/class/hwmon/hwmon7/temp2_input"))
        self.assertIsNone(t["cpu_die"].get_temperature())
        self.assertFalse(t["cpu_die"].get_presence())


if __name__ == "__main__":
    r = unittest.main(exit=False, verbosity=1).result
    print("OK" if r.wasSuccessful() else "FAILED")
    sys.exit(0 if r.wasSuccessful() else 1)

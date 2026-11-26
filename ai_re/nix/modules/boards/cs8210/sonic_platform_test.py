#!/usr/bin/env python3
"""Unit test of the CS8210 sonic_platform on a fake board (fake_board.py).

  python3 -I sonic_platform_test.py   (sonic_platform, sonic_platform_base importable)
Run by the package's passthru.tests.unit.
"""
import json
import os
import shutil
import socket
import struct
import sys
import tempfile
import threading
import time
import unittest

ROOT = tempfile.mkdtemp(prefix="cs8210-fake-")
os.environ["CS8210_PLATFORM_ROOT"] = ROOT
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fake_board  # noqa: E402

fake_board.build(ROOT)

import sonic_platform.platform  # noqa: E402
from sonic_platform import hw, optics  # noqa: E402


def fake(rel):
    return os.path.join(ROOT, rel.lstrip("/"))


def rd(p, mode="r"):
    with open(p, mode) as f:
        return f.read()


def wr(p, s):
    with open(p, "wb" if isinstance(s, bytes) else "w") as f:
        f.write(s)


def calls():
    """write / lpmode / reset requests made so far (fake cs8210-optics), and clear the log."""
    p = fake("/fpga/calls.txt")
    try:
        out = [json.loads(line) for line in rd(p).splitlines()]
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
        calls()

    def test_inventory(self):
        c = self.chassis
        self.assertEqual(c.get_name(), "CS8210-32X")
        self.assertEqual(c.get_num_fan_drawers(), 14)
        self.assertEqual(sum(d.get_num_fans() for d in c.get_all_fan_drawers()), 28)
        self.assertEqual(c.get_num_thermals(), 14)
        self.assertEqual(c.get_num_psus(), 2)                 # 19-inch variant (sys 0x09 b0 = 1)
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
        self.assertEqual([x.get_name() for x in p], ["PSU 1", "PSU 2"])
        # 0x08 = 0x45 (2026-11-15): PSU1 OK; PSU2 present without power
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
        # PMBus identity and readings (cs8210-psu's file, the pmbus driver's hwmon)
        os.makedirs(fake("/run/cs8210-psu"), exist_ok=True)
        wr(fake("/run/cs8210-psu/psu1.json"), json.dumps({"bus": 15, "model": "TDPS-1500AB-6 B", "serial": "T1", "revision": "02"}))
        hw_ = fake("/sys/bus/i2c/devices/15-0059/hwmon/hwmon99")
        os.makedirs(hw_, exist_ok=True)
        for n, lab, v in (("in1", "vin", 116750), ("power2", "pout1", 642000000), ("curr2", "iout1", 53250), ("temp1", "temp1", 37000)):
            wr(hw_ + "/%s_label" % n, lab + "\n")
            wr(hw_ + "/%s_input" % n, "%d\n" % v)
        self.assertEqual(p[0].get_model(), "TDPS-1500AB-6 B")
        self.assertAlmostEqual(p[0].get_input_voltage(), 116.75)
        self.assertAlmostEqual(p[0].get_power(), 642.0)
        self.assertAlmostEqual(p[0].get_current(), 53.25)
        self.assertAlmostEqual(p[0].get_temperature(), 37.0)
        self.assertEqual(p[1].get_model(), "N/A")
        self.assertIsNone(p[1].get_power())

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
        self.assertEqual(info["hardware_rev"], "1.0")      # upper page 1 at flat offset 256
        self.assertAlmostEqual(float(s.get_temperature()), 35.25, places=2)
        self.assertEqual(calls(), [])                      # reads only
        self.assertIsNone(s.read_eeprom(128 * 0x30, 1))    # page the module lacks

    def test_sfp_cmis_flat_dac(self):
        s = self.chassis.get_sfp(16)
        info = s.get_transceiver_info()
        self.assertEqual(info["model"].strip(), "FAKE-QDD-DAC1M")
        self.assertIsNone(s.read_eeprom(256, 1))           # flat: no page 1

    def test_sfp_writes_go_to_the_daemon(self):
        s = self.chassis.get_sfp(1)
        before = rd(fake("/fpga/port1.bin"), "rb")
        try:
            self.assertTrue(s.write_eeprom(26, 1, bytearray(b"\x10")))
            self.assertTrue(s.set_lpmode(True))
            self.assertTrue(s.get_lpmode())
            self.assertTrue(s.set_lpmode(False))
            self.assertTrue(s.reset())
            self.assertEqual(calls(), [{"op": "write", "cage": 1, "offset": 26, "data": "10"},
                                       {"op": "lpmode", "cage": 1, "on": True},
                                       {"op": "lpmode", "cage": 1, "on": False},
                                       {"op": "reset", "cage": 1}])
            self.assertEqual(rd(fake("/fpga/port1.bin"), "rb")[26], 0x10)
        finally:
            wr(fake("/fpga/port1.bin"), before)
        self.assertFalse(self.chassis.get_sfp(5).reset())  # empty cage: refused

    def test_vdm_freeze_that_never_confirms_is_given_up(self):
        from sonic_platform import sfp as S
        s = self.chassis.get_sfp(1)
        s.invalidate()
        orig = (S.SfpOptoeBase.freeze_vdm_stats, S.SfpOptoeBase.get_vdm_freeze_status,
                S.SfpOptoeBase.is_vdm_statistic_supported, S.SfpOptoeBase.get_transceiver_info_firmware_versions)
        calls = []
        try:
            S.SfpOptoeBase.freeze_vdm_stats = lambda self: True
            S.SfpOptoeBase.get_vdm_freeze_status = lambda self: False
            S.SfpOptoeBase.is_vdm_statistic_supported = lambda self: True
            S.SfpOptoeBase.get_transceiver_info_firmware_versions = lambda self: calls.append(1) or {"fw": "x"}
            self.assertTrue(s.is_vdm_statistic_supported())
            S.VDM_DONE_S = 0.05
            s.freeze_vdm_stats()
            t0 = time.monotonic()
            while time.monotonic() - t0 < 0.2:
                self.assertFalse(s.get_vdm_freeze_status())
            self.assertFalse(s.is_vdm_statistic_supported())
            self.assertEqual(s.get_transceiver_info_firmware_versions(), {"fw": "x"})
            s.get_transceiver_info_firmware_versions()
            self.assertEqual(len(calls), 1)                  # asked once per module
            s.invalidate()
            self.assertTrue(s.is_vdm_statistic_supported())  # a new module is asked again
        finally:
            (S.SfpOptoeBase.freeze_vdm_stats, S.SfpOptoeBase.get_vdm_freeze_status,
             S.SfpOptoeBase.is_vdm_statistic_supported, S.SfpOptoeBase.get_transceiver_info_firmware_versions) = orig
            S.VDM_DONE_S = 0.9
            s.invalidate()

    def test_sfp_qsa(self):
        s = self.chassis.get_sfp(20)
        info = s.get_transceiver_info()
        self.assertEqual(info["model"].strip(), "SFP-H10GB-CU3M")
        self.assertEqual(s.read_eeprom(256, 4), bytearray(b"\xff" * 4))   # A2h
        self.assertFalse(s.get_lpmode())

    def test_sfp_qsfp28(self):
        s = self.chassis.get_sfp(17)
        self.assertEqual(s.read_eeprom(0, 1), bytearray(b"\x11"))
        self.assertEqual(s.get_transceiver_info()["model"].strip(), "QSFP-4SFP25-CU3M")

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

    def test_fpga_unavailable(self):
        sbp = fake("/fpga/sideband.json")
        orig = rd(sbp)
        sb = json.loads(orig)
        sb["dead"] = "status reads 0xffffffff"
        wr(sbp, json.dumps(sb))
        try:
            c = self.chassis
            self.assertIsNone(c._presence())
            self.assertFalse(c.get_sfp(1).get_presence())
            self.assertIsNone(c.get_sfp(1).read_eeprom(0, 1))
        finally:
            wr(sbp, orig)

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


class Client(unittest.TestCase):
    """optics.Client against a socket that answers as cs8210-optics does."""

    def serve(self, answers):
        path = os.path.join(ROOT, "optics-%d.sock" % len(answers))
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(path)
        srv.listen(8)
        got = []

        def run():
            for a in answers:
                conn, _ = srv.accept()
                with conn:
                    got.append(json.loads(conn.makefile().readline()))
                    conn.sendall((json.dumps(a) + "\n").encode())
            srv.close()

        t = threading.Thread(target=run, daemon=True)
        t.start()
        return optics.Client(path), got, t

    def test_requests_and_replies(self):
        cl, got, t = self.serve([
            {"ok": True, "present": 5, "reset": 4, "lpmode": 0, "interrupt": 1, "dead": None},
            {"ok": True, "data": "1840"},
            {"ok": False, "error": "cage 2: no module"},
            {"ok": True},
        ])
        self.assertEqual(cl.sideband(), {"present": 5, "reset": 4, "lpmode": 0, "interrupt": 1})
        self.assertEqual(cl.read(1, 0, 2), b"\x18\x40")
        with self.assertRaises(OSError):
            cl.read(2, 0, 1)
        cl.write(1, 2178, b"\x00")
        t.join(5)
        self.assertEqual(got, [{"op": "sideband"}, {"op": "read", "cage": 1, "offset": 0, "len": 2},
                               {"op": "read", "cage": 2, "offset": 0, "len": 1},
                               {"op": "write", "cage": 1, "offset": 2178, "data": "00"}])

    def test_dead_and_down(self):
        cl, _, t = self.serve([{"ok": True, "present": 0, "reset": 0, "lpmode": 0, "interrupt": 0,
                                "dead": "root port LnkSta 0x1012 (data link down)"}])
        self.assertIsNone(cl.sideband())
        self.assertIn("data link down", cl.problem)
        t.join(5)
        down = optics.Client(os.path.join(ROOT, "nobody.sock"))
        self.assertIsNone(down.sideband())
        with self.assertRaises(OSError):
            down.read(1, 0, 1)


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        shutil.rmtree(ROOT, ignore_errors=True)

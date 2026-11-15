"""ai-re-images against a synthetic devices/ tree: no hardware, no real dumps."""

import binascii
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("images", Path(__file__).parents[1] / "images.py")
images = importlib.util.module_from_spec(spec)
spec.loader.exec_module(images)

HAVE_ZSTD = shutil.which("zstd") is not None
SECRET_SERIAL = "SERIALSECRET42"
SECRET_MAC = "de:ad:be:ef:00:42"
PRIVATE_TEXT = "PRIVATE-CONTENT-DO-NOT-PRINT"


def sha(b):
    return hashlib.sha256(b).hexdigest()


def uimage(name, payload, typ=2, comp=1):
    hdr = struct.pack(">7I4B32s", 0x27051956, 0, 1590000000, len(payload), 0x80000, 0x80000,
                      binascii.crc32(payload), 5, 22, typ, comp, name.encode())
    crc = binascii.crc32(hdr)
    return hdr[:4] + struct.pack(">I", crc) + hdr[8:] + payload


def onie_eeprom():
    tlv = b""
    for t, v in ((0x21, b"TEST-BOX-48"), (0x22, b"PN-1234"), (0x23, SECRET_SERIAL.encode()),
                 (0x24, bytes.fromhex(SECRET_MAC.replace(":", ""))), (0x29, b"2020.1")):
        tlv += bytes([t, len(v)]) + v
    body = b"TlvInfo\0" + b"\x01" + struct.pack(">H", len(tlv) + 6) + tlv + b"\xfe\x04"
    return (body + struct.pack(">I", binascii.crc32(body))).ljust(256, b"\xff")


def uboot_env(size=0x1000, big_endian=False):
    data = b"bootcmd=run boot\0ethaddr=" + SECRET_MAC.encode() + b"\0serial#=" + SECRET_SERIAL.encode() + b"\0\0"
    data = data.ljust(size - 4, b"\0")
    return struct.pack(">I" if big_endian else "<I", binascii.crc32(data)) + data


def mbr_disk():
    disk = bytearray(3 << 20)
    entry = struct.pack("<B3sB3sII", 0x80, b"\0\0\0", 0x83, b"\0\0\0", 2048, 4096)
    disk[446:462] = entry
    disk[510:512] = b"\x55\xaa"
    p = 2048 * 512
    disk[p + 0x404:p + 0x408] = struct.pack("<I", 512)
    disk[p + 0x418:p + 0x41c] = struct.pack("<I", 2)
    disk[p + 0x438:p + 0x43a] = b"\x53\xef"
    disk[p + 0x460:p + 0x464] = struct.pack("<I", 0x40)
    disk[p + 0x478:p + 0x480] = b"testroot"
    return bytes(disk)


def flash_with_parts():
    f = bytearray(b"\xff" * (256 << 10))
    k = uimage("embedded-kernel", b"K" * 100)
    f[0x10000:0x10000 + len(k)] = k
    env = uboot_env(0x2000)
    f[0x30000:0x30000 + len(env)] = env
    return bytes(f)


class Fixture:
    def __init__(self, tmp):
        self.root = Path(tmp) / "ai_re"
        dev = self.root / "devices"
        (self.root / "DEVICES.md").parent.mkdir(parents=True)
        (self.root / "DEVICES.md").write_text("index\n")
        a = dev / "dev-a"
        u1, u2 = a / "dumps" / "u01", a / "dumps" / "u02"
        for d in (u1 / "factory-os", u2 / "factory-os", dev / "dev-b" / "images", dev / "dev-b" / "images" / "open-jffs2"):
            d.mkdir(parents=True)
        self.flash = flash_with_parts()
        raw = u1 / "mtd0.bin"
        raw.write_bytes(self.flash)
        subprocess.run(["zstd", "-q", "-T1", "--long=27", str(raw), "-o", str(u1 / "mtd0.bin.zst")], check=True)
        zst = (u1 / "mtd0.bin.zst").read_bytes()
        raw.unlink()
        self.kernel = uimage("TEST_Kernel 1.2.3.4@abc", b"\x1f\x8b\x08" + b"k" * 4000)
        (u1 / "factory-os" / "kernel-1.2.3.4.uImage").write_bytes(self.kernel)
        (u2 / "factory-os" / "kernel-1.2.3.4.uImage").write_bytes(self.kernel)
        (u1 / "board-eeprom.bin").write_bytes(onie_eeprom())
        (u1 / "uboot-env.bin").write_bytes(uboot_env(0x1000, big_endian=True))
        (u1 / "notes.log").write_text("a log, not an image\n")
        (u1 / "uboot-printenv-private.txt").write_text(PRIVATE_TEXT)
        (u1 / "dump-private.bin").write_bytes(PRIVATE_TEXT.encode())
        (a / "units-private.txt").write_text(PRIVATE_TEXT)
        rows = [{"file": "mtd0.bin", "source": "/dev/mtd0", "bytes": len(self.flash), "sha256": sha(self.flash),
                 "zst_bytes": len(zst), "zst_sha256": sha(zst), "date": "2026-11-15T02:28:19-0800"}]
        (u1 / "manifest.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
        sums = [(sha(self.flash), "mtd0.bin"), (sha(zst), "mtd0.bin.zst"),
                (sha(self.kernel), "factory-os/kernel-1.2.3.4.uImage"), (sha(onie_eeprom()), "board-eeprom.bin"),
                (sha(b"log"), "notes.log")]
        (u1 / "SHA256SUMS").write_text("".join(f"{h}  {n}\n" for h, n in sums))
        (u2 / "SHA256SUMS").write_text(f"{sha(self.kernel)}  factory-os/kernel-1.2.3.4.uImage\n")
        b = dev / "dev-b" / "images"
        self.disk = mbr_disk()
        (b / "sda.img").write_bytes(self.disk)
        (b / "bad.bin").write_bytes(b"\x00" * 4096)
        (b / "untracked.bin").write_bytes(b"\x01" * 4096)
        (b / "open-jffs2" / "inner.bin").write_bytes(b"\x02" * 16)
        (b / "SHA256SUMS").write_text(f"{sha(self.disk)}  sda.img\n{'0' * 64}  bad.bin\n{'1' * 64}  missing.bin\n")
        (b / "device-sha256.txt").write_text(f"{sha(self.disk)}  /dev/sda\n")

    def snapshot(self):
        out = {}
        for p in sorted((self.root / "devices").rglob("*")):
            if p.is_file() and p.name != images.SIDECAR:
                st = p.stat()
                out[str(p)] = (st.st_size, st.st_mtime_ns, sha(p.read_bytes()))
        return out


@unittest.skipUnless(HAVE_ZSTD, "zstd not on PATH")
class ImagesTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.fx = Fixture(self.tmp.name)
        self.env = patch.dict(os.environ, {"XDG_CACHE_HOME": str(Path(self.tmp.name) / "cache"), "AI_RE_ROOT": str(self.fx.root)})
        self.env.start()
        self.before = self.fx.snapshot()

    def tearDown(self):
        self.assertEqual(self.before, self.fx.snapshot(), "an image or manifest changed")
        self.env.stop()
        self.tmp.cleanup()

    def run_cli(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = images.main(list(argv))
        text = out.getvalue() + err.getvalue()
        self.assertNotIn(PRIVATE_TEXT, text)
        self.assertNotIn(SECRET_SERIAL, text)
        self.assertNotIn(SECRET_MAC, text)
        return rc, out.getvalue(), err.getvalue()

    def json_cli(self, *argv):
        rc, out, _ = self.run_cli(*argv, "--json")
        return rc, json.loads(out)

    def by_path(self, rows):
        return {Path(r["path"]).relative_to("devices").as_posix(): r for r in rows}

    def test_scan_reports_untracked_missing_and_skips_private(self):
        rc, d = self.json_cli("scan")
        self.assertEqual(rc, 0)
        self.assertEqual([u["path"] for u in d["untracked"]],
                         ["devices/dev-a/dumps/u01/uboot-env.bin", "devices/dev-b/images/untracked.bin"])
        self.assertEqual(d["missing"], ["devices/dev-b/images/missing.bin"])
        self.assertEqual(d["private_files_skipped"], 2)   # units-private.txt is outside the scan dirs
        self.assertEqual(d["devices"]["dev-a"]["raw-removed"], 1)
        rc, d = self.json_cli("list")
        paths = set(self.by_path(d))
        self.assertNotIn("dev-a/dumps/u01/dump-private.bin", paths)
        self.assertNotIn("dev-a/dumps/u01/notes.log", paths)
        self.assertNotIn("dev-b/images/open-jffs2/inner.bin", paths)

    def test_scan_hash_gives_untracked_an_identity(self):
        self.run_cli("scan", "--hash")
        rc, d = self.json_cli("show", "dev-b:images/untracked.bin")
        self.assertEqual(d["sha256"], sha(b"\x01" * 4096))
        self.assertTrue(d["sha_cached"])

    def test_list_classifies_and_filters(self):
        rc, d = self.json_cli("list")
        rows = self.by_path(d)
        z = rows["dev-a/dumps/u01/mtd0.bin.zst"]
        self.assertEqual((z["kind"], z["unit"], z["status"], z["bytes"]), ("flash", "u01", "raw-removed", len(self.fx.flash)))
        self.assertEqual(z["raw_sha256"], sha(self.fx.flash))
        self.assertEqual(rows["dev-a/dumps/u02/factory-os/kernel-1.2.3.4.uImage"]["version"], "1.2.3.4")
        self.assertEqual(rows["dev-a/dumps/u01/board-eeprom.bin"]["kind"], "eeprom")
        self.assertEqual(rows["dev-a/dumps/u01/uboot-env.bin"]["kind"], "env")
        self.assertEqual(rows["dev-b/images/sda.img"]["kind"], "disk")
        self.assertEqual(rows["dev-b/images/sda.img"]["device_sha256"], sha(self.fx.disk))
        rc, d = self.json_cli("list", "--unit", "u02")
        self.assertEqual([r["unit"] for r in d], ["u02"])
        rc, d = self.json_cli("list", "--kind", "disk")
        self.assertEqual(len(d), 1)
        rc, d = self.json_cli("list", "--sort", "size", "-r")
        self.assertEqual(d[0]["path"], "devices/dev-b/images/sda.img")
        rc, out, _ = self.run_cli("list")
        self.assertIn("zst-only", out)

    def test_verify(self):
        rc, d = self.json_cli("verify", "--device", "dev-a")
        self.assertEqual(rc, 0)
        res = {Path(r["path"]).name: r["result"] for r in d}
        self.assertEqual(res["mtd0.bin.zst"], "raw removed, .zst verified")
        rc, d = self.json_cli("verify", "--raw", "dev-a:dumps/u01/mtd0.bin.zst")
        self.assertEqual(rc, 0)
        self.assertIn("decompressed mtd0.bin: sha256 ok", d[0]["checks"][-1])
        self.assertEqual(d[0]["result"], "raw removed, content verified")
        rc, d = self.json_cli("verify", "--device", "dev-b")
        self.assertEqual(rc, 1)
        res = {Path(r["path"]).name: r for r in d}
        self.assertEqual(res["bad.bin"]["result"], "MISMATCH")
        self.assertEqual(res["missing.bin"]["result"], "missing")
        self.assertIn("matches on-device hash", res["sda.img"]["checks"][0])
        rc, _ = self.json_cli("verify", "--status", "missing")
        self.assertEqual(rc, 3)

    def test_tag_note_set_live_in_device_sidecar(self):
        ref = "dev-a:dumps/u01/mtd0.bin.zst"
        self.assertEqual(self.run_cli("tag", ref, "factory", "original-state")[0], 0)
        self.assertEqual(self.run_cli("untag", ref, "factory")[0], 0)
        self.run_cli("note", ref, "read", "via", "socat")
        self.run_cli("set", ref, "--kind", "spi-nor")
        self.assertEqual(self.run_cli("tag", ref, "Bad Tag")[0], 2)
        side = self.fx.root / "devices" / "dev-a" / images.SIDECAR
        rows = [json.loads(l) for l in side.read_text().splitlines()]
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["path"], "dumps/u01/mtd0.bin")
        self.assertEqual(rows[0]["sha256"], sha(self.fx.flash))
        self.assertEqual(rows[0]["tags"], ["original-state"])
        self.assertEqual(rows[0]["kind"], "spi-nor")
        self.assertEqual(rows[0]["notes"][0]["text"], "read via socat")
        rc, d = self.json_cli("list", "--tag", "original-state")
        self.assertEqual([r["kind"] for r in d], ["spi-nor"])
        self.run_cli("set", ref, "--kind", "")
        self.run_cli("untag", ref, "original-state")
        rows = [json.loads(l) for l in side.read_text().splitlines()]
        self.assertNotIn("kind", rows[0])
        self.assertNotIn("tags", rows[0])

    def test_inspect_stores_safe_results(self):
        rc, d = self.json_cli("inspect", "--device", "dev-a")
        self.assertEqual(rc, 0)
        res = {Path(r["path"]).name: r["detect"] for r in d}
        tlv = res["board-eeprom.bin"]["onie_tlv"]
        self.assertEqual(tlv["fields"]["product_name"], "TEST-BOX-48")
        self.assertTrue(tlv["crc_ok"])
        self.assertIn("serial_number", tlv["withheld"])
        self.assertEqual(res["uboot-env.bin"]["uboot_env"]["vars"], 3)
        k = res["kernel-1.2.3.4.uImage"]["uimage"]
        self.assertEqual((k["type"], k["arch"], k["name"]), ("kernel", "arm64", "TEST_Kernel 1.2.3.4@abc"))
        emb = res["mtd0.bin.zst"]["embedded"]
        self.assertEqual({e["type"] for e in emb}, {"uimage", "uboot-env"})
        side = (self.fx.root / "devices" / "dev-a" / images.SIDECAR).read_text()
        self.assertNotIn(SECRET_SERIAL, side)
        self.assertNotIn(SECRET_MAC, side)
        self.assertNotIn("ethaddr", side)
        self.assertIn("TEST-BOX-48", side)

    @unittest.skipUnless(shutil.which("sfdisk"), "sfdisk not on PATH")
    def test_inspect_disk(self):
        rc, d = self.json_cli("inspect", "dev-b:images/sda.img")
        p = d[0]["detect"]["partitions"]
        self.assertEqual(d[0]["detect"]["label"], "dos")
        self.assertEqual((p[0]["n"], p[0]["start"], p[0]["fs"], p[0]["label"]), (1, 2048, "ext4", "testroot"))

    def test_dupes_and_ambiguous_ref(self):
        rc, d = self.json_cli("dupes")
        grp = [g for g in d if g["content_sha256"] == sha(self.fx.kernel)]
        self.assertEqual(len(grp[0]["images"]), 2)
        rc, _, err = self.run_cli("show", sha(self.fx.kernel)[:10])
        self.assertEqual(rc, 2)
        self.assertIn("matches 2 images", err)
        self.assertEqual(self.run_cli("show", "nope.bin")[0], 2)
        rel = os.path.relpath(self.fx.root / "devices/dev-a/dumps/u02/factory-os/kernel-1.2.3.4.uImage")
        rc, d = self.json_cli("show", rel)
        self.assertEqual(d["unit"], "u02")

    @unittest.skipUnless(shutil.which("qemu-img"), "qemu-img not on PATH")
    def test_export_qcow2(self):
        out = Path(self.tmp.name) / "export" / "mtd0.qcow2"
        ref = "dev-a:dumps/u01/mtd0.bin.zst"
        rc, d = self.json_cli("export-qcow2", ref, str(out))
        self.assertEqual(rc, 0)
        self.assertEqual(out.read_bytes()[:4], b"QFI\xfb")
        self.assertTrue(any("--read-only" in c for c in d["attach"]))
        self.assertEqual([p.name for p in out.parent.iterdir()], ["mtd0.qcow2"])
        back = Path(self.tmp.name) / "back.raw"
        subprocess.run(["qemu-img", "convert", "-O", "raw", str(out), str(back)], check=True)
        self.assertEqual(sha(back.read_bytes()), sha(self.fx.flash))
        self.assertEqual(self.run_cli("export-qcow2", ref, str(out))[0], 2)
        inside = self.fx.root / "devices/dev-a/dumps/u01/x.qcow2"
        self.assertEqual(self.run_cli("export-qcow2", ref, str(inside))[0], 2)
        self.assertFalse(inside.exists())


class DetectorTests(unittest.TestCase):
    def test_big_endian_env_and_fdt(self):
        self.assertEqual(images.uboot_env(uboot_env(0x2000, big_endian=True))["size"], 0x2000)
        self.assertIsNone(images.uboot_env(b"\0" * 0x1000))
        self.assertIsNone(images.uimage_header(b"\x27\x05\x19\x56" + b"\0" * 60))   # bad header crc

    def test_zstd_frame_size(self):
        if not HAVE_ZSTD:
            self.skipTest("zstd not on PATH")
        with tempfile.TemporaryDirectory() as td:
            raw, p = Path(td) / "x", Path(td) / "x.zst"
            raw.write_bytes(b"a" * 70000)
            subprocess.run(["zstd", "-q", str(raw), "-o", str(p)], check=True)
            self.assertEqual(images.zstd_frame_size(p), 70000)
            piped = subprocess.run(["zstd", "-q", "-c"], input=b"a" * 70000, capture_output=True, check=True).stdout
            p.write_bytes(piped)
            self.assertIsNone(images.zstd_frame_size(p))   # streamed: size not in the header


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Independently check legacy header/payload CRCs and RAM staging ranges."""
import gzip
import hashlib
import json
import struct
import sys
import zlib
from pathlib import Path

out = Path(sys.argv[1])
report = {"hardware_boot_verified": False,
          "dt_status": "captured vendor DT; modern driver bindings unverified",
          "files": {}}
for name, image_type in [("uImage", 2), ("uInitrd", 3)]:
    blob = (out / name).read_bytes()
    header = bytearray(blob[:64])
    magic, hcrc, timestamp, size, load, entry, dcrc, os_id, arch, typ, comp, label = struct.unpack(
        ">7I4B32s", header)
    assert magic == 0x27051956 and os_id == 5 and arch == 7
    assert typ == image_type and comp == 1
    assert len(blob) == size + 64
    header[4:8] = b"\0" * 4
    assert zlib.crc32(header) == hcrc
    assert zlib.crc32(blob[64:]) == dcrc
    uncompressed = gzip.decompress(blob[64:])
    if name == "uImage":
        assert load == entry == 0
        assert uncompressed == Path(sys.argv[2]).read_bytes()
        assert len(uncompressed) < 0x08000000, "kernel overlaps DT staging"
    else:
        assert uncompressed[:6] in (b"070701", b"070702"), "not a newc initramfs"
    assert len(blob) < 0x08000000, "boot artifact exceeds reserved staging window"
    report["files"][name] = {"bytes": len(blob), "uncompressed_bytes": len(uncompressed),
                            "sha256": hashlib.sha256(blob).hexdigest()}
dtb = (out / "as7760.dtb").read_bytes()
assert struct.unpack(">I", dtb[:4])[0] == 0xD00DFEED
report["files"]["as7760.dtb"] = {"bytes": len(dtb), "sha256": hashlib.sha256(dtb).hexdigest()}
(out / "manifest.json").write_text(json.dumps(report, indent=2) + "\n")

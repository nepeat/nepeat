#!/usr/bin/env python3
"""Extract the captured ONIE updater/FIT as data; never execute installer code.

Regular files only: archive symlinks and device nodes are inventoried, not created.
Output belongs under ignored analysis/. Existing output must be trusted.
"""
import argparse
import gzip
import hashlib
import io
import json
import lzma
from pathlib import Path
import stat
import struct
import tarfile
import zlib


def regular_file(root, name, data):
    path = root / name.lstrip("/")
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"unsafe archive path: {name}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def unpack_tar(data, root):
    inventory = []
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        for member in archive:
            inventory.append({"name": member.name, "size": member.size,
                              "type": member.type.decode(), "link": member.linkname})
            if member.isfile():
                regular_file(root, member.name, archive.extractfile(member).read())
    return inventory


def fdt_nodes(data):
    magic, total, off_struct, off_strings, _, _, _, _, size_strings, size_struct = struct.unpack_from(">10I", data)
    assert magic == 0xD00DFEED and total <= len(data)
    strings = data[off_strings:off_strings + size_strings]
    pos = off_struct
    stack = []
    nodes = {}
    while pos < off_struct + size_struct:
        token, = struct.unpack_from(">I", data, pos)
        pos += 4
        if token == 1:
            end = data.index(b"\0", pos)
            stack.append(data[pos:end].decode())
            nodes["/".join(stack) or "/"] = {}
            pos = (end + 4) & ~3
        elif token == 2:
            stack.pop()
        elif token == 3:
            size, nameoff = struct.unpack_from(">II", data, pos)
            pos += 8
            name = strings[nameoff:strings.index(b"\0", nameoff)].decode()
            nodes["/".join(stack) or "/"][name] = data[pos:pos + size]
            pos = (pos + size + 3) & ~3
        elif token == 4:
            pass
        elif token == 9:
            return nodes
        else:
            raise ValueError(f"bad FDT token {token}")
    raise ValueError("missing FDT end")


def unpack_cpio(data, root):
    pos = 0
    inventory = []
    while pos + 110 <= len(data):
        assert data[pos:pos + 6] in (b"070701", b"070702"), (pos, data[pos:pos + 6])
        fields = [int(data[pos + 6 + i * 8:pos + 14 + i * 8], 16) for i in range(13)]
        mode, size, namesize = fields[1], fields[6], fields[11]
        name = data[pos + 110:pos + 110 + namesize - 1].decode()
        pos = (pos + 110 + namesize + 3) & ~3
        body = data[pos:pos + size]
        assert len(body) == size
        pos = (pos + size + 3) & ~3
        if name == "TRAILER!!!":
            break
        entry = {"name": name, "mode": oct(mode), "size": size}
        if stat.S_ISREG(mode):
            regular_file(root, name, body)
        elif stat.S_ISLNK(mode):
            entry["link"] = body.decode()
        inventory.append(entry)
    return inventory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("updater", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    data = args.updater.read_bytes()
    offset = data.index(b"\nexit_marker\n") + len(b"\nexit_marker\n")
    payload = data[offset:]
    expected = data[:offset].decode().split("payload_sha1=")[1].splitlines()[0].strip("\"\x27")
    assert hashlib.sha1(payload).hexdigest() == expected
    out = args.output
    out.mkdir(parents=True, exist_ok=True)
    report = {"updater_sha256": hashlib.sha256(data).hexdigest(), "payload_sha1": expected}
    report["installer"] = unpack_tar(payload, out)
    report["update"] = unpack_tar((out / "installer/onie-update.tar.xz").read_bytes(), out / "update")
    nodes = fdt_nodes((out / "update/onie.itb").read_bytes())
    report["fit"] = {}
    for path, props in nodes.items():
        meta = {k: (v.rstrip(b"\0").decode() if v and any(32 <= c < 127 for c in v) and all(32 <= c < 127 or c == 0 for c in v) else v.hex())
                for k, v in props.items() if k != "data"}
        if "data" in props:
            blob = props["data"]
            meta.update(size=len(blob), sha256=hashlib.sha256(blob).hexdigest())
            regular_file(out / "fit", path.removeprefix("/images/"), blob)
            for hp, hprops in nodes.items():
                if hp.startswith(path + "/hash"):
                    algo = hprops["algo"].rstrip(b"\0").decode()
                    got = (struct.pack(">I", zlib.crc32(blob)) if algo == "crc32"
                           else hashlib.new(algo, blob).digest())
                    assert got == hprops["value"], hp
            if props.get("type") == b"ramdisk\0":
                report["rootfs"] = unpack_cpio((lzma.decompress(blob) if blob.startswith(b"\xfd7zXZ") else gzip.decompress(blob)), out / "rootfs")
        report["fit"][path] = meta
    (out / "inventory.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k not in ("rootfs", "installer", "update")}, indent=2))
    print("rootfs entries:", len(report.get("rootfs", [])))


if __name__ == "__main__":
    main()

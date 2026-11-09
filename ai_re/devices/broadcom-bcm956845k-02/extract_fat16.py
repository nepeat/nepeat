#!/usr/bin/env python3
"""Extract regular root-directory files from the acquired VxWorks FAT16 volume."""

import argparse
import hashlib
import json
import mmap
from pathlib import Path
import struct


def extract(image, destination):
    u16 = lambda offset: struct.unpack_from("<H", image, offset)[0]
    bps = u16(11)
    spc = image[13]
    reserved, fats, entries, fat_sectors = u16(14), image[16], u16(17), u16(22)
    if bps != 512 or not spc or image[54:62] != b"FAT16   ":
        raise ValueError("expected FAT16 with 512-byte sectors")
    fat_base = reserved * bps
    root_base = (reserved + fats * fat_sectors) * bps
    data_base = root_base + ((entries * 32 + bps - 1) // bps) * bps
    cluster_bytes = spc * bps
    destination.mkdir(exist_ok=True)
    manifest = []
    longparts = {}
    for index in range(entries):
        entry = image[root_base + index * 32:root_base + (index + 1) * 32]
        if entry[0] == 0:
            break
        if entry[0] == 0xE5:
            longparts = {}
            continue
        attr = entry[11]
        if attr == 0x0F:
            longparts[entry[0] & 31] = entry[1:11] + entry[14:26] + entry[28:32]
            continue
        if attr & 0x18:
            longparts = {}
            continue
        if longparts:
            name = b"".join(longparts[key] for key in sorted(longparts)).decode("utf-16-le")
            name = name.split("\0", 1)[0].rstrip("\uffff")
        else:
            name = entry[:8].decode("ascii").rstrip()
            extension = entry[8:11].decode("ascii").rstrip()
            if extension:
                name += "." + extension
        longparts = {}
        if Path(name).name != name or name in (".", "..") or "\\" in name:
            raise ValueError("unsafe filename in image")
        cluster, size = struct.unpack_from("<HI", entry, 26)
        remaining = size
        output = bytearray()
        seen = set()
        while remaining:
            if cluster < 2 or cluster >= 0xFFF0 or cluster in seen:
                raise ValueError(f"invalid or cyclic cluster chain for {name}")
            seen.add(cluster)
            offset = data_base + (cluster - 2) * cluster_bytes
            length = min(cluster_bytes, remaining)
            if offset + length > len(image) or cluster * 2 >= fat_sectors * bps:
                raise ValueError("cluster outside acquired image/FAT")
            output.extend(image[offset:offset + length])
            remaining -= length
            cluster = u16(fat_base + cluster * 2)
        path = destination / name
        with path.open("xb") as stream:
            stream.write(output)
        record = {"file": name, "bytes": size, "sha256": hashlib.sha256(output).hexdigest()}
        manifest.append(record)
        print(json.dumps(record))
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    with args.image.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as image:
        extract(image, args.destination)

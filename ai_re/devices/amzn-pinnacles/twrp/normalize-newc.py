#!/usr/bin/env python3
"""Normalize filesystem-dependent inode fields in a newc cpio stream."""

import sys


archive = bytearray(sys.stdin.buffer.read())
offset = 0
inode_map = {}
next_inode = 1

while offset < len(archive):
    if archive[offset : offset + 6] != b"070701":
        raise SystemExit(f"invalid newc magic at offset {offset}")

    def field(index: int) -> int:
        start = offset + 6 + index * 8
        return int(archive[start : start + 8], 16)

    old_inode = field(0)
    file_size = field(6)
    dev_major = field(7)
    dev_minor = field(8)
    name_size = field(11)
    key = (dev_major, dev_minor, old_inode)
    if key not in inode_map:
        inode_map[key] = next_inode
        next_inode += 1

    archive[offset + 6 : offset + 14] = f"{inode_map[key]:08x}".encode()
    archive[offset + 62 : offset + 70] = b"00000000"
    archive[offset + 70 : offset + 78] = b"00000000"

    name_start = offset + 110
    name_end = name_start + name_size
    data_start = (name_end + 3) & ~3
    offset = (data_start + file_size + 3) & ~3

    if archive[name_start : name_end - 1] == b"TRAILER!!!":
        break

if offset > len(archive):
    raise SystemExit("truncated newc archive")

sys.stdout.buffer.write(archive)

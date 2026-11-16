#!/usr/bin/env python3
"""Offline map of the CS8210 boot SPI (spi0.0, 32 MiB) from the per-mtd dumps.

Assembles mtd0..mtd4 at their DT offsets, then prints blank (0xff / 0x00) runs
of >= 16 KiB below a limit, every 4 KiB-aligned al-boot object header (also
those the TOC does not list), and the ONIE FIT tail. Prints no object
contents. Object/TOC parsing itself stays in nix/packages/alboot (use
`alboot.py list <full image>` for the TOC view); this only adds the map.

  spi_freemap.py <dump dir with mtd0-uboot.bin ... mtd4-onie.bin> [limit_hex]
"""
import os
import struct
import sys

PARTS = [("mtd0-uboot.bin", 0x0), ("mtd1-uboot-env.bin", 0x380000),
         ("mtd2-unused.bin", 0x3C0000), ("mtd3-board_eeprom.bin", 0x400000),
         ("mtd4-onie.bin", 0x410000)]
SIZE = 0x2000000
OBJ_MAGIC = 0x000B9EC7


def assemble(d):
    img = bytearray(b"\xff" * SIZE)
    for name, off in PARTS:
        b = open(os.path.join(d, name), "rb").read()
        img[off:off + len(b)] = b
    return bytes(img)


def runs(img, val, lim, minlen=0x4000):
    out, start, blk = [], None, bytes([val]) * 0x1000
    for o in range(0, lim, 0x1000):
        e = img[o:o + 0x1000] == blk
        if e and start is None:
            start = o
        if not e and start is not None:
            out.append((start, o))
            start = None
    if start is not None:
        out.append((start, lim))
    return [(a, b) for a, b in out if b - a >= minlen]


def headers(img, lim):
    for o in range(0, lim, 0x1000):
        if struct.unpack_from("<I", img, o)[0] != OBJ_MAGIC:
            continue
        h = img[o:o + 0x48]
        oid, ma, mi, su = struct.unpack_from("<4I", h, 8)
        size, load, ex = struct.unpack_from("<3Q", h, 0x28)
        hok = (sum(h[:0x44]) & 0xFFFFFFFF) == struct.unpack_from("<I", h, 0x44)[0]
        dok = None
        if o + 0x4C + size <= len(img):
            dok = (sum(img[o + 0x48:o + 0x48 + size]) & 0xFFFFFFFF) == \
                struct.unpack_from("<I", img, o + 0x48 + size)[0]
        yield o, oid, (ma, mi, su), size, load, ex, hok, dok


def main(argv):
    img = assemble(argv[0])
    lim = int(argv[1], 16) if len(argv) > 1 else 0x410000
    for v in (0xFF, 0x00):
        print(f"blank {v:02x} runs below {lim:#x}:")
        for a, b in runs(img, v, lim):
            print(f"  {a:#09x}-{b:#09x}  {(b - a) // 1024} KiB")
    print("object headers (4 KiB aligned):")
    for o, oid, v, size, load, ex, hok, dok in headers(img, lim):
        print(f"  {o:#09x} id {oid:#x} v{v[0]}.{v[1]}.{v[2]} size {size:#x} load {load:#x} "
              f"exec {ex:#x} hdr {'ok' if hok else 'BAD'} data {'ok' if dok else 'BAD'} "
              f"end {o + 0x4C + size:#x}")
    if struct.unpack_from(">I", img, 0x410000)[0] == 0xD00DFEED:
        fit = struct.unpack_from(">I", img, 0x410004)[0]
        e = SIZE
        while e > 0x410000 and img[e - 0x1000:e] == b"\xff" * 0x1000:
            e -= 0x1000
        print(f"ONIE FIT @0x410000 totalsize {fit:#x} (ends {0x410000 + fit:#x}); "
              f"blank tail {e:#x}-{SIZE:#x} = {(SIZE - e) // 1024} KiB (erased by any ONIE update)")


if __name__ == "__main__":
    main(sys.argv[1:])

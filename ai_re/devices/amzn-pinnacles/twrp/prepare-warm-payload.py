#!/usr/bin/env python3
"""Split and validate yacht's TWRP image for a future RAM-only warm boot.

This tool only reads the recovery image and writes ordinary host files. It does
not talk to the tablet, patch a partition, or produce jumping kernel code.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import zlib
from pathlib import Path


ANDROID_MAGIC = b"ANDROID!"
FDT_MAGIC = b"\xd0\x0d\xfe\xed"
ARM64_IMAGE_MAGIC = b"ARM\x64"
EXPECTED_RECOVERY_SHA256 = (
    "23fd505f2a27f7191d8daa3866a0ed2500f9882aef68e4606a452142c5f58b27"
)
EXPECTED_KERNEL_SHA256 = (
    "4852ece1022ff2d6ad41e746dad1e7448a3e3c2afca4036d62f4cc2e5fc1a463"
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def align(value: int, boundary: int) -> int:
    return (value + boundary - 1) // boundary * boundary


def u32le(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def split_dtbs(data: bytes) -> list[bytes]:
    dtbs: list[bytes] = []
    offset = 0

    while offset < len(data):
        if data[offset : offset + 4] != FDT_MAGIC:
            raise ValueError(f"unexpected data at appended-DTB offset 0x{offset:x}")
        if len(data) - offset < 8:
            raise ValueError("truncated FDT header")
        size = struct.unpack_from(">I", data, offset + 4)[0]
        if size < 40 or offset + size > len(data):
            raise ValueError(f"invalid FDT size 0x{size:x} at offset 0x{offset:x}")
        dtbs.append(data[offset : offset + size])
        offset += size

    return dtbs


def fdt_root_strings(dtb: bytes) -> dict[str, str]:
    """Return selected string properties from the FDT root node."""
    if len(dtb) < 40 or dtb[:4] != FDT_MAGIC:
        raise ValueError("invalid FDT header")
    fields = struct.unpack_from(">10I", dtb, 0)
    struct_offset = fields[2]
    strings_offset = fields[3]
    strings_size = fields[8]
    struct_size = fields[9]
    structure = dtb[struct_offset : struct_offset + struct_size]
    strings = dtb[strings_offset : strings_offset + strings_size]
    if len(structure) != struct_size or len(strings) != strings_size:
        raise ValueError("truncated FDT blocks")

    wanted = {"model", "compatible", "version"}
    result: dict[str, str] = {}
    offset = 0
    depth = 0
    while offset + 4 <= len(structure):
        token = struct.unpack_from(">I", structure, offset)[0]
        offset += 4
        if token == 1:  # FDT_BEGIN_NODE
            end = structure.find(b"\0", offset)
            if end < 0:
                raise ValueError("unterminated FDT node name")
            offset = align(end + 1, 4)
            depth += 1
        elif token == 2:  # FDT_END_NODE
            depth -= 1
        elif token == 3:  # FDT_PROP
            if offset + 8 > len(structure):
                raise ValueError("truncated FDT property")
            length, name_offset = struct.unpack_from(">II", structure, offset)
            offset += 8
            value = structure[offset : offset + length]
            offset = align(offset + length, 4)
            name_end = strings.find(b"\0", name_offset)
            if name_end < 0:
                raise ValueError("unterminated FDT property name")
            name = strings[name_offset:name_end].decode("ascii")
            if depth == 1 and name in wanted:
                result[name] = value.rstrip(b"\0").replace(b"\0", b",").decode(
                    "ascii", errors="replace"
                )
        elif token == 4:  # FDT_NOP
            continue
        elif token == 9:  # FDT_END
            break
        else:
            raise ValueError(f"unknown FDT token {token}")
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recovery", type=Path, help="validated TWRP recovery.img")
    parser.add_argument("out", type=Path, help="new output directory")
    parser.add_argument(
        "--allow-unknown-image",
        action="store_true",
        help="accept a recovery hash other than yacht's validated build",
    )
    parser.add_argument(
        "--dtb-index",
        type=int,
        help="also emit selected.dtb; do not guess this value",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    recovery = args.recovery.read_bytes()
    recovery_hash = sha256(recovery)
    if not args.allow_unknown_image and recovery_hash != EXPECTED_RECOVERY_SHA256:
        raise SystemExit(
            "refusing unvalidated recovery image: "
            f"sha256={recovery_hash}; use --allow-unknown-image only deliberately"
        )

    if len(recovery) < 1648 or recovery[:8] != ANDROID_MAGIC:
        raise SystemExit("not an Android boot image v1")

    kernel_size = u32le(recovery, 8)
    kernel_addr = u32le(recovery, 12)
    ramdisk_size = u32le(recovery, 16)
    ramdisk_addr = u32le(recovery, 20)
    second_size = u32le(recovery, 24)
    second_addr = u32le(recovery, 28)
    tags_addr = u32le(recovery, 32)
    page_size = u32le(recovery, 36)
    header_version = u32le(recovery, 40)
    header_size = u32le(recovery, 1644)

    if header_version != 1 or header_size != 1648:
        raise SystemExit(
            f"unsupported boot header: version={header_version}, size={header_size}"
        )
    if page_size != 2048 or second_size != 0:
        raise SystemExit(
            f"unexpected layout: page_size={page_size}, second_size={second_size}"
        )

    kernel_offset = page_size
    ramdisk_offset = align(kernel_offset + kernel_size, page_size)
    image_end = ramdisk_offset + ramdisk_size
    if image_end > len(recovery):
        raise SystemExit("boot image payload is truncated")

    kernel_blob = recovery[kernel_offset : kernel_offset + kernel_size]
    ramdisk = recovery[ramdisk_offset:image_end]
    kernel_hash = sha256(kernel_blob)
    if not args.allow_unknown_image and kernel_hash != EXPECTED_KERNEL_SHA256:
        raise SystemExit(f"embedded kernel hash mismatch: {kernel_hash}")

    inflater = zlib.decompressobj(16 + zlib.MAX_WBITS)
    image = inflater.decompress(kernel_blob) + inflater.flush()
    if not inflater.eof or not inflater.unused_data:
        raise SystemExit("kernel is not gzip(Image) followed by appended DTBs")
    if len(image) < 0x40 or image[0x38:0x3C] != ARM64_IMAGE_MAGIC:
        raise SystemExit("decompressed kernel lacks the arm64 Image magic")

    dtbs = split_dtbs(inflater.unused_data)
    if len(dtbs) != 4:
        raise SystemExit(f"expected four appended DTBs, found {len(dtbs)}")
    if args.dtb_index is not None and not 0 <= args.dtb_index < len(dtbs):
        raise SystemExit(f"DTB index must be between 0 and {len(dtbs) - 1}")

    manifest = {
        "source": {
            "path": str(args.recovery),
            "size": len(recovery),
            "sha256": recovery_hash,
        },
        "boot_header": {
            "version": header_version,
            "page_size": page_size,
            "kernel_address": f"0x{kernel_addr:08x}",
            "ramdisk_address": f"0x{ramdisk_addr:08x}",
            "second_address": f"0x{second_addr:08x}",
            "tags_address": f"0x{tags_addr:08x}",
        },
        "image": {"size": len(image), "sha256": sha256(image)},
        "ramdisk": {"size": len(ramdisk), "sha256": sha256(ramdisk)},
        "kernel_blob": {"size": len(kernel_blob), "sha256": kernel_hash},
        "dtbs": [
            {
                "index": index,
                "size": len(dtb),
                "sha256": sha256(dtb),
                "identity": fdt_root_strings(dtb),
            }
            for index, dtb in enumerate(dtbs)
        ],
        "selected_dtb": args.dtb_index,
    }

    args.out.mkdir(parents=True, exist_ok=False)
    (args.out / "Image").write_bytes(image)
    (args.out / "ramdisk.gz").write_bytes(ramdisk)
    dtb_dir = args.out / "dtbs"
    dtb_dir.mkdir()
    for index, dtb in enumerate(dtbs):
        (dtb_dir / f"{index}.dtb").write_bytes(dtb)
    if args.dtb_index is not None:
        (args.out / "selected.dtb").write_bytes(dtbs[args.dtb_index])
    (args.out / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )

    print(f"validated {args.recovery}")
    print(f"wrote {len(image)}-byte arm64 Image, ramdisk and {len(dtbs)} DTBs")
    for index, dtb in enumerate(dtbs):
        identity = fdt_root_strings(dtb)
        print(f"dtb[{index}]: version={identity.get('version', 'unknown')}")
    if args.dtb_index is None:
        print("no DTB selected; compare against the running device before choosing")


if __name__ == "__main__":
    main()

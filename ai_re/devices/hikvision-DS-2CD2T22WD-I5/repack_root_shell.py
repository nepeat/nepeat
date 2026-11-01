#!/usr/bin/env python3
"""Patch the matching R6 firmware so SSH admin sessions start BusyBox sh."""

from __future__ import annotations

import argparse
import hashlib
import struct
import zlib
from pathlib import Path

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes


XOR_KEY = bytes.fromhex("bacdbcfed6caddd3bab9a3abbfcbb5be")
HEADER_SEED = bytes.fromhex(
    "b64ec50866c49b757f1b2726a3755f2252abdfe9fbbb151e247d0d806ae425db"
)
FILE_SEED = bytes.fromhex(
    "bddc070b81fc4e47d03a7ed04c5b58d072e7215d4ad4634754b2e5b68732bd37"
)
TARGET_NAME = b"initrun.sh"
COMMENT_PREFIX = b"#/home/modules/load_audio_drv.sh"
SHELL_COMMAND = b"sed -i s,/bin/psh,/bin/sh, /etc/passwd"


def xor16(data: bytes) -> bytes:
    return bytes(byte ^ XOR_KEY[(i + (i >> 4)) & 0xF] for i, byte in enumerate(data))


def derive_key(seed: bytes) -> bytes:
    out = bytearray(32)
    for i, byte in enumerate(seed):
        value = i * i + byte * byte + byte % 32 + byte * 32 * i
        out[i] = (value & 0xFF) ^ byte
    return bytes(out)


def aes_ecb(data: bytes, seed: bytes, *, encrypt: bool) -> bytes:
    cipher = Cipher(algorithms.AES(derive_key(seed)), modes.ECB())
    worker = cipher.encryptor() if encrypt else cipher.decryptor()
    return worker.update(data) + worker.finalize()


def transform_payload(data: bytes, *, encrypt: bool) -> bytes:
    full = len(data) // 16 * 16
    return aes_ecb(data[:full], FILE_SEED, encrypt=encrypt) + data[full:]


def solve_crc_patch(data: bytes, offset: int, target: int) -> bytes | None:
    base = bytearray(data)
    base[offset : offset + 4] = b"\0" * 4
    base_crc = zlib.crc32(base) & 0xFFFFFFFF
    basis: list[tuple[int, int] | None] = [None] * 32

    for source_bit in range(32):
        trial = bytearray(base)
        trial[offset + source_bit // 8] ^= 1 << (source_bit % 8)
        value = (zlib.crc32(trial) & 0xFFFFFFFF) ^ base_crc
        mask = 1 << source_bit
        for output_bit in range(31, -1, -1):
            if not value & (1 << output_bit):
                continue
            if basis[output_bit] is None:
                basis[output_bit] = value, mask
                break
            value ^= basis[output_bit][0]
            mask ^= basis[output_bit][1]

    value = target ^ base_crc
    mask = 0
    for output_bit in range(31, -1, -1):
        if not value & (1 << output_bit):
            continue
        if basis[output_bit] is None:
            return None
        value ^= basis[output_bit][0]
        mask ^= basis[output_bit][1]
    if value:
        return None
    return bytes((mask >> (8 * i)) & 0xFF for i in range(4))


def add_printable_crc_patch(data: bytearray, target: int) -> tuple[int, bytes]:
    line_offset = 0
    for line in data.splitlines(keepends=True):
        if line.startswith(b"#") and not line.startswith(COMMENT_PREFIX):
            for relative in range(1, len(line) - 4):
                patch = solve_crc_patch(data, line_offset + relative, target)
                if patch is not None and all(0x20 <= byte <= 0x7E for byte in patch):
                    offset = line_offset + relative
                    data[offset : offset + 4] = patch
                    return offset, patch
        line_offset += len(line)
    raise RuntimeError("could not place a printable CRC compensation in a comment")


def find_entry(header: bytes) -> tuple[int, int, int]:
    for offset in range(0xF0, len(header) - 0x40 + 1, 0x40):
        name = header[offset : offset + 0x20].split(b"\0", 1)[0]
        if name == TARGET_NAME:
            relative, length, checksum = struct.unpack_from("<III", header, offset + 0x20)
            return relative, length, checksum
    raise RuntimeError("initrun.sh entry not found in the HK30 header")


def patch_firmware(source: Path, output: Path) -> None:
    raw = bytearray(source.read_bytes())
    outer = xor16(raw[:0x6C])
    if outer[:4] != b"02KH" or struct.unpack_from("<I", outer, 8)[0] != 0x6C:
        raise RuntimeError("expected the R6 HK20 outer header")

    hk30_wrapper = xor16(raw[0x6C : 0x7C])
    if hk30_wrapper[:4] != b"03KH":
        raise RuntimeError("expected an HK30 inner header")
    header_length = struct.unpack_from("<I", hk30_wrapper, 8)[0]
    header_start = 0x7C
    encrypted_header = bytes(raw[header_start : header_start + header_length])
    header = aes_ecb(encrypted_header, HEADER_SEED, encrypt=False)

    relative, length, stored_checksum = find_entry(header)
    payload_offset = 0x6C + relative
    original_payload = bytes(raw[payload_offset : payload_offset + length])
    plaintext = bytearray(transform_payload(original_payload, encrypt=False))
    if transform_payload(plaintext, encrypt=True) != original_payload:
        raise RuntimeError("payload AES round trip did not reproduce the official firmware")
    if (~zlib.crc32(plaintext) & 0xFFFFFFFF) != stored_checksum:
        raise RuntimeError("official initrun.sh checksum does not match")

    old_line = next(
        (line for line in plaintext.splitlines(keepends=True) if line.startswith(COMMENT_PREFIX)),
        None,
    )
    if old_line is None:
        raise RuntimeError("expected initrun.sh insertion comment not found")
    padding = len(old_line) - 1 - len(SHELL_COMMAND) - 2
    if padding < 0:
        raise RuntimeError("insertion comment is too short")
    new_line = SHELL_COMMAND + b" #" + b"X" * padding + b"\n"
    line_offset = plaintext.find(old_line)
    plaintext[line_offset : line_offset + len(old_line)] = new_line

    target_crc = (~stored_checksum) & 0xFFFFFFFF
    patch_offset, crc_patch = add_printable_crc_patch(plaintext, target_crc)
    if zlib.crc32(plaintext) & 0xFFFFFFFF != target_crc:
        raise RuntimeError("CRC compensation failed")

    modified_payload = transform_payload(plaintext, encrypt=True)
    raw[payload_offset : payload_offset + length] = modified_payload
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(raw)

    verify_plaintext = transform_payload(
        bytes(raw[payload_offset : payload_offset + length]), encrypt=False
    )
    if SHELL_COMMAND not in verify_plaintext:
        raise RuntimeError("output round trip lost the shell command")
    if (~zlib.crc32(verify_plaintext) & 0xFFFFFFFF) != stored_checksum:
        raise RuntimeError("output round trip lost the original checksum")

    changed = sum(a != b for a, b in zip(source.read_bytes(), raw))
    print(f"source sha256: {hashlib.sha256(source.read_bytes()).hexdigest()}")
    print(f"output sha256: {hashlib.sha256(raw).hexdigest()}")
    print(f"initrun entry: offset={payload_offset} length={length} crc=0x{stored_checksum:08x}")
    print(f"printable CRC patch: offset={patch_offset} bytes={crc_patch!r}")
    print(f"changed encrypted bytes: {changed}")
    print(f"wrote {output}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "source",
        nargs="?",
        type=Path,
        default=Path("firmware/v5.5.82/digicap.dav"),
    )
    parser.add_argument(
        "output",
        nargs="?",
        type=Path,
        default=Path("firmware/v5.5.82/digicap-root-shell.dav"),
    )
    args = parser.parse_args()
    patch_firmware(args.source, args.output)


if __name__ == "__main__":
    main()

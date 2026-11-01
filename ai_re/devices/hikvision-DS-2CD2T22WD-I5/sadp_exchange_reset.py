#!/usr/bin/env python3
"""Reset this R6 camera through SADP's client-key exchange mode."""

import argparse
import base64
import hashlib
import os
import socket
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


GROUP = "239.255.255.250"
PORT = 37020


def transact(
    sock: socket.socket,
    mac: str,
    kind: str,
    fields: dict[str, str],
    camera_ip: str,
) -> ET.Element:
    request_id = str(uuid.uuid4()).upper()
    root = ET.Element("Probe")
    ET.SubElement(root, "Uuid").text = request_id
    ET.SubElement(root, "MAC").text = mac
    ET.SubElement(root, "Types").text = kind
    for name, value in fields.items():
        ET.SubElement(root, name).text = value
    packet = b'<?xml version="1.0" encoding="utf-8"?>' + ET.tostring(root)

    destination = (camera_ip, PORT)
    deadline = time.monotonic() + 8
    sock.sendto(packet, destination)
    while time.monotonic() < deadline:
        try:
            response, peer = sock.recvfrom(65535)
        except TimeoutError:
            continue
        parsed = ET.fromstring(response)
        if parsed.findtext("Uuid") == request_id:
            print(f"{kind}: {parsed.findtext('Result')} from {peer[0]}")
            return parsed
    raise TimeoutError(f"camera did not answer SADP {kind}")


def encrypt_password(challenge: bytes, password: str) -> str:
    encoded = password.encode()
    if not 1 <= len(encoded) <= 16:
        raise ValueError("password must be 1-16 bytes")
    key = challenge[:16]
    plaintext = key + encoded.ljust(16, b"\0")
    encryptor = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return base64.b64encode(encryptor.update(plaintext) + encryptor.finalize()).decode()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--local-ip", required=True)
    parser.add_argument("--camera-ip", default="192.168.20.207")
    parser.add_argument("--mac", required=True)
    parser.add_argument("--password", required=True)
    parser.add_argument("--credentials", type=Path)
    parser.add_argument("--challenge", help="resume with a previously decrypted 32-byte challenge")
    parser.add_argument("--reset-code", help="override the derived eight-character reset code")
    args = parser.parse_args()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((args.local_ip, PORT))
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(args.local_ip))
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sock.settimeout(0.25)

    if args.challenge:
        challenge = args.challenge.encode()
    else:
        private_key = rsa.generate_private_key(public_exponent=65537, key_size=1024)
        public_der = private_key.public_key().public_bytes(Encoding.DER, PublicFormat.PKCS1)
        exchange = transact(
            sock,
            args.mac,
            "exchangecode",
            {"Code": base64.b64encode(public_der).decode()},
            args.camera_ip,
        )
        if exchange.findtext("Result") != "success":
            return 1
        ciphertext = base64.b64decode(exchange.findtext("Code"), validate=True)
        challenge = private_key.decrypt(ciphertext, padding.PKCS1v15())
    if len(challenge) != 32 or any(byte not in b"0123456789abcdef" for byte in challenge):
        raise ValueError(f"unexpected exchange challenge: {challenge!r}")

    reset_code = args.reset_code or hashlib.md5(challenge).hexdigest()[:8]
    encrypted_password = encrypt_password(challenge, args.password)
    print(f"challenge: {challenge.decode()}")
    print(f"derived reset code: {reset_code}")
    reset = transact(
        sock,
        args.mac,
        "reset",
        {"Code": reset_code, "Password": encrypted_password},
        args.camera_ip,
    )
    if reset.findtext("Result") != "success":
        print(ET.tostring(reset, encoding="unicode"))
        return 1

    print(f"accepted reset code: {reset_code}")
    if args.credentials:
        args.credentials.parent.mkdir(parents=True, exist_ok=True)
        args.credentials.write_text(f"username=admin\npassword={args.password}\n")
        os.chmod(args.credentials, 0o600)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Request Hikvision's password-reset challenge over SADP."""

import argparse
import socket
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path


GROUP = "239.255.255.250"
PORT = 37020


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--local-ip", required=True)
    parser.add_argument("--camera-ip", default="192.168.20.207")
    parser.add_argument("--mac", required=True)
    parser.add_argument("--type", default="getcode")
    parser.add_argument("--timeout", type=float, default=5.0)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    request_id = str(uuid.uuid4()).upper()
    probe = (
        '<?xml version="1.0" encoding="utf-8"?>'
        f"<Probe><Uuid>{request_id}</Uuid><MAC>{args.mac}</MAC>"
        f"<Types>{args.type}</Types></Probe>"
    ).encode()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((args.local_ip, PORT))
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(args.local_ip))
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sock.settimeout(0.25)

    sock.sendto(probe, (args.camera_ip, PORT))
    print(probe.decode())
    deadline = time.monotonic() + args.timeout
    found = False
    while time.monotonic() < deadline:
        try:
            response, peer = sock.recvfrom(65535)
        except TimeoutError:
            continue
        text = response.decode(errors="replace")
        if request_id in text or args.type.lower() in text.lower():
            print(f"\n{peer[0]}:{peer[1]}\n{text}")
            if args.output:
                code = ET.fromstring(text).findtext("Code")
                if code:
                    args.output.parent.mkdir(parents=True, exist_ok=True)
                    args.output.write_text(code)
            found = True
    return 0 if found else 1


if __name__ == "__main__":
    raise SystemExit(main())

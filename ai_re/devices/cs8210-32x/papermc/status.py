#!/usr/bin/env python3
"""Read-only Minecraft Java status probe."""
import json
import socket
import struct
import sys

def varint(n):
    result = bytearray()
    while True:
        b = n & 127
        n >>= 7
        result.append(b | (128 if n else 0))
        if not n:
            return bytes(result)

def read_varint(s):
    n = 0
    for i in range(5):
        b = s.recv(1)
        if not b:
            raise EOFError("connection closed")
        n |= (b[0] & 127) << (7*i)
        if not b[0] & 128:
            return n
    raise ValueError("oversized varint")

host = sys.argv[1] if len(sys.argv) > 1 else "10.60.2.190"
with socket.create_connection((host, 25565), timeout=15) as s:
    data = b"\x00" + varint(772) + varint(len(host)) + host.encode() + struct.pack(">H", 25565) + b"\x01"
    s.sendall(varint(len(data)) + data + b"\x01\x00")
    read_varint(s)
    assert read_varint(s) == 0
    size = read_varint(s)
    data = bytearray()
    while len(data) < size:
        chunk = s.recv(size-len(data))
        if not chunk:
            raise EOFError("truncated response")
        data.extend(chunk)
    print(json.dumps(json.loads(data), indent=2))

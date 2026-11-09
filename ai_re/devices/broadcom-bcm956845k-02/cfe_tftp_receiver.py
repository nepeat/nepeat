#!/usr/bin/env python3
"""Receive CFE flash backups, including its nonstandard initial DATA block 0.

Bind an unprivileged port and redirect only the target's UDP/69 traffic to it.
Accepts uploads only from --source, with plain basename filenames. No downloads.
Use chunks smaller than 65535 negotiated blocks to avoid rollover ambiguity.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import struct
import time


def packet(opcode, block):
    return struct.pack("!HH", opcode, block)


def receive(listener, request, peer, root, bind):
    fields = request[2:].split(b"\0")
    name, mode = fields[:2]
    name = name.decode("ascii")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+\.bin", name) or mode.lower() != b"octet":
        raise ValueError("only octet uploads with plain .bin basenames are accepted")
    destination = root / name
    partial = root / (name + ".partial")
    if destination.exists():
        raise ValueError("destination already exists")
    options = dict(zip(fields[2:-1:2], fields[3:-1:2]))
    blocksize = min(int(options.get(b"blksize", b"512")), 1428)
    if not 8 <= blocksize <= 1428:
        raise ValueError("invalid block size")
    reply = b"\0\6blksize\0" + str(blocksize).encode() + b"\0" if b"blksize" in options else packet(4, 0)
    digest = hashlib.sha256()
    total = 0
    first = None
    expected = None
    started = time.monotonic()
    print(f"WRQ {peer} {name} blksize={blocksize}", flush=True)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as transfer, partial.open("xb") as output:
        transfer.bind((bind, 0))
        transfer.settimeout(2)
        transfer.sendto(reply, peer)
        retries = 0
        while True:
            try:
                data, sender = transfer.recvfrom(65535)
            except socket.timeout:
                retries += 1
                if retries > 10:
                    raise TimeoutError("transfer stalled; partial file retained")
                transfer.sendto(reply, peer)
                continue
            if sender != peer or len(data) < 4:
                continue
            opcode, block = struct.unpack("!HH", data[:4])
            if opcode == 5:
                raise ValueError("client returned TFTP error")
            if opcode != 3:
                continue
            payload = data[4:]
            if len(payload) > blocksize:
                raise ValueError("data larger than negotiated block")
            if expected is None:
                if block not in (0, 1):
                    raise ValueError("first block must be 0 or 1")
                expected = first = block
            if block == expected:
                output.write(payload)
                digest.update(payload)
                total += len(payload)
                expected = (expected + 1) & 65535
                reply = packet(4, block)
                retries = 0
                if len(payload) < blocksize:
                    output.flush()
                    os.fsync(output.fileno())
                    # Publish the complete file before acknowledging completion.
                    partial.rename(destination)
                    transfer.sendto(reply, peer)
                    break
                transfer.sendto(reply, peer)
            elif block == ((expected - 1) & 65535):
                transfer.sendto(reply, peer)
    record = {"file": name, "bytes": total, "sha256": digest.hexdigest(),
              "source": peer[0], "first_block": first, "blocksize": blocksize,
              "seconds": round(time.monotonic() - started, 3)}
    with (root / "transfers.jsonl").open("a") as records:
        records.write(json.dumps(record) + "\n")
    print(json.dumps(record), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--port", type=int, default=1069)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    args.root.mkdir(parents=True, exist_ok=True)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as listener:
        listener.bind((args.bind, args.port))
        print(f"Listening {args.bind}:{args.port}; source {args.source}; root {args.root}", flush=True)
        while True:
            request, peer = listener.recvfrom(4096)
            if peer[0] != args.source:
                continue
            if request[:2] != b"\0\2":
                listener.sendto(b"\0\5\0\2Uploads only\0", peer)
                continue
            try:
                receive(listener, request, peer, args.root, args.bind)
            except (OSError, ValueError, UnicodeError) as exc:
                print(f"FAILED {peer}: {exc}", flush=True)
                listener.sendto(b"\0\5\0\2Upload refused\0", peer)


if __name__ == "__main__":
    main()

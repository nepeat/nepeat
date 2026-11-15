#!/usr/bin/env python3
"""Read known storage over TCP; all target commands go through tmux/tio."""
import hashlib, json, pathlib, socket, subprocess, time, sys
ROOT = pathlib.Path(__file__).resolve().parent
SESSION = "serial-7760"
LOG = ROOT / "serial-20261114-020529.txt"

def send(command):
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "-l", command], check=True)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)

items = [(f"/dev/mtd{i}", f"mtd{i}.bin", size) for i, size in enumerate([131072,131072,33554432,33554432,65798144,131072,131072,786432])]
items += [("/sys/bus/i2c/devices/0-0050/eeprom", "board-eeprom.bin", 32768), ("/dev/mmcblk0", "mmcblk0.img", 7948206080)]
for index, (source, name, size) in enumerate(items):
    if len(sys.argv) > 1 and name not in sys.argv[1:]:
        continue
    dest = ROOT / "dumps" / name
    if dest.exists():
        raise SystemExit(f"Refusing to replace {dest}")
    port = 19000 + index
    send(f"nc -l -p {port} < {source}")
    time.sleep(1)
    h = hashlib.sha256()
    received = 0
    started = last = time.monotonic()
    partial = dest.with_suffix(dest.suffix + ".partial")
    with socket.create_connection(("10.60.2.253", port), timeout=30) as sock, partial.open("xb") as out:
        sock.settimeout(60)
        while received < size:
            chunk = sock.recv(min(1024*1024, size-received))
            if not chunk:
                raise RuntimeError(f"{name}: premature EOF at {received}/{size}")
            out.write(chunk)
            h.update(chunk)
            received += len(chunk)
            now = time.monotonic()
            if now-last > 30:
                print(f"{name}: {received}/{size} bytes ({received/(now-started)/1e6:.1f} MB/s)", flush=True)
                last = now
    # BusyBox nc may remain waiting for stdin after the peer closes.
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "C-c"], check=True)
    time.sleep(.5)
    offset = LOG.stat().st_size
    send(f"sha256sum {source}")
    deadline = time.monotonic()+900
    expected = None
    while time.monotonic() < deadline:
        with LOG.open("rb") as f:
            f.seek(offset)
            lines = f.read().decode(errors="replace").splitlines()
        for line in lines:
            parts = line.split()
            if len(parts)==2 and parts[1]==source and len(parts[0])==64:
                expected = parts[0]
        if expected:
            break
        time.sleep(2)
    if expected != h.hexdigest():
        raise RuntimeError(f"{name}: source hash {expected} != transfer {h.hexdigest()}")
    stored = hashlib.file_digest(partial.open("rb"), "sha256").hexdigest()
    if stored != expected:
        raise RuntimeError(f"{name}: saved file hash mismatch")
    partial.rename(dest)
    record = dict(source=source, file=name, bytes=size, sha256=stored, verified=True)
    with (ROOT / "dumps" / "manifest.jsonl").open("a") as f:
        f.write(json.dumps(record)+"\n")
    print(json.dumps(record), flush=True)

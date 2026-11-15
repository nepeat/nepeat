#!/usr/bin/env python3
"""Copy one small file (sysfs blob, DTB) off the rdinit shell, hash-checked.
  fetch_small.py /sys/firmware/fdt dumps/live.dtb [port]
"""
import hashlib, pathlib, socket, subprocess, sys, time
ROOT = pathlib.Path(__file__).resolve().parent
src, dst = sys.argv[1], pathlib.Path(sys.argv[2])
port = int(sys.argv[3]) if len(sys.argv) > 3 else 19300
def box(cmd):
    return subprocess.run([sys.executable, str(ROOT / "console.py"), "--prompt", "# ", cmd],
                          check=True, capture_output=True, text=True).stdout
if dst.exists():
    raise SystemExit(f"refusing to replace {dst}")
box(f"socat -u OPEN:{src},rdonly TCP-LISTEN:{port},reuseaddr >/dev/null 2>&1 &")
time.sleep(1)
data = b""
with socket.create_connection(("10.60.2.10", port), timeout=10) as s:
    s.settimeout(5)  # sysfs blobs are small; idle 5 s = done
    try:
        while chunk := s.recv(65536):
            data += chunk
    except socket.timeout:
        pass
box(f"kill %1 2>/dev/null; true")
want = next(w for w in box(f"sha256sum {src}").split() if len(w) == 64)
got = hashlib.sha256(data).hexdigest()
if want != got or not data:
    raise SystemExit(f"{src}: box {want} != received {got} ({len(data)} bytes)")
dst.write_bytes(data)
print(f"{dst} {len(data)} {got}")

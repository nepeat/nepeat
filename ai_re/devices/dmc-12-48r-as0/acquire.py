#!/usr/bin/env python3
"""Image DMC-12 storage read-only over TCP, then zstd-compress verified images.

Precondition: the box is in the factory initramfs shell with nothing mounted
from disk (U-Boot: `setenv bootargsextra rdinit=/bin/sh; run dmc12_bootdisk`,
not saved), /proc /sys /dev mounted, eth0 up. See PROGRESS.md.

Per item: box streams it with socat; host hashes while receiving into
dumps/<unit>/<name>.partial; box sha256 == transfer sha256 == reread sha256, or it
stays .partial and fails. Then: zstd, `zstd -t`, decompress-and-hash must equal
the raw hash, and only then is the raw file removed. Results append to
dumps/<unit>/manifest.jsonl and SHA256SUMS. Add the unit's row to units.md.
  acquire.py --unit u02 --host 10.60.2.x              # everything not yet captured
  acquire.py --unit u02 --host 10.60.2.x mtd0.bin     # one item
  acquire.py --unit u01 --compress sda.img <sha256>   # resume: raw verified, zstd not done
"""
import hashlib, json, pathlib, socket, subprocess, sys, time

ROOT = pathlib.Path(__file__).resolve().parent

def take(flag):
    if flag not in sys.argv:
        raise SystemExit(f"{flag} is required (see --help in the docstring)")
    i = sys.argv.index(flag)
    value = sys.argv[i + 1]
    del sys.argv[i:i + 2]
    return value


UNIT = take("--unit")
if not (len(UNIT) == 3 and UNIT[0] == "u" and UNIT[1:].isdigit()):
    raise SystemExit("--unit must look like u01")
DUMPS = ROOT / "dumps" / UNIT
HOST = take("--host") if "--compress" not in sys.argv else None  # rdinit-shell DHCP lease
SECTOR = 512
ITEMS = [  # (source, file, size in bytes, as reported by the box; None = whatever it reports)
    ("/dev/mtd0", "mtd0.bin", 16777216),
    ("/dev/mtd1", "mtd1.bin", 16777216),
    ("/dev/sda", "sda.img", None),  # SSDs differ per unit (u01: 62533296 sectors, 32 GB)
]
# Bounded threads: -T0 (32 threads, ~1.7 GB RSS on a 32 GB image) plus a big
# nix build ran the host out of memory on 2026-11-15 and took the user session
# down with it.
ZSTD = ["zstd", "-q", "-T6", "-12", "--long=27"]


def box(command, timeout=60):
    out = subprocess.run([sys.executable, str(ROOT / "console.py"), "--prompt", "# ",
                          "--timeout", str(timeout), command],
                         check=True, capture_output=True, text=True).stdout
    return out.replace("\r", "")


def sha256_file(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def sha256_stream(cmd):
    h = hashlib.sha256()
    with subprocess.Popen(cmd, stdout=subprocess.PIPE) as p:
        for chunk in iter(lambda: p.stdout.read(1 << 20), b""):
            h.update(chunk)
    if p.returncode:
        raise RuntimeError(f"{cmd[0]} exited {p.returncode}")
    return h.hexdigest()


def record(entry):
    with (DUMPS / "manifest.jsonl").open("a") as f:
        f.write(json.dumps(entry) + "\n")
    with (DUMPS / "SHA256SUMS").open("a") as f:
        f.write(f"{entry['sha256']}  {entry['file']}\n")
        f.write(f"{entry['zst_sha256']}  {entry['file']}.zst\n")


def compress(name, source, size, digest, method):
    dest, zst = DUMPS / name, DUMPS / (name + ".zst")
    tmp = DUMPS / (name + ".zst.partial")
    tmp.unlink(missing_ok=True)
    subprocess.run(["nice", "-n", "10"] + ZSTD + [str(dest), "-o", str(tmp)], check=True)
    subprocess.run(["zstd", "-q", "-t", "--long=27", str(tmp)], check=True)
    if sha256_stream(["zstd", "-q", "-dc", "--long=27", str(tmp)]) != digest:
        raise SystemExit(f"{name}: decompressed hash differs; kept raw {dest.name}")
    tmp.rename(zst)
    record({"file": name, "source": source, "bytes": size, "sha256": digest,
            "zst_bytes": zst.stat().st_size, "zst_sha256": sha256_file(zst),
            "zstd": " ".join(ZSTD[1:]), "method": method,
            "date": time.strftime("%Y-%m-%dT%H:%M:%S%z")})
    dest.unlink()
    print(f"{name}: compressed to {zst.stat().st_size} bytes, raw removed", flush=True)


DUMPS.mkdir(mode=0o700, exist_ok=True)
if sys.argv[1:2] == ["--compress"]:
    name, digest = sys.argv[2], sys.argv[3]
    index, (source, _, size) = next((i, it) for i, it in enumerate(ITEMS) if it[1] == name)
    dest = DUMPS / name
    size = size or dest.stat().st_size
    if dest.stat().st_size != size or sha256_file(dest) != digest:
        raise SystemExit(f"{name}: raw file no longer matches {digest}")
    compress(name, source, size, digest, f"socat tcp :{19100 + index} from rdinit shell")
    sys.exit()
for index, (source, name, size) in enumerate(ITEMS):
    if len(sys.argv) > 1 and name not in sys.argv[1:]:
        continue
    dest, zst = DUMPS / name, DUMPS / (name + ".zst")
    if zst.exists() or dest.exists():
        print(f"{name}: already captured, skipping")
        continue
    reported = box(f"cat /sys/class/{'mtd/' + source[5:] if 'mtd' in source else 'block/sda'}/size").split()
    reported = int(reported[-1]) * (1 if "mtd" in source else SECTOR)
    if size is None:
        if reported <= 0:
            raise SystemExit(f"{name}: box reports {reported} bytes")
        size = reported
        print(f"{name}: box reports {size} bytes ({size // SECTOR} sectors)", flush=True)
    if reported != size:
        raise SystemExit(f"{name}: box reports {reported} bytes, expected {size}")

    port = 19100 + index
    # Background listener on the box; the shell prompt returns at once.
    box(f"socat -u OPEN:{source},rdonly TCP-LISTEN:{port},reuseaddr >/tmp/socat-{port}.err 2>&1 &")
    time.sleep(1)
    partial = DUMPS / (name + ".partial")
    h, received = hashlib.sha256(), 0
    started = last = time.monotonic()
    with socket.create_connection((HOST, port), timeout=30) as sock, partial.open("xb") as out:
        sock.settimeout(120)
        while received < size:
            chunk = sock.recv(min(4 << 20, size - received))
            if not chunk:
                raise SystemExit(f"{name}: premature EOF at {received}/{size}; left {partial.name}")
            out.write(chunk)
            h.update(chunk)
            received += len(chunk)
            now = time.monotonic()
            if now - last > 30:
                print(f"{name}: {received}/{size} ({received / (now - started) / 1e6:.1f} MB/s)", flush=True)
                last = now
    elapsed = time.monotonic() - started
    transfer = h.hexdigest()
    # Hash on the box after the transfer (large disks: minutes).
    words = box(f"sha256sum {source}", timeout=3600).split()
    source_hash = next(w for w in words if len(w) == 64)
    reread = sha256_file(partial)
    if not (source_hash == transfer == reread):
        raise SystemExit(f"{name}: hash mismatch source={source_hash} transfer={transfer} "
                         f"reread={reread}; left {partial.name}")
    partial.rename(dest)
    print(f"{name}: {size} bytes verified in {elapsed:.0f}s, sha256 {transfer}", flush=True)

    compress(name, source, size, transfer, f"socat tcp :{port} from rdinit shell")

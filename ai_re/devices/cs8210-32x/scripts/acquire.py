"""Read the confirmed SSD once, hash its entire byte stream, and compress it.

Run inside ai_re's nix devshell. Never opens the source for writing.
Failure leaves a .partial archive and does not record a successful acquisition.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

base = Path(__file__).resolve().parents[1]
os.umask(0o077)
source = '/dev/sdb'
expected_size = 120034123776
identity = subprocess.check_output(['lsblk', '-dn', '-o', 'MODEL,SERIAL', source], text=True).split()
assert identity == ['HBS3A1912A4M4B1', 'A069BCC5'], identity
assert int(subprocess.check_output(['sudo', '-n', 'blockdev', '--getsize64', source])) == expected_size
devices = json.loads(subprocess.check_output(['lsblk', '-J', '-o', 'PATH,MOUNTPOINTS', source]))
def unmounted(device):
    assert not any(device.get('mountpoints') or []), device
    for child in device.get('children', []):
        unmounted(child)
for device in devices['blockdevices']:
    unmounted(device)
# Kernel-side write protection; no writes to SSD media.
subprocess.run(['sudo', '-n', 'blockdev', '--setro', source], check=True)
assert subprocess.check_output(['sudo', '-n', 'blockdev', '--getro', source]).strip() == b'1'
archive = base / 'images/ssd-2026-10-17.img.zst'
partial = archive.with_suffix(archive.suffix + '.partial')
assert not archive.exists() and not partial.exists()
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
hasher = hashlib.sha256()
count = 0
start = last = time.monotonic()
with partial.open('xb') as output, (base / 'evidence/read-errors.txt').open('x') as errors:
    reader = subprocess.Popen(['sudo', '-n', 'dd', 'if=' + source, 'bs=4M', 'iflag=fullblock', 'status=none'], stdout=subprocess.PIPE, stderr=errors)
    compressor = subprocess.Popen(['zstd', '-T4', '-6', '--check', '--stream-size=' + str(expected_size), '-c'], stdin=subprocess.PIPE, stdout=output)
    try:
        while chunk := reader.stdout.read(4 * 1024 * 1024):
            hasher.update(chunk)
            compressor.stdin.write(chunk)
            count += len(chunk)
            now = time.monotonic()
            if now - last >= 15:
                print(f'{count}/{expected_size} bytes ({count/expected_size:.1%}); {count/(now-start)/1e6:.1f} MB/s', flush=True)
                last = now
        compressor.stdin.close()
        assert reader.wait() == 0, 'source read failed; see read-errors.txt'
        assert compressor.wait() == 0, 'compression failed'
        assert count == expected_size, (count, expected_size)
        output.flush()
        os.fsync(output.fileno())
    finally:
        for process in (reader, compressor):
            if process.poll() is None:
                process.terminate()
                process.wait()
partial.rename(archive)
record = dict(source=source, model=identity[0], serial=identity[1], bytes=count,
              raw_sha256=hasher.hexdigest(), started_utc=started,
              completed_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
              compressed_bytes=archive.stat().st_size, archive=str(archive.relative_to(base)),
              compression='zstd -T4 -6 --check', source_kernel_read_only=True,
              read_errors=False)
(base / 'evidence/acquisition.json').write_text(json.dumps(record, indent=2) + '\n')
print(json.dumps(record, indent=2), flush=True)

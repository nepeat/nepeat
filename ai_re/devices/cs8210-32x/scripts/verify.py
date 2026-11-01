"""Verify the compressed acquisition and make a disposable sparse analysis copy."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

base = Path(__file__).resolve().parents[1]
os.umask(0o077)
record = json.loads((base / 'evidence/acquisition.json').read_text())
archive = base / record['archive']
raw = base / 'analysis/ssd.img'
partial = raw.with_suffix('.img.partial')
assert not raw.exists() and not partial.exists()
compressed_hash = hashlib.file_digest(archive.open('rb'), 'sha256').hexdigest()
hasher = hashlib.sha256()
count = 0
last = time.monotonic()
decoder = subprocess.Popen(['zstd', '-dc', str(archive)], stdout=subprocess.PIPE)
try:
    with partial.open('xb') as output:
        while chunk := decoder.stdout.read(4 * 1024 * 1024):
            hasher.update(chunk)
            if chunk.count(0) == len(chunk):
                output.seek(len(chunk), 1)
            else:
                output.write(chunk)
            count += len(chunk)
            if time.monotonic() - last >= 15:
                print(f'verified {count}/{record["bytes"]} bytes ({count/record["bytes"]:.1%})', flush=True)
                last = time.monotonic()
        output.truncate(count)
        output.flush()
        os.fsync(output.fileno())
    assert decoder.wait() == 0, 'zstd frame/checksum verification failed'
    assert count == record['bytes'], 'wrong decompressed size'
    assert hasher.hexdigest() == record['raw_sha256'], 'raw SHA-256 mismatch'
finally:
    if decoder.poll() is None:
        decoder.terminate()
        decoder.wait()
partial.rename(raw)
raw.chmod(0o400)
result = dict(verified_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
              decompressed_bytes=count, raw_sha256=hasher.hexdigest(),
              compressed_sha256=compressed_hash, zstd_decode_passed=True,
              matches_acquisition=True)
(base / 'evidence/verification.json').write_text(json.dumps(result, indent=2) + '\n')
(base / 'images/SHA256SUMS').write_text(compressed_hash + '  ' + archive.name + '\n')
(base / 'images/RAW-SHA256SUMS').write_text(hasher.hexdigest() + '  ssd-2026-10-17.img\n')
print(json.dumps(result, indent=2), flush=True)

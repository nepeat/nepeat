"""One-shot SmartNIC/QCT promotion after independent candidate acceptance.

Historical once run; never rerun. Prior live bytes are retained as recovery files.
Run from the project root with PYTHONPATH=.
"""
import hashlib
import json
import os
import shutil
from pathlib import Path

ROOT = Path.cwd()
O = ROOT / 'research/batches/smartnic-qct-2026-10-04/integration'
CANDIDATE = ROOT / 'data/candidate-with-smartnic-qct-2026-10-05.sqlite'
STAGED = ROOT / 'data/import-manifest-smartnic-qct-2026-10-05.json'
REBUILT = ROOT / 'data/rebuilt-with-smartnic-qct-2026-10-05.sqlite'
LIVE = ROOT / 'data/devices.sqlite'
MANIFEST = ROOT / 'data/import-manifest.json'
BACKUP = ROOT / 'data/devices-before-smartnic-qct-2026-10-05.sqlite'
BACKUP_MANIFEST = ROOT / 'data/import-manifest-before-smartnic-qct-2026-10-05.json'


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


proof = json.loads((O / 'candidate-verification.json').read_text())
stage = json.loads((O / 'stage-result.json').read_text())
verdict = (O / 'candidate-review.md').read_text().split('\n', 1)[0]
# The independent verdict must lead the report: ACCEPT or ACCEPT with notes, never BLOCK.
assert verdict.split(':', 1)[0] in ('ACCEPT', 'ACCEPT with notes'), verdict
assert not BACKUP.exists() and not BACKUP_MANIFEST.exists()
assert sha(CANDIDATE) == proof['candidate_sha256'] == stage['candidate_sha256']
assert sha(STAGED) == proof['staged_manifest_sha256'] == stage['staged_manifest_sha256']
assert sha(REBUILT) == proof['rebuild_sha256']
assert sha(LIVE) == proof['live_sha256'] == stage['live_sha256']
assert sha(MANIFEST) == proof['live_manifest_sha256']
intent = dict(before_database_sha256=proof['live_sha256'], before_manifest_sha256=proof['live_manifest_sha256'],
              after_database_sha256=proof['candidate_sha256'], after_manifest_sha256=proof['staged_manifest_sha256'],
              recovery_database=str(BACKUP.relative_to(ROOT)), recovery_manifest=str(BACKUP_MANIFEST.relative_to(ROOT)))
with (O / 'promotion-intent.json').open('x') as stream:
    json.dump(intent, stream, indent=2)
    stream.flush()
    os.fsync(stream.fileno())
for source, destination in ((LIVE, BACKUP), (MANIFEST, BACKUP_MANIFEST)):
    with source.open('rb') as src, destination.open('xb') as dst:
        shutil.copyfileobj(src, dst, 1 << 20)
        dst.flush()
        os.fsync(dst.fileno())
    assert sha(source) == sha(destination)
os.replace(CANDIDATE, LIVE)
os.replace(STAGED, MANIFEST)
fd = os.open(ROOT / 'data', os.O_RDONLY)
try:
    os.fsync(fd)
finally:
    os.close(fd)
assert sha(LIVE) == intent['after_database_sha256'] and sha(MANIFEST) == intent['after_manifest_sha256']
with (O / 'promotion-result.json').open('x') as stream:
    json.dump(dict(intent, promoted=True, candidate_moved_to_live=True, staged_manifest_moved_to_live=True),
              stream, indent=2)
print(json.dumps(intent, indent=2))

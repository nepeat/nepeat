"""One-shot coordinator promotion after independent acceptance; prior bytes retained."""
import json
import os
import shutil
from pathlib import Path
from devicehunt.lineage import checksum

root = Path.cwd()
batch = root / 'research/batches/archive-implementation-2026-10-04'
freeze = json.loads((batch / 'acceptance-freeze-final.json').read_text())
candidate = root / 'data/catalog-archive-candidate-2026-10-04.sqlite'
staged_manifest = root / 'data/import-manifest-archive-2026-10-04.json'
live = root / 'data/devices.sqlite'
manifest = root / 'data/import-manifest.json'
backup = root / 'data/devices-before-archive-2026-10-04.sqlite'
backup_manifest = root / 'data/import-manifest-before-archive-2026-10-04.json'
assert not backup.exists() and not backup_manifest.exists()
assert (batch / 'promotion-review.md').exists()
assert (batch / 'storage-consolidation-result.json').exists()
assert checksum(candidate) == freeze['candidate_sha256']
assert checksum(staged_manifest) == freeze['manifest_sha256']
assert checksum(root / 'data/catalog-archive-rebuilt-2026-10-04.sqlite') == freeze['rebuild_sha256']
assert checksum(live) == 'a098c390431efd77d25f162acb039d2ddd938ff5effeffd7e93fa9b0bc34ff16'
assert all(checksum(root / name) == sha for name, sha in freeze['application_test_file_hashes'].items())
intent = dict(before_database_sha256=checksum(live), before_manifest_sha256=checksum(manifest),
              after_database_sha256=freeze['candidate_sha256'], after_manifest_sha256=freeze['manifest_sha256'],
              recovery_database=str(backup.relative_to(root)), recovery_manifest=str(backup_manifest.relative_to(root)))
with (batch / 'promotion-intent.json').open('x') as stream:
    json.dump(intent, stream, indent=2)
    stream.flush()
    os.fsync(stream.fileno())
for source, destination in ((live, backup), (manifest, backup_manifest)):
    with source.open('rb') as src, destination.open('xb') as dst:
        shutil.copyfileobj(src, dst, 1024 * 1024)
        dst.flush()
        os.fsync(dst.fileno())
    assert checksum(source) == checksum(destination)
os.replace(candidate, live)
os.replace(staged_manifest, manifest)
fd = os.open(root / 'data', os.O_RDONLY)
try:
    os.fsync(fd)
finally:
    os.close(fd)
assert checksum(live) == intent['after_database_sha256']
assert checksum(manifest) == intent['after_manifest_sha256']
with (batch / 'promotion-result.json').open('x') as stream:
    json.dump(dict(intent, promoted=True, candidate_moved_to_live=True,
                   staged_manifest_moved_to_live=True), stream, indent=2)
print(json.dumps(intent, indent=2))

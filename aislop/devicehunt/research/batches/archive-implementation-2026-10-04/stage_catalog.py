"""One-shot candidate staging. Never writes live database/manifest/exports."""
import json
import shutil
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt import lineage

root = Path.cwd()
batch = root / 'research/batches/archive-implementation-2026-10-04'
candidate = root / 'data/catalog-archive-candidate-2026-10-04.sqlite'
manifest = root / 'data/import-manifest-archive-2026-10-04.json'
release_path = root / 'data/archive-release-2026-10-04.json'
assert not candidate.exists() and not manifest.exists()
baseline = lineage.checksum(root / 'data/devices.sqlite')
shutil.copyfile(root / 'data/devices.sqlite', candidate)
c = Catalog(candidate)
old_root = Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans')
relocated = 0
for sid, cached, expected in c.db.execute('SELECT id,cache_path,sha256 FROM sources').fetchall():
    if not cached:
        continue
    p = Path(cached)
    if p.is_absolute():
        relative = p.relative_to(old_root) if p.is_relative_to(old_root) else p.relative_to(root)
    else:
        relative = p
    target = lineage.local(root, str(relative))
    assert lineage.checksum(target) == expected, sid
    c.db.execute('UPDATE sources SET cache_path=? WHERE id=?', (str(target), sid))
    relocated += 1
result = lineage.import_bundle(c, release_path)
c.db.commit()
assert c.db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
assert not c.db.execute('PRAGMA foreign_key_check').fetchall()
m = json.loads((root / 'data/import-manifest.json').read_bytes())
m['description'] += '; frozen accepted archive lineage release 2026-10-04'
m['imports'].append({'source': 'archive-release', 'path': release_path.name})
m['files'][release_path.name] = lineage.checksum(release_path)
release = lineage.prepare(release_path)
for artifact in release['artifacts']:
    for name in artifact['paths']:
        if name in m['files']:
            assert m['files'][name] == artifact['sha256']
        m['files'][name] = artifact['sha256']
m['expected'].update({table: c.db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                      for table in lineage.TABLES})
with manifest.open('x') as stream:
    json.dump(m, stream, indent=2)
    stream.write('\n')
c.close() if hasattr(c, 'close') else c.db.close()
assert lineage.checksum(root / 'data/devices.sqlite') == baseline
proof = dict(candidate=str(candidate.relative_to(root)), candidate_sha256=lineage.checksum(candidate),
             manifest=str(manifest.relative_to(root)), manifest_sha256=lineage.checksum(manifest),
             baseline_sha256=baseline, lineage=result, source_paths_relocated=relocated,
             schema=29, live_unchanged=True)
with (batch / 'candidate-proof.json').open('x') as stream:
    json.dump(proof, stream, indent=2)
print(json.dumps(proof, indent=2))

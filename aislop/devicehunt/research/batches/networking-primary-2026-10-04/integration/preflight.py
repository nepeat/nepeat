"""Read-only batch preflight. Does not stage inputs, import, or change manifests."""
import hashlib
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent
BATCH = HERE.parent

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

before = json.loads((BATCH / 'before.json').read_text())
live = ROOT / 'data/devices.sqlite'
manifest_path = ROOT / 'data/import-manifest.json'
assert sha(live) == before['database_sha256']
assert sha(manifest_path) == before['manifest_sha256']
manifest = json.loads(manifest_path.read_text())
report = {'database_sha256': sha(live), 'manifest_sha256': sha(manifest_path),
          'jobs_before': len(manifest['imports']), 'inputs_before': len(manifest['files']),
          'inputs': [], 'mode': 'read-only prep; pending independent acceptance'}
for name, count, brand in [('owned/artifact-candidate.json', 10, ''),
                           ('oracle/candidate-curated.json', 25, 'Oracle')]:
    path = BATCH / name
    payload = json.loads(path.read_text())
    assert len(payload['sources']) == len(payload['devices']) == 1
    source, device = payload['sources'][0], payload['devices'][0]
    assert device['source'] == source['id'] and device['brand'] == brand
    assert len(device['facts']) == count
    if not brand:
        assert device['affiliations'] == [] and device['model'] == 'AS7772-32X'
        snapshot = ROOT / 'data' / source['snapshot']
    else:
        snapshot = (path.parent / source['snapshot']).resolve()
        assert device['model'] == 'Oracle Server X7-2c'
    assert snapshot.is_file()
    expected = dict(device['facts'])['source_sha256']
    assert sha(snapshot) == expected
    report['inputs'].append({'path': str(path.relative_to(ROOT)), 'sha256': sha(path),
                            'snapshot': str(snapshot.relative_to(ROOT)),
                            'snapshot_sha256': expected, 'facts': count,
                            'source_id': source['id']})
with sqlite3.connect(live.as_uri() + '?mode=ro', uri=True) as db:
    assert not any(db.execute('SELECT 1 FROM sources WHERE id=?', (x['source_id'],)).fetchone()
                   for x in report['inputs'])
    report['counts_before'] = {t: db.execute('SELECT count(*) FROM ' + t).fetchone()[0]
                              for t in ['devices', 'records', 'sources', 'facts',
                                        'documents', 'document_pages', 'hyperscaler_evidence']}
assert sha(live) == before['database_sha256']
assert sha(manifest_path) == before['manifest_sha256']
print(json.dumps(report, indent=2))

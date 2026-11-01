"""Authorized one-shot staging only; never promotes or refreshes normal exports."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import sys
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import read_manifest, rebuild

ROOT = Path.cwd()
HERE = Path(__file__).resolve().parent
DATA = ROOT / 'data'
LIVE = DATA / 'devices.sqlite'
MANIFEST = DATA / 'import-manifest.json'
INPUT = DATA / 'networking-primary-facts-2026-10-04.json'
CANDIDATE = DATA / 'networking-primary-candidate-2026-10-04.sqlite'
REBUILT = DATA / 'rebuilt-with-networking-primary-2026-10-04.sqlite'
acceptance = json.loads((HERE.parent / 'acceptance.json').read_text())
before = json.loads((HERE.parent / 'before.json').read_text())

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def freeze():
    for group in ('files', 'code_sha256'):
        for path, expected in acceptance[group].items():
            assert sha(ROOT / path) == expected, path
    assert sha(LIVE) == before['database_sha256']

def save_new(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2, ensure_ascii=False)
        stream.write('\n')

def copy_new(source, destination):
    assert not destination.exists(), destination
    shutil.copy2(source, destination)
    assert sha(source) == sha(destination)

freeze()
assert acceptance['phase'] == 'stage_only'
assert sha(MANIFEST) == before['manifest_sha256']
_, old_manifest = read_manifest(MANIFEST)
owned = json.loads((HERE.parent / 'owned/artifact-candidate.json').read_text())
oracle = json.loads((HERE.parent / 'oracle/candidate-curated.json').read_text())
merged = copy.deepcopy(owned)
oracle_rebased = copy.deepcopy(oracle)
oracle_rebased['sources'][0]['snapshot'] = 'raw/networking-primary-oracle-2026-10-04/oracle-x7-2c-vpat-t2-6972.html'
merged['sources'].extend(oracle_rebased['sources'])
merged['devices'].extend(oracle_rebased['devices'])
assert merged['devices'] == owned['devices'] + oracle['devices']
assert sum(len(d['facts']) for d in merged['devices']) == 35
for source in merged['sources']:
    snapshot = DATA / source['snapshot']
    rows = [d for d in merged['devices'] if d['source'] == source['id']]
    assert len(rows) == 1 and snapshot.is_file()
    assert sha(snapshot) == dict(rows[0]['facts'])['source_sha256']
assert merged['devices'][0]['brand'] == '' and merged['devices'][0]['affiliations'] == []
for path in [INPUT, CANDIDATE, REBUILT,
             DATA / 'devices-before-networking-primary-2026-10-04.sqlite',
             DATA / 'import-manifest-before-networking-primary-2026-10-04.json']:
    assert not path.exists(), path
copy_new(LIVE, DATA / 'devices-before-networking-primary-2026-10-04.sqlite')
copy_new(MANIFEST, DATA / 'import-manifest-before-networking-primary-2026-10-04.json')
save_new(INPUT, merged)
copy_new(LIVE, CANDIDATE)
c = Catalog(CANDIDATE)
try:
    assert not any(c.db.execute('SELECT 1 FROM sources WHERE id=?', (s['id'],)).fetchone() for s in merged['sources'])
    assert import_source(c, 'curated', INPUT) == 2
    counts = {t: c.db.execute('SELECT count(*) FROM ' + t).fetchone()[0]
              for t in ['devices', 'records', 'sources', 'facts', 'documents', 'document_pages', 'hyperscaler_evidence']}
    assert counts == dict(devices=4084, records=5302, sources=283, facts=44763,
                         documents=56, document_pages=3651, hyperscaler_evidence=22), counts
finally:
    c.db.close()
manifest = copy.deepcopy(old_manifest)
manifest['imports'].append({'source': 'curated', 'path': INPUT.name})
for path in [INPUT, *[DATA / s['snapshot'] for s in merged['sources']]]:
    name = str(path.relative_to(DATA))
    assert name not in manifest['files']
    manifest['files'][name] = sha(path)
manifest['expected']['devices'] += 1
manifest['expected']['records'] += 2
for source in merged['sources']:
    assert source['id'] not in manifest['expected']['sources']
    manifest['expected']['sources'][source['id']] = 1
assert len(manifest['imports']) == 118 and len(manifest['files']) == 2181
assert manifest['imports'][:-1] == old_manifest['imports']
assert all(manifest['files'][k] == v for k,v in old_manifest['files'].items())
# Manifest is the only authorized existing shared file changed at this phase.
MANIFEST.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
read_manifest(MANIFEST)
freeze()
with (HERE / 'rebuild.log').open('x') as log:
    result = rebuild(MANIFEST, REBUILT, lambda row: print(json.dumps(row), file=log, flush=True))
freeze()
save_new(HERE / 'stage-verification.json', {'phase': 'stage_only', 'counts': counts,
         'live_sha256_unchanged': sha(LIVE), 'candidate_sha256': sha(CANDIDATE),
         'rebuilt_sha256': sha(REBUILT), 'merged_input_sha256': sha(INPUT),
         'manifest_sha256': sha(MANIFEST), 'jobs': 118, 'inputs': 2181,
         'accepted_full_suite': acceptance['files']['research/batches/networking-primary-2026-10-04/engineering/full-tests.log'],
         'frozen_files': len(acceptance['files']), 'frozen_code_files': len(acceptance['code_sha256']),
         'source_snapshot_only_normalization': True})
print('staged candidate and clean rebuild; live unchanged')

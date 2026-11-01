"""One-shot SmartNIC/QCT staging, rebased onto the current schema-29 catalog.

Integrator: /root-claude (2026-10-05). Reads accepted inputs, writes only NEW
files: merged curated input, byte-identical sibling PDF/text copies, the complete
50-entry reference bundle, a candidate catalog, a staged manifest and this
batch's integration proofs. The live catalog and live manifest stay unchanged.
Run from the project root: PYTHONPATH=. python3 research/.../integration/stage.py
"""
import hashlib
import json
import os
import sqlite3
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.reference_documents import prepare

R = Path.cwd()
B = R / 'research/batches/smartnic-qct-2026-10-04'
O = B / 'integration'
LIVE_SHA = '62ab16eb8ab562ebbd6eaf6951987800fa8f4482879007d1713d0b8529ce0e4a'
MERGED = R / 'data/smartnic-qct-facts-2026-10-04.json'
BUNDLE = R / 'data/raw/reference-documents-with-smartnic-qct-2026-10-05.json'
CANDIDATE = R / 'data/candidate-with-smartnic-qct-2026-10-05.sqlite'
STAGED = R / 'data/import-manifest-smartnic-qct-2026-10-05.json'


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def dump(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def fresh(path, data):
    with open(path, 'xb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


acceptance = json.loads((B / 'acceptance.json').read_text())
for name, digest_ in acceptance['files'].items():
    assert sha(R / name) == digest_, name
assert sha(R / 'data/devices.sqlite') == LIVE_SHA
manifest = json.loads((R / 'data/import-manifest.json').read_text())
assert len(manifest['imports']) == 121 and len(manifest['files']) == 2255
for name, digest_ in manifest['files'].items():
    assert sha(R / 'data' / name) == digest_, name
old = json.loads((R / acceptance['complete_old_bundle']).read_text())
assert len(old['documents']) == 47
O.mkdir(exist_ok=True)
dump(O / 'preflight.json', dict(
    integrator='/root-claude', rebased_from='acceptance.json (pre-archive 120-job/2,205-input base)',
    live_sha256=LIVE_SHA, live_manifest_sha256=sha(R / 'data/import-manifest.json'),
    manifest_jobs=121, manifest_hashed_inputs=2255, accepted_input_files=len(acceptance['files']),
    standard_exports={str(p.relative_to(R)): sha(p) for p in sorted((R / 'data/exports').rglob('*')) if p.is_file()}))

# Merge curated inputs; snapshot paths rebase to the same immutable files.
merged = {'sources': [], 'devices': []}
for name in acceptance['curated_inputs']:
    path = R / name
    data = json.loads(path.read_text())
    for source in data['sources']:
        original = (path.parent / source['snapshot']).resolve()
        source['snapshot'] = os.path.relpath(original, MERGED.parent)
        assert (MERGED.parent / source['snapshot']).resolve() == original and original.is_file()
    merged['sources'] += data['sources']
    merged['devices'] += data['devices']
fresh(MERGED, (json.dumps(merged, indent=2, ensure_ascii=False) + '\n').encode())

# Complete bundle: all 47 old entries unchanged, plus three byte-identical siblings.
entries, copied = [], []
for name in acceptance['pdf_proposals']:
    proposal = R / name
    entry = json.loads(proposal.read_text())['documents']
    assert len(entry) == 1
    entry = entry[0]
    for key in ('pdf', 'text'):
        src = proposal.parent / entry[key]['path']
        dst = R / 'data/raw' / entry[key]['path']
        assert Path(entry[key]['path']).name == entry[key]['path'] and sha(src) == entry[key]['sha256']
        if dst.exists():
            assert dst.read_bytes() == src.read_bytes()
        else:
            fresh(dst, src.read_bytes())
            copied.append(str(dst.relative_to(R)))
    entries.append(entry)
bundle = dict(old)
bundle['documents'] = old['documents'] + entries
assert bundle['documents'][:47] == old['documents'] and len({e['id'] for e in bundle['documents']}) == 50
fresh(BUNDLE, (json.dumps(bundle, indent=2, ensure_ascii=False) + '\n').encode())
prepare(BUNDLE)

# Candidate from the live bytes; exclusive replacement of the two touched sources.
with open(R / 'data/devices.sqlite', 'rb') as src, open(CANDIDATE, 'xb') as dst:
    for block in iter(lambda: src.read(1 << 20), b''):
        dst.write(block)
assert sha(CANDIDATE) == LIVE_SHA
catalog = Catalog(CANDIDATE)
imports = {'reference': import_source(catalog, 'reference-documents', BUNDLE),
           'curated': import_source(catalog, 'curated', MERGED)}
catalog.db.close()
con = sqlite3.connect(CANDIDATE.as_uri() + '?mode=ro', uri=True)
counts = {t: con.execute('SELECT count(*) FROM ' + t).fetchone()[0] for t in acceptance['expected_counts']}
assert counts == acceptance['expected_counts'], counts

# Staged manifest: same 121 jobs (reference path updated) plus one curated job.
staged = json.loads(json.dumps(manifest))
jobs = [j for j in staged['imports'] if j['source'] == 'reference-documents']
assert len(jobs) == 1 and jobs[0]['path'] == str(Path(acceptance['complete_old_bundle']).relative_to('data'))
jobs[0]['path'] = str(BUNDLE.relative_to(R / 'data'))
staged['imports'].append({'source': 'curated', 'path': MERGED.name})
register = [MERGED, BUNDLE] + [R / 'data/raw' / e[k]['path'] for e in entries for k in ('pdf', 'text')]
register += [R / n for n in acceptance['files'] if n.startswith('data/')]
for path in register:
    staged['files'][str(path.relative_to(R / 'data'))] = sha(path)
for key in ('devices', 'records', 'documents', 'document_pages'):
    staged['expected'][key] = counts[key]
staged['expected']['sources'] = dict(con.execute('SELECT id,records_seen FROM sources'))
assert all(staged['files'][k] == v for k, v in manifest['files'].items())
assert len(staged['imports']) == 122
con.close()
fresh(STAGED, (json.dumps(staged, indent=2, ensure_ascii=False) + '\n').encode())
dump(O / 'old-complete-bundle.json', old)
dump(O / 'stage-result.json', dict(
    counts=counts, jobs=122, hashed_inputs=len(staged['files']), imports=imports,
    old_hashed_inputs_preserved=2255, old_bundle_entries_preserved=47, copied_sibling_files=copied,
    candidate_sha256=sha(CANDIDATE), staged_manifest_sha256=sha(STAGED), live_sha256=sha(R / 'data/devices.sqlite'),
    live_manifest_unchanged=sha(R / 'data/import-manifest.json') == json.loads((O / 'preflight.json').read_text())['live_manifest_sha256']))
print(json.dumps(counts))

"""One-shot accepted data-only staging. Never promotes or refreshes normal exports."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from audit import sha, connect, audit
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import read_manifest
from devicehunt.reference_documents import prepare

ROOT = Path.cwd()
HERE = Path(__file__).resolve().parent
DATA = ROOT / 'data'
TAG = 'vendor-discovery-2026-10-04'
LIVE = DATA / 'devices.sqlite'
CAND = DATA / ('vendor-discovery-candidate-2026-10-04.sqlite')
REBUILT = DATA / 'rebuilt-with-vendor-discovery-2026-10-04.sqlite'
MANIFEST = DATA / 'import-manifest.json'
OLD_BUNDLE = DATA / 'raw/reference-documents-with-nics-wiwynn-hyve-2026-10-04.json'
BUNDLE = DATA / 'raw/reference-documents-with-vendor-discovery-2026-10-04.json'
INPUT = DATA / 'vendor-discovery-facts-2026-10-04.json'
acceptance = json.loads((HERE.parent / 'acceptance.json').read_text())
before = json.loads((HERE.parent / 'before.json').read_text())


def freeze():
    for group in ('files', 'code_sha256'):
        for name, expected in acceptance[group].items():
            assert sha(ROOT / name) == expected, name
    assert sha(LIVE) == before['database_sha256']


def save(path, value):
    assert not path.exists(), path
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def copy_new(source, target):
    assert not target.exists(), target
    shutil.copy2(source, target)
    assert sha(source) == sha(target)


def run(args, log):
    with log.open('x') as stream:
        subprocess.run([sys.executable, *args], cwd=ROOT, stdout=stream,
                       stderr=subprocess.STDOUT, check=True)


freeze()
assert sha(MANIFEST) == before['manifest_sha256']
_, manifest = read_manifest(MANIFEST)
old_manifest = json.loads(MANIFEST.read_text())
proposal = HERE.parent / 'qct/proposed-qct-facts.json'
curated = json.loads(proposal.read_text())
assert set(s['id'] for s in curated['sources']) == set(acceptance['source_ids'])
assert len(curated['sources']) == len(curated['devices']) == 2
assert sum(len(d['facts']) for d in curated['devices']) == 38
old = connect(LIVE)
try:
    for s in curated['sources']:
        assert not old.execute('SELECT 1 FROM sources WHERE id=?', (s['id'],)).fetchone()
        for field in ('id', 'url', 'tier', 'license', 'snapshot', 'coverage'):
            assert isinstance(s[field], str) and s[field]
        assert s['tier'] == 'vendor' and s['url'].startswith('https://')
        snapshot = (DATA / s['snapshot']).resolve()
        assert DATA in snapshot.parents and snapshot.is_file()
        assert str(snapshot.relative_to(ROOT)) in acceptance['files']
    for d in curated['devices']:
        for field in ('source', 'key', 'brand', 'model', 'title', 'kind'):
            assert isinstance(d[field], str) and d[field]
        assert d['source'] in acceptance['source_ids'] and d['brand'] == 'QCT'
        assert d['affiliations'] == [['QCT', 'brand'], ['QCT', 'platform_vendor']]
        assert not d.get('hyperscaler') and not d.get('identifiers')
        assert all(isinstance(f, list) and len(f) == 2 and all(isinstance(v, str) and v for v in f) for f in d['facts'])
        assert len({tuple(f) for f in d['facts']}) == len(d['facts'])
        s = next(s for s in curated['sources'] if s['id'] == d['source'])
        assert dict(d['facts'])['source_sha256'] == sha(DATA / s['snapshot'])
finally:
    old.close()
copy_new(LIVE, DATA / 'devices-before-vendor-discovery-2026-10-04.sqlite')
copy_new(MANIFEST, DATA / 'import-manifest-before-vendor-discovery-2026-10-04.json')
copy_new(proposal, INPUT)
entry = json.loads((HERE / 'proposed-d51b-reference-entry.json').read_text())
assert entry['id'] == acceptance['reference_id'] and entry['page_count'] == 2
copy_new(DATA / 'raw/vendor-discovery-qct-2026-10-04/d51b-datasheet.pdf', BUNDLE.parent / entry['pdf']['path'])
copy_new(HERE / 'd51b-embedded-text-wrapper.json', BUNDLE.parent / entry['text']['path'])
bundle = json.loads(OLD_BUNDLE.read_text())
assert len(bundle['documents']) == 44
bundle['documents'].append(entry)
save(BUNDLE, bundle)
_, ready = prepare(BUNDLE)
assert len(ready) == 45
freeze()
copy_new(LIVE, CAND)
c = Catalog(CAND)
try:
    result = {'curated': import_source(c, 'curated', INPUT),
              'reference': import_source(c, 'reference-documents', BUNDLE)}
finally:
    c.db.close()
preservation = audit(LIVE, CAND, 'preserve', OLD_BUNDLE, BUNDLE)
save(HERE / 'candidate-preservation.json', preservation)
new = connect(CAND)
try:
    counts = {table: new.execute('SELECT count(*) FROM ' + table).fetchone()[0]
              for table in ('devices','records','facts','sources','documents','document_pages','hyperscaler_evidence')}
    assert counts == dict(devices=4083,records=5300,facts=44728,sources=281,documents=56,document_pages=3651,hyperscaler_evidence=22), counts
    manifest['expected'].update(devices=counts['devices'], records=counts['records'],
                                sources=dict(new.execute('SELECT id,records_seen FROM sources')),
                                documents=counts['documents'], document_pages=counts['document_pages'])
    source_counts = {r['id']: r['records_seen'] for r in new.execute('SELECT id,records_seen FROM sources') if r['id'] in acceptance['source_ids'] or r['id'] == 'reference-pdf-' + entry['id']}
finally:
    new.close()
reference_jobs = [j for j in manifest['imports'] if j['source']=='reference-documents']
assert len(reference_jobs)==1 and reference_jobs[0]['path']==str(OLD_BUNDLE.relative_to(DATA))
reference_jobs[0]['path']=str(BUNDLE.relative_to(DATA))
manifest['imports'].append({'source':'curated','path':INPUT.name})
paths = [INPUT,BUNDLE,BUNDLE.parent / entry['pdf']['path'],BUNDLE.parent / entry['text']['path']]
paths.extend(DATA / s['snapshot'] for s in curated['sources'])
for path in paths:
    manifest['files'][str(path.relative_to(DATA))]=sha(path)
assert all(manifest['files'][k]==v for k,v in old_manifest['files'].items())
temporary = DATA / '.manifest-vendor-discovery-2026-10-04.json'
save(temporary,manifest)
read_manifest(temporary)
freeze()
os.replace(temporary,MANIFEST)
for name in ('reference_documents','rebuild','buying'):
    run(['-m','unittest','discover','-s','tests','-p','test_'+name+'.py'],HERE / ('tests-'+name+'.log'))
run(['-m','devicehunt','--db',str(REBUILT),'rebuild','--manifest',str(MANIFEST)], HERE / 'clean-rebuild.log')
run(['scripts/compare_rebuild.py',str(REBUILT),'--database',str(CAND),'--output-prefix',TAG], HERE / 'compare-rebuild.log')
save(HERE / 'candidate-rebuild-counter-equality.json', audit(CAND,REBUILT,'equal'))
cli_before = sha(CAND)
reports=[]
for model in ('QuantaGrid D51B-2U','QuantaGrid D52BQ-2U'):
    result = subprocess.run([sys.executable,'-m','devicehunt','--db',str(CAND),'buying-report',model,'--format','json'],check=True,capture_output=True,text=True)
    report=json.loads(result.stdout)
    assert report['vendor'] is None and report['match_status']=='exact_records_found'
    assert any(r['source_id'] in acceptance['source_ids'] for r in report['matched_records'])
    reports.append(report)
assert sha(CAND)==cli_before
save(HERE / 'candidate-buying-reports.json',reports)
freeze()
read_manifest(MANIFEST)
save(HERE / 'stage-verification.json',{'candidate':str(CAND.relative_to(ROOT)),'candidate_sha256':sha(CAND),'rebuilt':str(REBUILT.relative_to(ROOT)),'rebuilt_sha256':sha(REBUILT),'live_sha256':sha(LIVE),'manifest_sha256':sha(MANIFEST),'counts':counts,'new_source_counts':source_counts,'imports':result if isinstance(result,dict) else {'curated':2,'reference_documents':45},'manifest_jobs':len(manifest['imports']),'manifest_hashed_inputs':len(manifest['files']),'code_test_files_unchanged':len(acceptance['code_sha256']),'accepted_inputs_unchanged':len(acceptance['files']),'read_only_cli_hash_unchanged':True,'logical_tables_equal':43,'both_fts_duplicate_sensitive_equal':True,'promotion':'not authorized; await independent candidate review'})
print('staging complete',counts,'live unchanged',sha(LIVE))

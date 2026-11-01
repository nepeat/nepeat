"""Stage reviewed PSU additions exclusively; never promote authoritative files."""
from contextlib import closing
from pathlib import Path
import copy
import hashlib
import json
import shutil
import sqlite3
import sys

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt import reference_documents, rebuild, review_queue, validation

OUT = Path(__file__).resolve().parent
BATCH = OUT.parent
DATA = ROOT / 'data'
CAND_DB = DATA / 'devices-candidate-psu-2026-10-12.sqlite'
CAND_MANIFEST = DATA / 'import-manifest-candidate-psu-2026-10-12.json'
OLD_BUNDLE = DATA / 'raw/reference-documents-with-smartnic-qct-2026-10-05.json'
NEW_BUNDLE = DATA / 'raw/reference-documents-with-psu-2026-10-12.json'
CURATED = DATA / 'raw/psu-facts-2026-10-12/candidate.json'


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def save(path, body):
    with path.open('x') as f:
        json.dump(body, f, indent=2, ensure_ascii=False)
        f.write('\n')


def clone(source, dest):
    with source.open('rb') as src, dest.open('xb') as dst:
        shutil.copyfileobj(src, dst)
    assert sha(source) == sha(dest)


def main():
    for p in (CAND_DB, CAND_MANIFEST, NEW_BUNDLE, CURATED.parent, OUT / 'stage-report.json'):
        assert not p.exists(), p
    for name in ('poppler-support-review.md', 'psu-facts-review.md'):
        assert (BATCH / 'review' / name).is_file(), name
    before = {p.name: sha(p) for p in (DATA / 'devices.sqlite', DATA / 'import-manifest.json')}
    assert before['devices.sqlite'] == '63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e'
    assert before['import-manifest.json'] == '14c2a1f177051b6e62c8e04dabbd9621a3d4baf6b759d2db25a722ba8efc9195'
    manifest = json.loads((DATA / 'import-manifest.json').read_text())
    assert sha(BATCH / 'psu-facts/candidate.json') == '2c55934a430e423c861f04e107e121ff801e9a36e32985daa032308eab4ef756'
    candidate = json.loads((BATCH / 'psu-facts/candidate.json').read_text())
    assert len(candidate['sources']) == len(candidate['devices']) == 6
    assert sum(len(d['facts']) for d in candidate['devices']) == 42
    (CURATED.parent / 'snapshots').mkdir(parents=True)
    clone(BATCH / 'psu-facts/candidate.json', CURATED)
    new_inputs = [CURATED]
    for source in candidate['sources']:
        original = BATCH / 'psu-facts' / source['snapshot']
        assert sha(original) == source['original_sha256']
        dest = CURATED.parent / source['snapshot']
        clone(original, dest)
        new_inputs.append(dest)
    bundle = json.loads(OLD_BUNDLE.read_text())
    original_entries = copy.deepcopy(bundle['documents'])
    assert len(original_entries) == 50
    extraction = json.loads((BATCH / 'psu-extraction-r2/report.json').read_text())
    for item in extraction['documents']:
        original = ROOT / item['source']
        source = next(s for s in candidate['sources'] if s['original_sha256'] == item['pdf_sha256'])
        pdf = DATA / 'raw' / ('psu-2026-10-12-' + original.name)
        text = DATA / 'raw' / ('psu-2026-10-12-' + item['text'])
        clone(original, pdf)
        clone(BATCH / 'psu-extraction-r2' / item['text'], text)
        assert sha(text) == item['text_sha256']
        new_inputs.extend([pdf, text])
        bundle['documents'].append({'id': source['id'], 'url': source['url'],
            'title': original.stem + ' — complete captured PSU document',
            'category': 'power supply documentation', 'tier': source['tier'],
            'scope': source['coverage'], 'page_count': item['pages'],
            'pdf': {'path': pdf.name, 'sha256': sha(pdf)},
            'text': {'path': text.name, 'sha256': sha(text)}})
    assert bundle['documents'][:50] == original_entries
    save(NEW_BUNDLE, bundle)
    _, prepared = reference_documents.prepare(NEW_BUNDLE)
    assert len(prepared) == 55
    new_inputs.append(NEW_BUNDLE)
    jobs = [j for j in manifest['imports'] if j['source'] == 'reference-documents']
    assert len(jobs) == 1 and jobs[0]['path'] == str(OLD_BUNDLE.relative_to(DATA))
    jobs[0]['path'] = str(NEW_BUNDLE.relative_to(DATA))
    manifest['imports'].append({'source': 'curated', 'path': str(CURATED.relative_to(DATA))})
    for p in new_inputs:
        key = str(p.relative_to(DATA))
        assert key not in manifest['files']
        manifest['files'][key] = sha(p)
    release = review_queue.gate_inputs(DATA, manifest)
    with closing(sqlite3.connect((DATA / 'devices.sqlite').as_uri() + '?mode=ro', uri=True)) as src, closing(sqlite3.connect(CAND_DB)) as dst:
        src.backup(dst)
    c = Catalog(CAND_DB)
    try:
        results = {'curated': import_source(c, 'curated', CURATED),
                   'reference-documents': import_source(c, 'reference-documents', NEW_BUNDLE)}
        review_queue.gate_projections(c.db, release)
        c.db.execute('ATTACH DATABASE ? AS live', ((DATA / 'devices.sqlite').as_uri() + '?mode=ro',))
        preservation = {}
        changed_source_hashes = [dict(r) for r in c.db.execute('''
            SELECT l.id,l.sha256 AS old_sha256,m.sha256 AS new_sha256
            FROM live.sources l JOIN main.sources m ON m.id=l.id
            WHERE l.sha256 IS NOT m.sha256 ORDER BY l.id''')]
        assert {r['id'] for r in changed_source_hashes} == {'reference-pdf-' + e['id'] for e in original_entries}
        assert all(r['old_sha256'] == sha(OLD_BUNDLE) and r['new_sha256'] == sha(NEW_BUNDLE)
                   for r in changed_source_hashes)
        for (table,) in c.db.execute("SELECT name FROM live.sqlite_master WHERE type='table'").fetchall():
            if table.startswith(('sqlite_', 'search_index', 'document_search')):
                continue
            cols = [r[1] for r in c.db.execute('PRAGMA main.table_info("' + table + '")')
                    if r[1] not in ('observed_at', 'fetched_at', 'cache_path')]
            if table == 'sources':
                cols.remove('sha256')  # Complete bundle hash changes are checked explicitly above.
            fields = ','.join('"' + x + '"' for x in cols)
            query = f'SELECT {fields},count(*) AS multiplicity FROM {{}}."{table}" GROUP BY {fields}'
            missing = c.db.execute('SELECT count(*) FROM (' + query.format('live') + ' EXCEPT ' + query.format('main') + ')').fetchone()[0]
            preservation[table] = missing
        assert not any(preservation.values()), preservation
        counts = {name: c.db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
                  for name in ('devices', 'records', 'facts', 'sources', 'documents', 'document_pages')}
        assert counts == {'devices': 5115, 'records': 7236, 'facts': 74171, 'sources': 2087, 'documents': 66, 'document_pages': 3779}, counts
        manifest['expected'].update({k: counts[k] for k in ('devices', 'records', 'documents', 'document_pages')})
        manifest['expected']['sources'] = dict(c.db.execute('SELECT id,records_seen FROM sources'))
        integrity = [r[0] for r in c.db.execute('PRAGMA integrity_check')]
        fk = c.db.execute('PRAGMA foreign_key_check').fetchall()
        assert integrity == ['ok'] and not fk
        baseline = validation.run(c.db, ROOT / 'validation/owned-hardware.json')
        assert baseline['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}, baseline['summary']
    finally:
        c.db.close()
    save(CAND_MANIFEST, manifest)
    rebuild.read_manifest(CAND_MANIFEST)
    assert before == {p.name: sha(p) for p in (DATA / 'devices.sqlite', DATA / 'import-manifest.json')}
    report = {'counts': counts, 'import_results': results, 'preservation_missing_rows': preservation,
              'preservation_excludes': ['observed_at', 'fetched_at', 'cache_path', 'FTS internal tables'],
              'expected_complete_bundle_source_hash_changes': changed_source_hashes,
              'integrity': integrity, 'foreign_key_violations': len(fk), 'baseline': baseline['summary'],
              'complete_reference_entries': len(prepared), 'original_entries_unchanged': True,
              'campaign_gate': 'inputs and projections pass', 'manifest_jobs': len(manifest['imports']),
              'manifest_inputs': len(manifest['files']), 'live_unchanged': before,
              'candidate_manifest_sha256': sha(CAND_MANIFEST), 'candidate_database_sha256': sha(CAND_DB),
              'new_input_hashes': {str(p.relative_to(DATA)): sha(p) for p in new_inputs}}
    save(OUT / 'stage-report.json', report)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

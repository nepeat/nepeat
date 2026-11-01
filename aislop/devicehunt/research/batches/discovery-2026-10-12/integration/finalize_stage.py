"""Finish the interrupted stage using read-only URI connections; no re-import."""
import json
import sqlite3
from contextlib import closing
from stage import (ROOT, OUT, BATCH, DATA, CAND_DB, CAND_MANIFEST, OLD_BUNDLE,
                   NEW_BUNDLE, CURATED, sha, save, rebuild, review_queue, validation,
                   reference_documents)


def main():
    assert not CAND_MANIFEST.exists() and not (OUT / 'stage-report.json').exists()
    before = {p.name: sha(p) for p in (DATA / 'devices.sqlite', DATA / 'import-manifest.json')}
    assert before == {'devices.sqlite': '63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e',
                      'import-manifest.json': '14c2a1f177051b6e62c8e04dabbd9621a3d4baf6b759d2db25a722ba8efc9195'}
    candidate_hash = '2c55934a430e423c861f04e107e121ff801e9a36e32985daa032308eab4ef756'
    assert sha(CURATED) == sha(BATCH / 'psu-facts/candidate.json') == candidate_hash
    candidate = json.loads(CURATED.read_text())
    bundle = json.loads(NEW_BUNDLE.read_text())
    original_entries = json.loads(OLD_BUNDLE.read_text())['documents']
    assert len(original_entries) == 50 and bundle['documents'][:50] == original_entries
    _, prepared = reference_documents.prepare(NEW_BUNDLE)
    assert len(prepared) == 55
    new_inputs = [CURATED, NEW_BUNDLE]
    for source in candidate['sources']:
        dest = CURATED.parent / source['snapshot']
        assert sha(dest) == sha(BATCH / 'psu-facts' / source['snapshot']) == source['original_sha256']
        new_inputs.append(dest)
    extraction = json.loads((BATCH / 'psu-extraction-r2/report.json').read_text())
    for entry, item in zip(bundle['documents'][50:], extraction['documents'], strict=True):
        pdf, text = (DATA / 'raw' / entry[k]['path'] for k in ('pdf', 'text'))
        assert sha(pdf) == sha(ROOT / item['source']) == item['pdf_sha256']
        assert sha(text) == sha(BATCH / 'psu-extraction-r2' / item['text']) == item['text_sha256']
        new_inputs.extend([pdf, text])
    manifest = json.loads((DATA / 'import-manifest.json').read_text())
    jobs = [j for j in manifest['imports'] if j['source'] == 'reference-documents']
    assert len(jobs) == 1 and jobs[0]['path'] == str(OLD_BUNDLE.relative_to(DATA))
    jobs[0]['path'] = str(NEW_BUNDLE.relative_to(DATA))
    manifest['imports'].append({'source': 'curated', 'path': str(CURATED.relative_to(DATA))})
    for p in new_inputs:
        key = str(p.relative_to(DATA))
        assert key not in manifest['files']
        manifest['files'][key] = sha(p)
    release = review_queue.gate_inputs(DATA, manifest)
    candidate_before = sha(CAND_DB)
    with closing(sqlite3.connect(CAND_DB.as_uri() + '?mode=ro', uri=True)) as db:
        db.row_factory = sqlite3.Row
        db.execute('ATTACH DATABASE ? AS live', ((DATA / 'devices.sqlite').as_uri() + '?mode=ro',))
        review_queue.gate_projections(db, release)
        changes = [dict(r) for r in db.execute('''SELECT l.id,l.sha256 AS old_sha256,m.sha256 AS new_sha256
            FROM live.sources l JOIN main.sources m ON m.id=l.id
            WHERE l.sha256 IS NOT m.sha256 ORDER BY l.id''')]
        assert {r['id'] for r in changes} == {'reference-pdf-' + e['id'] for e in original_entries}
        assert all(r['old_sha256'] == sha(OLD_BUNDLE) and r['new_sha256'] == sha(NEW_BUNDLE) for r in changes)
        preservation = {}
        for (table,) in db.execute("SELECT name FROM live.sqlite_master WHERE type='table'").fetchall():
            if table.startswith(('sqlite_', 'search_index', 'document_search')):
                continue
            cols = [r[1] for r in db.execute('PRAGMA main.table_info("' + table + '")')
                    if r[1] not in ('observed_at', 'fetched_at', 'cache_path')]
            if table == 'sources':
                cols.remove('sha256')
            fields = ','.join('"' + x + '"' for x in cols)
            query = f'SELECT {fields},count(*) AS multiplicity FROM {{}}."{table}" GROUP BY {fields}'
            preservation[table] = db.execute('SELECT count(*) FROM (' + query.format('live') + ' EXCEPT ' + query.format('main') + ')').fetchone()[0]
        assert not any(preservation.values()), preservation
        counts = {name: db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
                  for name in ('devices', 'records', 'facts', 'sources', 'documents', 'document_pages')}
        assert counts == {'devices': 5115, 'records': 7236, 'facts': 74171, 'sources': 2087, 'documents': 66, 'document_pages': 3779}, counts
        manifest['expected'].update({k: counts[k] for k in ('devices', 'records', 'documents', 'document_pages')})
        manifest['expected']['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
        integrity = [r[0] for r in db.execute('PRAGMA integrity_check')]
        fk = db.execute('PRAGMA foreign_key_check').fetchall()
        assert integrity == ['ok'] and not fk
        baseline = validation.run(db, ROOT / 'validation/owned-hardware.json')
        assert baseline['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
    save(CAND_MANIFEST, manifest)
    rebuild.read_manifest(CAND_MANIFEST)
    assert candidate_before == sha(CAND_DB)
    assert before == {p.name: sha(p) for p in (DATA / 'devices.sqlite', DATA / 'import-manifest.json')}
    report = {'counts': counts, 'preservation_missing_rows': preservation,
        'preservation_excludes': ['observed_at', 'fetched_at', 'cache_path', 'FTS internal tables'],
        'expected_complete_bundle_source_hash_changes': changes,
        'integrity': integrity, 'foreign_key_violations': len(fk), 'baseline': baseline['summary'],
        'complete_reference_entries': len(prepared), 'original_entries_unchanged': True,
        'campaign_gate': 'inputs and projections pass', 'manifest_jobs': len(manifest['imports']),
        'manifest_inputs': len(manifest['files']), 'live_unchanged': before,
        'candidate_manifest_sha256': sha(CAND_MANIFEST), 'candidate_database_sha256': sha(CAND_DB),
        'new_input_hashes': {str(p.relative_to(DATA)): sha(p) for p in new_inputs},
        'recovery': 'stage.py imported isolated candidate, then ATTACH failed on URI-disabled Catalog connection; finalize_stage.py read-only URI audit completed without re-import or catalog writes',
        'failed_script_sha256': sha(OUT / 'stage.py'), 'finalizer_sha256': sha(OUT / 'finalize_stage.py')}
    save(OUT / 'stage-report.json', report)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

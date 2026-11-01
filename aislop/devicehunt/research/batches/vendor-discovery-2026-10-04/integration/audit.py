"""Batch-local, read-only duplicate-sensitive audit; no integration mutations."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import sqlite3

TABLES = ('fcc_sitemap_pages fcc_sitemap_urls fcc_company_rows japan_equipment '
          'public_equipment_rows ghana_equipment nigeria_equipment ised_equipment '
          'energystar_datacenter energystar_server_archives energystar_servers '
          'sources devices records affiliations identifiers facts hyperscaler_evidence '
          'fcc_applications fcc_exhibits fcc_grantees equipment_register bahrain_equipment '
          'anatel_equipment ntra_equipment tec_equipment ieee_assignments pci_entries '
          'bsmi_certificates bsmi_product_certificates bsmi_voluntary_certificates '
          'bsmi_declarations rra_equipment ncc_equipment fcc_id_index documents '
          'document_pages fcc_detail_pages fcc_detail_links fcc_text_excerpts '
          'search_index document_search inspur_support_entries').split()
VOLATILE = {'observed_at', 'fetched_at', 'cache_path'}


def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1048576), b''):
            h.update(chunk)
    return h.hexdigest()


def connect(path):
    db = sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)
    db.row_factory = sqlite3.Row
    return db


def rows(db, table, excluded=(), exact=False):
    columns = [r['name'] for r in db.execute('PRAGMA table_info("' + table + '")')
               if (exact or r['name'] not in VOLATILE) and r['name'] not in excluded]
    if not columns:
        raise ValueError('Missing table: ' + table)
    # Compare FTS content and multiplicity; physical rowids are not content.
    fields = ','.join('"' + name + '"' for name in columns)
    return Counter(tuple(r) for r in db.execute('SELECT ' + fields + ' FROM "' + table + '"'))


def audit(old_path, new_path, mode, old_bundle=None, new_bundle=None):
    paths = [Path(old_path), Path(new_path)]
    before = [sha(p) for p in paths]
    report = {'mode': mode, 'database_sha256': before, 'tables': {}}
    old, new = [connect(p) for p in paths]
    try:
        for db in (old, new):
            assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
            assert not db.execute('PRAGMA foreign_key_check').fetchall()
            assert db.execute('PRAGMA user_version').fetchone()[0] == 28
        for table in TABLES:
            excluded = ('sha256', 'fetched_at', 'cache_path') if mode == 'preserve' and table == 'sources' else ()
            left, right = rows(old, table, excluded, exact=mode == 'preserve'), rows(new, table, excluded, exact=mode == 'preserve')
            removed, added = left - right, right - left
            item = {'before': sum(left.values()), 'after': sum(right.values()),
                    'removed': sum(removed.values()), 'added': sum(added.values())}
            report['tables'][table] = item
            if mode == 'equal':
                assert not removed and not added, (table, item)
            elif table != 'search_index':
                assert not removed, (table, item)
        report['sources_before'] = dict(old.execute('SELECT id,records_seen FROM sources'))
        report['sources_after'] = dict(new.execute('SELECT id,records_seen FROM sources'))
        # Ordinary source-row comparison above ignores only approved volatile metadata.
        report['source_metadata_changes'] = []
        reference_ids = set()
        if mode == 'preserve':
            if not old_bundle or not new_bundle:
                raise ValueError('Preservation requires explicit old/new complete bundle paths')
            entries = json.loads(Path(old_bundle).read_text())['documents']
            assert len(entries) == 44
            reference_ids = {'reference-pdf-' + e['id'] for e in entries}
            new_entries = json.loads(Path(new_bundle).read_text())['documents']
            assert new_entries[:44] == entries
        expected_document_fts = Counter(tuple(r) for r in new.execute(
            'SELECT document_key,page_number,text FROM document_pages'))
        assert expected_document_fts == rows(new, 'document_search', exact=True)
        report['document_fts_content_matches_pages'] = True
        if mode == 'preserve':
            changed_devices = [r[0] for r in new.execute(
                "SELECT DISTINCT device_id FROM records WHERE source_id IN ('qct-d51b-datasheet-mirror-2017','qct-d52bq-public-product-2026')")]
            assert len(changed_devices) == 2
            old_search = rows(old, 'search_index', exact=True)
            new_search = rows(new, 'search_index', exact=True)
            for row, count in old_search.items():
                if row[0] not in changed_devices:
                    assert new_search[row] == count, ('unrelated device FTS changed', row[0])
            report['device_fts_permitted_changes'] = changed_devices
        for r in old.execute('SELECT * FROM sources ORDER BY id'):
            after = new.execute('SELECT * FROM sources WHERE id=?', (r['id'],)).fetchone()
            changed = {k: {'before': r[k], 'after': after[k]} for k in r.keys()
                       if r[k] != after[k]}
            if mode == 'preserve':
                allowed = set()
                if r['id'] in reference_ids:
                    allowed = {'sha256', 'cache_path', 'fetched_at'}
                    assert r['sha256'] == sha(old_bundle)
                    assert after['sha256'] == sha(new_bundle)
                    assert Path(r['cache_path']).resolve() == Path(old_bundle).resolve()
                    assert Path(after['cache_path']).resolve() == Path(new_bundle).resolve()
                assert set(changed) <= allowed, (r['id'], changed)
            if changed:
                report['source_metadata_changes'].append({'id': r['id'], 'changes': changed})
        from devicehunt import validation
        report['owned'] = validation.run(new, Path('validation/owned-hardware.json'))
        report['bytedance'] = validation.run(new, Path('validation/vendor-examples.json'))
        assert report['owned']['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
        assert report['owned']['evidence_summary'] == {'document_snapshot_hits': 9,
                'label_photo_only_hits': 1, 'unresolved_identity_gaps': 0}
        assert report['bytedance']['summary'] == {'total': 2, 'hits': 0, 'gaps': 2}
        assert new.execute('SELECT count(*) FROM search_index').fetchone()[0] == new.execute('SELECT count(*) FROM devices').fetchone()[0]
        assert new.execute('SELECT count(*) FROM document_search').fetchone()[0] == new.execute('SELECT count(*) FROM document_pages').fetchone()[0]
    finally:
        old.close()
        new.close()
    assert before == [sha(p) for p in paths], 'Read-only audit changed database bytes'
    report['read_only_hash_unchanged'] = True
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('old')
    parser.add_argument('new')
    parser.add_argument('--mode', choices=('preserve', 'equal'), required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--old-bundle')
    parser.add_argument('--new-bundle')
    args = parser.parse_args()
    output = Path(args.output).resolve()
    if output.parent != Path(__file__).resolve().parent or output.exists():
        parser.error('Output must be a fresh file beside this helper')
    result = audit(args.old, args.new, args.mode, args.old_bundle, args.new_bundle)
    output.write_text(json.dumps(result, indent=2) + '\n')
    print('Verified', len(TABLES), 'logical tables and both duplicate-sensitive FTS contents.')

"""Small isolated legacy-behavior probes; production modules are read-only."""
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile

from devicehunt.catalog import Catalog
from devicehunt.importers import curated

HERE = Path(__file__).resolve().parent


def source(sid, suffix=''):
    return {'id': sid, 'url': 'https://example.test/' + str(sid), 'tier': 'vendor',
            'license': 'fixture', 'coverage': 'fixture' + suffix}


def row(sid, key, model, title=None, kind='fixture', **extra):
    return {'source': sid, 'key': key, 'brand': 'Fixture', 'model': model,
            'title': title or model, 'kind': kind, 'affiliations': [], **extra}


def models(c):
    return [dict(r) for r in c.db.execute('SELECT model,title,kind,revision FROM devices ORDER BY model')]


def run():
    results = {}
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        def catalog(name):
            return Catalog(root / (name + '.sqlite'))
        def load(c, name, data):
            path = root / (name + '.json')
            path.write_text(json.dumps(data))
            return curated(c, path)
        # A's orphan cleanup occurs before B gets a chance to recreate the same identity.
        for cleanup in ('legacy', 'deferred'):
            c = catalog(cleanup)
            load(c, 'seed-' + cleanup, {'sources': [source('A'), source('B')],
                                      'devices': [row('A', 'one', 'M1', 'old title', 'old kind')]})
            incoming = {'sources': [source('A'), source('B')],
                        'devices': [row('B', 'one', 'M1', 'new title', 'new kind')]}
            if cleanup == 'legacy':
                load(c, 'move', incoming)
            else:
                # Hand-emulate the proposed final-cleanup-only optimization on this tiny fixture.
                for s in incoming['sources']:
                    c.source(s['id'], s['url'], s['tier'], s['license'], s['coverage'])
                    rows = [d for d in incoming['devices'] if d['source'] == s['id']]
                    for d in rows:
                        c.add(s['id'], d['key'], s['url'], d['brand'], d['model'], d['title'], d['kind'], d, d['affiliations'])
                    c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (len(rows),s['id']))
                c.db.execute('DELETE FROM devices WHERE id NOT IN (SELECT device_id FROM records)')
                c.reindex(); c.db.commit()
            results[cleanup + '_source_move'] = models(c)
            c.db.close()
        c = catalog('duplicates')
        returned = load(c, 'duplicates', {'sources': [source('A', 'first'), source('A', 'last')],
            'devices': [row('A', 'same', 'M1', 'first identity title'),
                        row('A', 'same', 'M1', 'last record title', revision='IGNORED'),
                        {'source': 'unreferenced', 'malformed_ignored_field': []}]})
        results['repeated_sources_keys_and_ignored_row'] = {
            'returned': returned, 'records': c.db.execute('SELECT COUNT(*) FROM records').fetchone()[0],
            'records_seen': c.db.execute('SELECT records_seen FROM sources').fetchone()[0],
            'coverage': c.db.execute('SELECT coverage FROM sources').fetchone()[0],
            'devices': models(c), 'raw_json': json.loads(c.db.execute('SELECT raw_json FROM records').fetchone()[0])}
        c.db.close()
        c = catalog('values')
        values = [None, '', 0, False, 1, True, [], {}, ['x'], {'x': 1}, 1.5]
        load(c, 'values', {'sources':[source('A')], 'devices':[row('A', 'one', 'M1',
            identifiers=[['part_number',v] for v in values], facts=[['raw_literal',v] for v in values])]})
        results['fact_identifier_values'] = {'input': values,
            'facts': [r[0] for r in c.db.execute('SELECT value FROM facts ORDER BY value')],
            'identifiers': [r[0] for r in c.db.execute('SELECT value FROM identifiers ORDER BY value')]}
        c.db.close()
        c = catalog('caller')
        c.db.execute("INSERT INTO sources(id,url,tier,license,coverage) VALUES('caller','u','t','l','c')")
        assert c.db.in_transaction
        load(c, 'caller', {'sources': [source('A')], 'devices': [row('A','one','M1')]})
        results['legacy_caller_transaction'] = {'in_transaction_after_success': c.db.in_transaction}
        c.db.rollback()
        results['legacy_caller_transaction']['caller_row_survives_rollback'] = bool(c.db.execute("SELECT 1 FROM sources WHERE id='caller'").fetchone())
        c.db.close()
        c = catalog('empty')
        results['empty_sources_missing_devices'] = {'returned': load(c, 'empty', {'sources':[]}),
                                                   'in_transaction':c.db.in_transaction}
        c.db.close()
    (HERE/'probe-results.json').write_text(json.dumps({'scope':'five tiny temporary catalogs only; no live mutation', 'results': results},indent=2)+'\n')
    print(json.dumps(results,indent=2))


if __name__ == '__main__':
    run()

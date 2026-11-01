"""Isolated demonstration of later-row failure across curated source commits."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt.importers import curated


def source(sid):
    return {'id': sid, 'url': 'https://example.test/' + sid, 'tier': 'vendor',
            'license': 'fixture', 'coverage': 'fixture'}


def row(sid, model):
    return {'source': sid, 'key': 'one', 'brand': 'Fixture', 'model': model,
            'title': model, 'kind': 'fixture', 'affiliations': [], 'facts': []}


def main():
    live = ROOT / 'data/devices.sqlite'
    before = hashlib.sha256(live.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        c = Catalog(tmp / 'fixture.sqlite')
        seed = tmp / 'seed.json'
        seed.write_text(json.dumps({'sources': [source('one'), source('two')],
                                    'devices': [row('one', 'old-one'), row('two', 'old-two')]}))
        curated(c, seed)
        old = [r[0] for r in c.db.execute('SELECT model FROM devices ORDER BY model')]
        bad = row('two', 'new-two')
        del bad['model']
        incoming = tmp / 'incoming.json'
        incoming.write_text(json.dumps({'sources': [source('one'), source('two')],
                                        'devices': [row('one', 'new-one'), bad]}))
        error = None
        try:
            curated(c, incoming)
        except KeyError as exc:
            error = {'type': type(exc).__name__, 'message': str(exc)}
            c.db.rollback()
        after = [r[0] for r in c.db.execute('SELECT model FROM devices ORDER BY model')]
        c.db.close()
        assert error and old == ['old-one', 'old-two'] and after == ['new-one', 'old-two']
    assert before == hashlib.sha256(live.read_bytes()).hexdigest()
    result = {'scope': 'temporary two-source fixture only; no live import', 'error': error,
              'before_models': old, 'after_rollback_models': after,
              'first_source_replacement_survives_later_failure': True,
              'live_sha256_unchanged': before,
              'importer_sha256': hashlib.sha256((ROOT / 'devicehunt/importers.py').read_bytes()).hexdigest(),
              'catalog_sha256': hashlib.sha256((ROOT / 'devicehunt/catalog.py').read_bytes()).hexdigest(),
              'performance_observation': 'Catalog.finish reindexes and commits once per curated source; current pinned redhat-cert-facts input contains342 sources. This is static call-path inspection, not a benchmark.'}
    with (Path(__file__).parent / 'result.json').open('x') as f:
        json.dump(result, f, indent=2)
        f.write('\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()

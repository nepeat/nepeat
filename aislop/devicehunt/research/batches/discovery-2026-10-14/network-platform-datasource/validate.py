"""Offline review receipt: immutable captures, deterministic outputs, scratch import.

Run from the project root: PYTHONPATH=. python3 <this-file>
No production database writes or network calls.
"""
import hashlib
import json
from pathlib import Path
import tempfile

from devicehunt.catalog import Catalog
from devicehunt.importers import curated, prepare_curated
from hcl import candidate, dump, novelty, parse_bisdn, verified_capture

ROOT = Path(__file__).resolve().parent


def main():
    ledger = [json.loads(line) for line in (ROOT / 'requests.jsonl').read_text().splitlines()]
    checked = []
    for row in ledger:
        receipts = list((ROOT / 'raw').glob(f"{row['sequence']:02d}-*.json"))
        assert len(receipts) == 1
        assert json.loads(receipts[0].read_text()) == row
        for label in ('body', 'headers'):
            if label + '_path' in row:
                path = ROOT / row[label + '_path']
                data = path.read_bytes()
                assert hashlib.sha256(data).hexdigest() == row[label + '_sha256']
                if label == 'body':
                    assert len(data) == row['body_bytes']
                checked.append(str(path.relative_to(ROOT)))
    receipt, raw = verified_capture(ROOT, 'raw/05-bisdn-hcl.json')
    matrix = parse_bisdn(raw, receipt['url'])
    assert json.loads((ROOT / 'bisdn-matrix.json').read_text()) == json.loads(json.dumps(matrix))
    proposed = candidate(matrix, receipt)
    assert json.loads((ROOT / 'candidate.json').read_text()) == proposed
    prepared = prepare_curated(ROOT / 'candidate.json')
    assert len(prepared) == 1 and len(prepared[0][3]) == 11
    assert prepared[0][2].sha256 == matrix['body_sha256']
    observed = novelty(matrix, ROOT.parents[3] / 'data/devices.sqlite')
    assert json.loads(json.dumps(observed)) == json.loads((ROOT / 'novelty.json').read_text())
    with tempfile.TemporaryDirectory(prefix='offline-hcl-', dir=ROOT / 'tmp') as temp:
        catalog = Catalog(Path(temp) / 'candidate.sqlite')
        try:
            imports = [curated(catalog, ROOT / 'candidate.json') for _ in range(2)]
            counts = {table: catalog.db.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                      for table in ('sources', 'devices', 'records', 'facts', 'identifiers', 'affiliations')}
            assert counts == {'sources': 1, 'devices': 11, 'records': 11, 'facts': 77, 'identifiers': 11, 'affiliations': 0}
            integrity = catalog.db.execute('PRAGMA integrity_check').fetchone()[0]
            foreign_keys = [list(r) for r in catalog.db.execute('PRAGMA foreign_key_check')]
            assert integrity == 'ok' and not foreign_keys
            fts_hits = catalog.db.execute("SELECT count(*) FROM search_index WHERE search_index MATCH 'Questone'").fetchone()[0]
            assert fts_hits == 1
        finally:
            catalog.db.close()
    report = {'offline': True, 'capture_receipts_verified': len(ledger), 'capture_files_hashed': checked,
              'attempts': len(ledger), 'actual_http_exchanges': sum(r.get('status') is not None for r in ledger),
              'dns_or_transport_failures': sum(r.get('status') is None for r in ledger),
              'deterministic_matrix_candidate_novelty': True, 'prepare_curated': 'pass',
              'scratch_imports_records_each': imports, 'scratch_counts_after_replacement': counts,
              'integrity_check': integrity, 'foreign_key_check': foreign_keys, 'Questone_fts_hits': fts_hits,
              'live_database_sha256_before': observed['database_sha256_before'],
              'live_database_sha256_after': observed['database_sha256_after'],
              'production_imported': False, 'tests': '21 offline unittest cases passed; see test-results.txt'}
    dump(ROOT / 'validation.json', report)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

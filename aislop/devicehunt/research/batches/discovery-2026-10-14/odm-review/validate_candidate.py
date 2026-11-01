"""Independent read-only novelty checks and isolated curated import proof."""
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile

from devicehunt.catalog import Catalog, digest, normalize
from devicehunt.importers import curated, prepare_curated
from review_build import ROOT, build, canonical, dump, sha


def main():
    source = ROOT.parent / 'odm-source-expansion'
    database = ROOT.parents[3] / 'data/devices.sqlite'
    before = sha(database.read_bytes())
    wal = Path(str(database) + '-wal')
    if wal.exists() and wal.stat().st_size:
        raise ValueError('nonempty WAL; obtain coordinator-pinned input')
    with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('PRAGMA query_only=ON')
        connection.execute('BEGIN')
        devices = connection.execute('SELECT id,brand,model,revision FROM devices ORDER BY id').fetchall()
        identifiers = connection.execute('SELECT DISTINCT d.id,i.kind,i.value FROM identifiers i JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id ORDER BY d.id,i.kind,i.value').fetchall()
        urls = set(row[0] for row in connection.execute('SELECT url FROM sources'))
        schema = connection.execute('PRAGMA user_version').fetchone()[0]
    after = sha(database.read_bytes())
    assert before == after == '0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853'
    discovery = json.loads((source / 'discovery-rows.json').read_text())
    original = json.loads((source / 'catalog-novelty.json').read_text())
    model_index, identifier_index = defaultdict(list), defaultdict(list)
    for row in devices:
        model_index[row[2]].append(row)
    for row in identifiers:
        identifier_index[row[2]].append(row)
    statuses = []
    for number, row in enumerate(discovery):
        models = model_index[row['model']]
        pairs = [d for d in models if d[1] == row['partner']] if row['partner'] else []
        ids = identifier_index[row['model']]
        status = ('exact_printed_partner_and_model' if pairs else
                  'same_literal_model_other_or_unknown_partner' if models else
                  'literal_identifier_hit_only' if ids else 'no_literal_model_or_identifier_hit')
        prior = original['rows'][number]
        assert prior['status'] == status
        assert sorted(d[0] for d in models) == sorted(d['id'] for d in prior['literal_model_hits'])
        assert sorted(d[0] for d in pairs) == sorted(d['id'] for d in prior['exact_partner_model_hits'])
        assert sorted((i[0], i[1], i[2]) for i in ids) == sorted((i['device_id'], i['kind'], i['value']) for i in prior['literal_identifier_hits'])
        statuses.append(status)
    assert dict(Counter(statuses)) == original['counts']
    assert all(url not in urls for url in original['exact_source_url_hits'])
    selection = json.loads((ROOT / 'selection.json').read_text())
    candidate, accepted = build(selection, ROOT / 'raw')
    assert candidate == json.loads((ROOT / 'candidate.json').read_text())
    assert accepted == json.loads((ROOT / 'accepted-rows.json').read_text())
    pairs = sorted({(r['brand'], r['model']) for r in candidate['devices']})
    pair_detail = [{'company': company, 'model': model,
                    'exact_pair_ids': [d[0] for d in model_index[model] if d[1] == company]}
                   for company, model in pairs]
    prepare = prepare_curated(ROOT / 'candidate.json')
    assert len(prepare) == 5 and sum(len(r[3]) for r in prepare) == 214
    assert all(metadata.sha256 == s['original_sha256'] for s, _, metadata, _ in prepare)
    candidate_ids = {digest(json.dumps([normalize(company), model.casefold().strip(), '']).encode())[:20]
                     for company, model in pairs}
    actual_catalog_reuses = candidate_ids & {d[0] for d in devices}
    nonexact_reuses = actual_catalog_reuses - {i for p in pair_detail for i in p['exact_pair_ids']}
    assert len(candidate_ids) == len(pairs) == 154
    # Nonexact case-normalized catalog collisions require a coordinator decision.
    assert not nonexact_reuses, sorted(nonexact_reuses)
    with tempfile.TemporaryDirectory(prefix='curated-proof-', dir=ROOT / 'tmp') as tmp:
        catalog = Catalog(Path(tmp) / 'candidate.sqlite')
        try:
            imported = [curated(catalog, ROOT / 'candidate.json') for _ in range(2)]
            counts = {name: catalog.db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
                      for name in ('sources', 'devices', 'records', 'facts', 'affiliations', 'identifiers', 'hyperscaler_evidence')}
            assert counts == {'sources': 5, 'devices': 154, 'records': 214, 'facts': 2255, 'affiliations': 214, 'identifiers': 0, 'hyperscaler_evidence': 0}
            integrity = catalog.db.execute('PRAGMA integrity_check').fetchone()[0]
            foreign_keys = [list(row) for row in catalog.db.execute('PRAGMA foreign_key_check')]
            assert integrity == 'ok' and not foreign_keys
            roles = dict(catalog.db.execute('SELECT role,count(*) FROM affiliations GROUP BY role'))
            assert roles == {'documentation_vendor': 38, 'listing_holder': 39, 'program_partner': 137}
            fts = catalog.db.execute("SELECT count(*) FROM search_index WHERE search_index MATCH 'B8056G68CE12HR'").fetchone()[0]
            assert fts > 0
        finally:
            catalog.db.close()
    result = {'reviewer': '/root/network_bulk_full', 'production_writes': False, 'network_calls': 0,
              'schema': schema, 'database_sha256_before': before, 'database_sha256_after': after,
              'all_discovery_novelty_rows_recomputed': len(statuses), 'novelty_counts': dict(Counter(statuses)),
              'exact_source_urls_absent': len(original['exact_source_url_hits']),
              'accepted_pair_count': len(pair_detail),
              'accepted_pairs_with_exact_live_match': sum(bool(p['exact_pair_ids']) for p in pair_detail),
              'accepted_pairs_without_exact_live_match': sum(not p['exact_pair_ids'] for p in pair_detail),
              'actual_catalog_identity_reuses': len(actual_catalog_reuses), 'nonexact_catalog_identity_reuses': sorted(nonexact_reuses),
              'pair_details': pair_detail, 'prepare_curated_sources': len(prepare),
              'deterministic_candidate_and_accepted_rows': True,
              'isolated_import_and_replacement_records': imported, 'isolated_final_counts': counts,
              'roles': roles, 'integrity_check': integrity, 'foreign_key_check': foreign_keys,
              'sample_fts_hits': fts, 'candidate_sha256': sha((ROOT / 'candidate.json').read_bytes())}
    dump(ROOT / 'candidate-validation.json', result)
    print(json.dumps({k:v for k,v in result.items() if k != 'pair_details'}, indent=2))


if __name__ == '__main__':
    main()

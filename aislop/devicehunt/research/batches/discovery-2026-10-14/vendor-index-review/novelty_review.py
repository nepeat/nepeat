"""Independent stable-snapshot, literal-label and URL novelty audit; read only."""
from collections import Counter, defaultdict
import json
from pathlib import Path
import sqlite3
from audit_inputs import HERE, ROOT, SOURCE, endpoint, normal, sha


def main():
    path = ROOT / 'data/devices.sqlite'
    before = sha(path)
    wal = Path(str(path) + '-wal')
    if wal.exists() and wal.stat().st_size:
        raise ValueError('nonempty WAL requires a coordinator-pinned snapshot')
    with sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True) as db:
        db.execute('PRAGMA query_only=ON')
        db.execute('BEGIN')
        sources = {r[0] for r in db.execute('SELECT url FROM sources')}
        records = {r[0] for r in db.execute('SELECT source_url FROM records')}
        devices = list(db.execute('SELECT id,brand,model FROM devices ORDER BY id'))
        identifiers = list(db.execute('SELECT DISTINCT r.device_id,i.kind,i.value FROM identifiers i JOIN records r ON r.id=i.record_id ORDER BY r.device_id,i.kind,i.value'))
    after = sha(path)
    assert before == after == '0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853'
    known_urls = sources | records
    known_endpoints = {endpoint(u) for u in known_urls}
    models, ids = defaultdict(list), defaultdict(list)
    for did,brand,model in devices:
        models[normal(model).casefold()].append({'device_id':did,'brand':brand,'model':model})
    for did,kind,value in identifiers:
        ids[normal(value).casefold()].append({'device_id':did,'kind':kind,'value':value})
    dataset = json.loads((SOURCE/'discovery-ready.json').read_text())
    for row in dataset['rows']:
        assert row['raw_url_in_catalog'] == (row['url'] in known_urls)
        assert row['endpoint_in_catalog'] == (endpoint(row['url']) in known_endpoints)
        labels = list(dict.fromkeys(x for x in [row['printed_label'],row.get('document_heading','')] if x))
        expected_models = {label:models[normal(label).casefold()] for label in labels if normal(label).casefold() in models}
        expected_ids = {label:ids[normal(label).casefold()] for label in labels if normal(label).casefold() in ids}
        expected_tokens = {token:models[token.casefold()] for token in row['identifier_token_leads'] if token.casefold() in models}
        assert row['complete_label_model_matches'] == expected_models
        assert row['complete_label_identifier_matches'] == expected_ids
        assert row['token_model_matches_leads_only'] == expected_tokens
    docs = [r for r in dataset['rows'] if r['classification'] in ('product_document_link','legacy_product_documentation_index_link','product_documentation_index_link')]
    result = {'result':'pass','rows_independently_compared':len(dataset['rows']),
              'raw_urls_absent':len({r['url'] for r in dataset['rows'] if r['url'] not in known_urls}),
              'endpoint_keys_absent':len({endpoint(r['url']) for r in dataset['rows'] if endpoint(r['url']) not in known_endpoints}),
              'document_index_rows':len(docs),'document_index_endpoint_keys':len({endpoint(r['url']) for r in docs}),
              'document_index_endpoint_keys_absent':len({endpoint(r['url']) for r in docs if endpoint(r['url']) not in known_endpoints}),
              'whole_label_model_match_rows':sum(bool(r['complete_label_model_matches']) for r in dataset['rows']),
              'whole_label_identifier_match_rows':sum(bool(r['complete_label_identifier_matches']) for r in dataset['rows']),
              'catalog_sha256_before':before,'catalog_sha256_after':after,'nonempty_wal':False,
              'snapshot_transaction':'BEGIN + query_only + mode=ro',
              'limits':'query-free endpoints are comparison keys; label equality is whitespace-normalized/casefolded; token hits remain heuristic leads; no aliases or hardware claims'}
    (HERE/'novelty-review.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__ == '__main__':
    main()

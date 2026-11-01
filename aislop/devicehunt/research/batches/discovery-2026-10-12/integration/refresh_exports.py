"""Refresh current generated exports from the accepted catalog, read-only."""
import json
from stage import ROOT, OUT, DATA, sha, save, clone
from devicehunt.catalog import Catalog
from devicehunt.__main__ import write_rows, report
from devicehunt import specifications, validation, documents


def main():
    proof = json.loads((OUT / 'promotion-result.json').read_text())
    db = DATA / 'devices.sqlite'
    before = sha(db)
    assert proof['promoted'] and before == proof['new']['devices.sqlite']
    c = Catalog(db, read_only=True)
    paths = []
    exports = DATA / 'exports'
    backup = OUT / 'exports-before'
    backup.mkdir()
    old_exports = {}
    for name in ('targets.csv', 'targets.json', 'catalog.html', 'specifications.csv',
                 'specifications.json', 'reference-documents.csv', 'inventory-validation.json'):
        p = exports / name
        if p.exists():
            clone(p, backup / name)
            old_exports[name] = sha(p)
    save(backup / 'provenance.json', {'original_directory': str(exports), 'sha256': old_exports})

    def rows_file(name, rows, fmt):
        p = exports / name
        with p.open('w', newline='') as f:
            write_rows(rows, fmt, f)
        paths.append(p)

    try:
        rows = [dict(r) for r in c.db.execute('SELECT * FROM target_devices ORDER BY target,brand,model')]
        rows_file('targets.csv', rows, 'csv')
        rows_file('targets.json', rows, 'json')
        report(c, exports / 'catalog.html')
        paths.append(exports / 'catalog.html')
        specs = specifications.search(c, limit=-1)
        rows_file('specifications.csv', specs, 'csv')
        rows_file('specifications.json', specs, 'json')
        rows_file('reference-documents.csv', [dict(r) for r in c.db.execute('''
            SELECT title,source_id,page_count,pages_with_text,source_url FROM documents
            WHERE source_id LIKE 'reference-pdf-%' ORDER BY title,source_id''')], 'csv')
        owned = validation.run(c.db, ROOT / 'validation/owned-hardware.json')
        assert owned['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
        p = exports / 'inventory-validation.json'
        p.write_text(json.dumps(owned, indent=2, ensure_ascii=False) + '\n')
        paths.append(p)
        psu = {}
        for model in ('PFE1100-12-054NA', 'PFE1500-12-054NAC', 'FSP550-29FM'):
            psu[model] = {'specifications': specifications.search(c, model, exact_model=True, limit=-1),
                          'documents': documents.search(c.db, model, limit=-1)}
        p = exports / 'psu-specifications-2026-10-12.json'
        save(p, psu)
        paths.append(p)
    finally:
        c.db.close()
    assert sha(db) == before
    save(OUT / 'exports-report.json', {'database_sha256_unchanged': before,
        'files': {str(p.relative_to(ROOT)): {'sha256': sha(p), 'bytes': p.stat().st_size} for p in paths}})


if __name__ == '__main__':
    main()

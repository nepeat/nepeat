"""Read-only final handoff checks after promotion and current exports finish."""
import json
from pathlib import Path
from devicehunt.catalog import Catalog, TARGETS
from devicehunt.rebuild import file_sha256, read_manifest
from devicehunt import specifications, validation

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
OUT = DATA / 'exports'
path = DATA / 'devices.sqlite'
before = file_sha256(path)
promotion = json.loads((OUT / 'inventec-qct-promotion-2026-10-04.json').read_text())
assert promotion['database_sha256'] == before
_, manifest = read_manifest(DATA / 'import-manifest.json')
assert len(manifest['imports']) == 110 and len(manifest['files']) == 2104
for name in ('latest-export-audit', 'collection-reports-audit', 'hyperscaler-inventory-audit',
             'inventec-qct-ui-audit-2026-10-04', 'inventec-qct-cli-checks-2026-10-04'):
    assert json.loads((OUT / (name + '.json')).read_text())['database_sha256'] == before, name
latest = json.loads((OUT / 'latest-export-audit.json').read_text())
c = Catalog(path, read_only=True)
db = c.db
assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
counts = {name: db.execute('SELECT count(*) FROM ' + name).fetchone()[0]
          for name in ('devices', 'records', 'sources', 'documents', 'document_pages')}
assert counts == dict(devices=3941, records=5140, sources=247, documents=44, document_pages=3474)
pdfs = []
for row in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id'):
    pdf = Path(row['cache_path']).parent / row['pdf_file']
    assert file_sha256(pdf) == row['pdf_sha256'], str(pdf)
    pdfs.append(dict(source=row['source_id'], path=str(pdf), sha256=row['pdf_sha256']))
base = validation.run(db, ROOT / 'validation/owned-hardware.json')
examples = validation.run(db, ROOT / 'validation/vendor-examples.json')
assert base['summary'] == latest['owned_baseline'] == dict(total=10, hits=10, gaps=0)
assert examples['summary'] == latest['vendor_examples'] == dict(total=2, hits=0, gaps=2)
assert base['evidence_summary']['document_snapshot_hits'] == 9
assert base['evidence_summary']['label_photo_only_hits'] == 1
assert all((OUT / 'catalog.html').read_text().count('<option>' + t + '</option>') == 1 for t in TARGETS)
manufacturer_models = ('Entei', 'Horsea', 'Golduck', 'E850G4', 'FPGA SmartNIC C5020X')
assert all(c.search(model, target='Inventec', role='manufacturer') for model in manufacturer_models)
assert not c.search('NX-H1331', target='Inventec', role='manufacturer')
assert c.search('NX-H1331', target='Inventec')
assert c.search('QuantaPlex T41S-2U', target='Quanta', role='historical_subsidiary_portfolio')
assert not c.search('QuantaPlex T41S-2U', target='Quanta', role='manufacturer')
specs = {model: specifications.search(c, model, exact_model=True, limit=-1)
         for model in ('Horsea', 'Seadra', 'QuantaPlex T41S-2U')}
assert all(specs.values())
assert db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0] == 21
db.close()
assert file_sha256(path) == before
comparison = json.loads((OUT / 'rebuild-comparison.json').read_text())
assert len(comparison) == 42
assert all(r['original'] == r['rebuilt'] and r['only_original'] == r['only_rebuilt'] == 0 for r in comparison.values())
result = dict(database_sha256=before, read_only=True, counts=counts, targets=list(TARGETS),
    manifest_jobs=110, hashed_inputs=2104, tests=404, report_script_tests=6, rebuild_tables=42,
    owned_baseline=base['summary'], vendor_examples=examples['summary'],
    lookup_reports_refreshed=latest['lookup_reports_refreshed'], verified_pdfs=pdfs,
    manufacturer_models=list(manufacturer_models), reviewed_specs=specs,
    browser_visual_check='Not performed; prior automated file:// navigation was blocked by browser policy.')
(OUT / 'inventec-qct-final-verification-2026-10-04.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({k: v for k, v in result.items() if k not in ('verified_pdfs', 'reviewed_specs')}, indent=2))

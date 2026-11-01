import hashlib
import json
from pathlib import Path

import pdfplumber
from devicehunt import listings, lookup, validation
from devicehunt.catalog import Catalog

sid = 'celestica-ds4000-chassis-skus'
data = json.loads(Path('data/celestica-chassis-skus-reviewed.json').read_text())
path = Path('data/devices.sqlite')
initial = hashlib.sha256(path.read_bytes()).hexdigest()
c = Catalog(path, read_only=True)
matcher = listings.Matcher(c.db, vendor='Celestica')
results = []
with pdfplumber.open('data/raw/celestica-ds4000-installation-guide.pdf') as pdf:
    tables = {n: pdf.pages[n - 1].extract_tables() for n in (7, 8)}
    for item in data['devices']:
        sku = item['key']; facts = dict(item['facts']); page = int(facts['document_page'])
        cells = tables[page][item['reviewed_table_index']][item['reviewed_row_index']]
        assert cells == item['reviewed_cells']
        assert cells[1] == sku and cells[2] == '1'
        assert cells[3].startswith('ASSY,TLA,' + item['model'] + ',')
        row = c.db.execute('SELECT r.id,r.device_id,d.model FROM records r JOIN devices d ON d.id=r.device_id '
                           'WHERE r.source_id=? AND r.source_key=?', (sid, sku)).fetchone()
        assert row['model'] == item['model']
        actual = dict(c.db.execute('SELECT name,value FROM facts WHERE record_id=?', (row['id'],)))
        assert actual == facts
        assert [tuple(v) for v in c.db.execute('SELECT kind,value FROM identifiers WHERE record_id=?', (row['id'],))] == [('part_number', sku)]
        exact = matcher.match('Celestica ' + sku + ' used switch')
        assert len(exact['candidates']) == 1 and exact['candidates'][0]['model'] == item['model']
        assert exact['candidates'][0]['best_match'] == 'identifier_mention'
        proof = exact['candidates'][0]['matches']
        assert len(proof) == 1 and proof[0]['source_id'] == sid and proof[0]['identifier'] == sku
        extended = matcher.match(sku + '-UNKNOWN')
        assert all(v['best_match'] == 'variant_lead' for v in extended['candidates'])
        found = lookup.run(c.db, sku)
        catalog = found['datasets'][0]
        assert catalog['matches'] == 1 and catalog['rows'][0]['model'] == item['model']
        results.append({'sku': sku, 'model': item['model'], 'pdf_page': page,
                        'exact_listing_match': True, 'lookup_identity_matches': catalog['matches'],
                        'source_configuration': facts['ordering_configuration'],
                        'unknown_suffix_not_exact': True, 'cells_reextracted_from_pdf_match': True})
    accessories = sorted({r[1] for table in tables[8][:2] for r in table[1:]
                          if r[0] in ('Power Cable', 'Console Cable', 'Fixed Brackets')})
    for value in accessories:
        assert not c.db.execute('SELECT 1 FROM identifiers i JOIN records r ON r.id=i.record_id '
                                'WHERE r.source_id=? AND i.value=?', (sid, value)).fetchone()
assert c.db.execute('SELECT count(*) FROM records WHERE source_id=?', (sid,)).fetchone()[0] == 8
assert {r[0] for r in c.db.execute('SELECT a.role FROM affiliations a JOIN records r ON r.id=a.record_id WHERE r.source_id=?', (sid,))} == {'brand'}
base = validation.run(c.db, Path('validation/owned-hardware.json'))
c.db.close()
assert hashlib.sha256(path.read_bytes()).hexdigest() == initial
report = {'database_sha256': initial, 'read_only_audit': True, 'reviewed_rows': results,
          'accessory_codes_excluded_from_chassis_identifiers': accessories,
          'baseline': base['summary']}
Path('data/exports/celestica-skus-audit.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))

import copy
import csv
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import energystar as es, lookup
from devicehunt.catalog import Catalog, digest
from devicehunt.pipeline import import_source


class EnergyStarTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.rows = [dict(zip(es.FIELDS, values)) for values in [
            ['100', 'Oracle Corporation', 'Oracle', 'Server X10M', 'X10M', 'ES-100', '08/16/2024', '01/01/2024'],
            ['200', 'Other Company', 'Other Brand', 'Other Server', 'OTHER-1', 'ES-200', '', ''],
        ]]
        self.rows[1]['Additional Model Information'] = 'Rare configuration searchable raw only'
        for n, (code, label) in enumerate(es.CONFIGS):
            self.rows[0]['SERT Idle Measurement (Watts) ' + label] = str(210 + n)
            self.rows[0]['Power Supply Rated Output ' + label + ' (W)'] = '1400'
            self.rows[0]['Model Number or Config ID for ' + label] = 'CONFIG-' + code
            self.rows[0]['Power Supply Model Number ' + label] = 'PSU-' + code
            self.rows[0]['SERT Total Server Power Score ' + label] = '999'

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, change_csv=None, change_meta=None, change_after=None, change_bundle=None):
        headers = list(dict.fromkeys(k for row in self.rows for k in row))
        cols = [dict(name=h, fieldName='pd_id' if h == es.FIELDS[0] else h) for h in headers]
        cols[0]['cachedContents'] = dict(null='0', non_null=str(len(self.rows)), count=str(len(self.rows)))
        meta = dict(id=es.DATASET, name=es.NAME, rowsUpdatedAt=1234, columns=cols)
        if change_meta: change_meta(meta)
        after = copy.deepcopy(meta)
        if change_after: change_after(after)
        cells = [headers, *[[row.get(h, '') for h in headers] for row in self.rows]]
        if change_csv: change_csv(cells)
        with (self.root / 'rows.csv').open('w', newline='') as stream:
            csv.writer(stream).writerows(cells)
        (self.root / 'before.json').write_text(json.dumps(meta))
        (self.root / 'after.json').write_text(json.dumps(after))
        bundle = dict(schema_version=1, dataset_url=es.URL, metadata_url=es.METADATA_URL, csv_url=es.CSV_URL)
        for key, filename in [('csv', 'rows.csv'), ('metadata_before', 'before.json'), ('metadata_after', 'after.json')]:
            bundle[key] = dict(path=filename, sha256=digest((self.root / filename).read_bytes()))
        if change_bundle: change_bundle(bundle)
        p = self.root / 'bundle.json'; p.write_text(json.dumps(bundle)); return p

    def test_all_brands_retained_but_only_target_identity_and_scoped_power(self):
        for _ in range(2):
            self.assertEqual(import_source(self.c, 'energystar-servers', self.save()),
                             dict(registry_rows=2, model_assertions=1))
        self.assertEqual(len(self.c.search()), 1)
        self.assertEqual(self.c.search(target='Oracle', role='manufacturer'), [])
        self.assertEqual(len(self.c.search(target='Oracle', role='program_partner')), 1)
        self.assertEqual(self.c.search(hyperscaler=True), [])
        facts = [(r['name'], r['value']) for r in self.c.db.execute('SELECT name,value FROM facts')]
        for n, (code, label) in enumerate(es.CONFIGS):
            self.assertIn(('energystar_' + code + '_idle_watts', str(210 + n)), facts)
            self.assertIn(('energystar_' + code + '_psu_rated_output_watts', '1400'), facts)
        descriptions = [value for name, value in facts if name == 'power_description']
        self.assertEqual(len(descriptions), 6)
        self.assertTrue(all('999' not in value for value in descriptions))
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT kind,value FROM identifiers')],
                         [('energystar_id', '100')])
        result = lookup.run(self.c.db, 'Rare configuration')
        section = next(s for s in result['datasets'] if s['dataset'] == 'energystar_servers')
        self.assertEqual(section['matches'], 1)
        self.assertEqual(section['rows'][0]['brand'], 'Other Brand')
        self.rows = self.rows[1:]
        es.import_bundle(self.c, self.save())
        self.assertEqual(self.c.search(), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM energystar_servers').fetchone()[0], 1)

    def test_drift_truncation_duplicate_and_bad_dependencies_preserve_data(self):
        es.import_bundle(self.c, self.save())
        before = {t: [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + t)]
                  for t in ['sources', 'records', 'energystar_servers']}
        changes = [
            dict(change_csv=lambda rows: rows.pop()),
            dict(change_csv=lambda rows: rows[2].__setitem__(0, '100')),
            dict(change_csv=lambda rows: rows[1].pop()),
            dict(change_csv=lambda rows: rows[0].__setitem__(0, 'Unknown ID')),
            dict(change_csv=lambda rows: rows[1].__setitem__(2, '')),
            dict(change_after=lambda meta: meta.update(rowsUpdatedAt=2345)),
            dict(change_after=lambda meta: meta['columns'][1].update(fieldName='changed')),
            dict(change_meta=lambda meta: meta['columns'][0]['cachedContents'].update(null='1')),
            dict(change_bundle=lambda b: b['csv'].update(sha256='0' * 64)),
            dict(change_bundle=lambda b: b.update(dataset_url='https://example.org')),
        ]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                es.import_bundle(self.c, self.save(**change))
            for table, rows in before.items():
                self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM ' + table)], rows)

    def test_partner_only_unknown_company_and_pattern_models_stay_raw(self):
        for brand, model in [('Other Brand', 'X10M'), ('Oracle 외', 'X11M'),
                             ('Oracle', 'Xxx-2'), ('Oracle', 'X10M/X11M')]:
            with self.subTest(brand=brand, model=model):
                self.rows[0]['Brand Name'] = brand
                self.rows[0]['Model Number'] = model
                self.assertEqual(es.import_bundle(self.c, self.save())['model_assertions'], 0)
                self.assertEqual(self.c.db.execute('SELECT count(*) FROM energystar_servers').fetchone()[0], 2)


if __name__ == '__main__':
    unittest.main()

import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import energystar_archive as a, lookup
from devicehunt.catalog import Catalog, digest
from devicehunt.pipeline import import_source


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def save(self, version='v1_1', change=None, change_bundle=None):
        spec = a.FORMATS[version]; sheets = []
        for name, (start, levels, skip, width) in spec['sheets'].items():
            rows = [[''] * width for _ in range(start - 1)]
            headers = ['field-' + str(n) for n in range(width)]
            headers[:skip] = [''] * skip
            for n, h in enumerate(['ENERGY STAR Partner', 'Brand', 'Model Name', 'Model Number'], skip): headers[n] = h
            if version == 'v2':
                headers[:6] = ['PD_ID', 'ENERGY STAR Partner', 'Brand Name', 'Model Name', 'Model Number', 'Date Qualified']
            rows.append(headers); merged = []
            if levels == 2:
                sub = [''] * width
                for col, field in [(5, 'Model Number / Configuration ID'), (40, 'Idle Power Draw @ 230V (W)'),
                                   (48, 'Full Load Testing Voltage / Frequency'),
                                   (50, 'Full Power Load (W)')]:
                    headers[col] = field; headers[col+1] = ''
                    sub[col] = 'Minimum Config.'; sub[col+1] = 'Maximum Config.'
                    merged.append([start-1, start, col, col+2])
                rows.append(sub)
            values = [''] * width
            if version == 'v2':
                values[:6] = [123, 'Oracle Corporation', 'Oracle', 'Oracle Server X6-2', 'Oracle Server X6-2', '2016-01-01']
            else:
                values[1:5] = ['Oracle Corporation', 'Sun Fire', 'Sun Fire X4170 M2', 'X4170 M2 Energy Star Family 1']
                if levels == 1:
                    values[1:5] = ['Other Company', 'Other Brand', 'Other Server', 'OTHER-1']
                else:
                    values[5:7] = ['Minimum config ID', 'Maximum config ID']
                    values[40:42] = ['161.0', '229.0']; values[50:52] = ['345.0', '511.0']
                    values[48:50] = ['220V / 60Hz', '220V / 60Hz']
                    values[52] = {'excel_serial': 40000.0, 'date_mode': 0, 'iso_datetime': '2009-07-06T00:00:00'}
            rows += [values, [''] * width]
            sheets.append(dict(name=name, rows=rows, merged_ranges=merged))
        raw = b'fixture workbook'; (self.root / 'workbook').write_bytes(raw)
        data = dict(schema_version=1, workbook_sha256=digest(raw), extraction_method=spec['method'], sheets=sheets)
        b = dict(schema_version=1, version=version, url=spec['url'], index_url=a.INDEX,
                 expected_rows={name: 1 for name in spec['sheets']}, header_sha256={})
        for s in sheets:
            b['header_sha256'][s['name']] = digest(json.dumps(a.fields(s, spec['sheets'][s['name']]),ensure_ascii=False).encode())
        if change: change(data)
        (self.root / 'cells.json').write_text(json.dumps(data))
        (self.root / 'index.html').write_text('<a href="' + spec['url'] + '">Historical List</a>')
        for key, filename in [('workbook', 'workbook'), ('cells', 'cells.json'), ('index', 'index.html')]:
            b[key] = dict(path=filename, sha256=digest((self.root / filename).read_bytes()))
        if change_bundle: change_bundle(b)
        p = self.root / 'bundle.json'; p.write_text(json.dumps(b)); return p

    def test_family_headers_power_roles_and_all_brand_retention(self):
        p = self.save()
        for _ in range(2): self.assertEqual(import_source(self.c, 'energystar-server-archive', p),
                                           dict(registry_rows=2, model_assertions=1))
        row = self.c.db.execute('SELECT brand,model FROM devices').fetchone()
        self.assertEqual(tuple(row), ('Sun Fire', 'Sun Fire X4170 M2'))
        self.assertEqual(len(self.c.search(target='Oracle', role='program_partner')), 1)
        self.assertEqual(self.c.search(target='Oracle', role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0], 0)
        descriptions = [r[0] for r in self.c.db.execute("SELECT value FROM facts WHERE name='power_description'")]
        self.assertEqual(len(descriptions), 4)
        self.assertTrue(any('Minimum Config.: 161.0 W' in v and '@ 230V' in v and 'Family 1' in v for v in descriptions))
        self.assertTrue(any('Maximum Config.: 229.0 W' in v for v in descriptions))
        self.assertTrue(any('Full Power Load (W) | Maximum Config.: 511.0 W' in v and '220V / 60Hz' in v for v in descriptions))
        raw = self.c.db.execute("SELECT cells_json FROM energystar_server_archives WHERE sheet LIKE 'Family%'").fetchone()[0]
        self.assertEqual(json.loads(raw)[52]['excel_serial'], 40000.0)
        section = next(s for s in lookup.run(self.c.db, 'OTHER-1')['datasets'] if s['dataset']=='energystar_server_archives')
        self.assertEqual(section['matches'], 1)

    def test_v2_refresh_keeps_other_sources_and_withdraws_target(self):
        a.import_bundle(self.c, self.save())
        self.assertEqual(a.import_bundle(self.c, self.save('v2')), dict(registry_rows=1, model_assertions=1))
        self.assertEqual(len(self.c.search()), 2)
        def change(data):
            row = data['sheets'][0]['rows'][4]
            row[1:5] = ['Other Company','Other Brand','Other Server','OTHER-2']
        a.import_bundle(self.c, self.save('v2', change=change))
        self.assertEqual(len(self.c.search()), 1)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM energystar_server_archives').fetchone()[0], 3)

    def test_invalid_extraction_rejected_before_source_replacement(self):
        a.import_bundle(self.c, self.save())
        before = {t:[tuple(r) for r in self.c.db.execute('SELECT * FROM '+t)]
                  for t in ['records','sources','energystar_server_archives']}
        changes = [
            lambda d: d.update(workbook_sha256='0'*64),
            lambda d: d.update(extraction_method='Unknown'),
            lambda d: d['sheets'].pop(),
            lambda d: d['sheets'].append(copy.deepcopy(d['sheets'][0])),
            lambda d: d['sheets'][0]['rows'][13].__setitem__(1, 'Changed Partner Header'),
            lambda d: d['sheets'][0]['rows'][14].__setitem__(1, ''),
            lambda d: d['sheets'][0]['rows'].pop(14),
            lambda d: d['sheets'][0]['rows'][14].pop(),
            lambda d: d['sheets'][1]['merged_ranges'][0].__setitem__(3,999),
        ]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                a.import_bundle(self.c, self.save(change=change))
            for t, rows in before.items():
                self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM '+t)], rows)
        with self.assertRaises(ValueError):
            a.import_bundle(self.c,self.save(change_bundle=lambda b:b['cells'].update(sha256='0'*64)))

    def test_v2_duplicate_ids_rejected_and_placeholder_stays_raw(self):
        def duplicate(d): d['sheets'][0]['rows'].append(copy.deepcopy(d['sheets'][0]['rows'][4]))
        with self.assertRaises(ValueError):
            a.import_bundle(self.c,self.save('v2',change=duplicate,
                change_bundle=lambda b:b.update(expected_rows={'Enterprise Servers QPL V2':2})))
        def pattern(d): d['sheets'][0]['rows'][4][4]='Xxx-2'
        self.assertEqual(a.import_bundle(self.c,self.save('v2',change=pattern))['model_assertions'],0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM energystar_server_archives').fetchone()[0],1)


if __name__ == '__main__': unittest.main()

import json
import sqlite3
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

from devicehunt import bsmi_additional as b, lookup
from devicehunt.catalog import Catalog, digest


class AdditionalBSMITests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.path = self.root / 'db.sqlite'; self.c = Catalog(self.path)

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def bundle(self, dataset, count=2):
        spec = b.DATASETS[dataset]; page, url = b.urls(dataset)
        row = {key: '' for key, _ in spec['fields']}
        row.update(primary_model='  MODEL-001 (suffix intact) ', product_name='Oracle named coffee machine')
        row[spec['fields'][0][0]] = '000123'
        row['company_name' if dataset == '169189' else 'applicant'] = 'Unrelated Company'
        root = ET.Element('bsmi')
        for _ in range(count):
            element = ET.SubElement(root, 'row')
            for key, label in spec['fields']:
                ET.SubElement(element, label).text = row[key]
        meta = dict(dataset_url=page, xml_url=url, title=spec['title'], license_url=b.LICENSE_URL,
                    fields=[label for _, label in spec['fields']], capture_format='factual web-tool metadata extract')
        data = dict(schema_version=1, dataset_id=dataset, dataset_url=page, download_url=url,
                    license_url=b.LICENSE_URL, expected=b.summary([row]*count, dataset))
        for name, content in [('xml', ET.tostring(root, encoding='utf-8')),
                              ('metadata', json.dumps(meta).encode())]:
            p = self.root / (dataset + '.' + name); p.write_bytes(content)
            data[name] = dict(path=p.name, sha256=digest(content))
        path = self.root / (dataset + '.json'); path.write_text(json.dumps(data)); return path, row

    def test_all_shapes_preserve_rows_and_source_scoped_refresh(self):
        for dataset, spec in b.DATASETS.items():
            path, row = self.bundle(dataset); result = b.import_bundle(self.c, path)
            self.assertEqual(result['duplicate_number_rows'], 1)
            actual = list(self.c.db.execute('SELECT * FROM ' + spec['table']))
            self.assertEqual(len(actual), 2)
            for actual_row in actual:
                self.assertEqual({k: actual_row[k] for k in row}, row)
            b.import_bundle(self.c, path)
        b.import_bundle(self.c, self.bundle('30176', 1)[0])
        for dataset, spec in b.DATASETS.items():
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ' + spec['table']).fetchone()[0],
                             1 if dataset == '30176' else 2)
        for table in ['devices', 'records', 'affiliations']:
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ' + table).fetchone()[0], 0)

    def test_schema_counts_metadata_and_hash_fail_before_replacement(self):
        for dataset, spec in b.DATASETS.items():
            b.import_bundle(self.c, self.bundle(dataset)[0])
            for change in ['xml', 'hash', 'metadata', 'counts', 'dataset', 'url']:
                path, _ = self.bundle(dataset); data = json.loads(path.read_text())
                if change == 'xml':
                    p = self.root / data['xml']['path']; p.write_bytes(b'<bsmi><row><unexpected/></row></bsmi>')
                    data['xml']['sha256'] = digest(p.read_bytes())
                elif change == 'hash':
                    (self.root / data['xml']['path']).write_bytes(b'changed')
                elif change == 'metadata':
                    p = self.root / data['metadata']['path']; meta = json.loads(p.read_text())
                    meta['title'] = 'Different register'; p.write_text(json.dumps(meta))
                    data['metadata']['sha256'] = digest(p.read_bytes())
                elif change == 'counts': data['expected']['rows'] = 999
                elif change == 'dataset': data['dataset_id'] = 'not-a-register'
                else: data['download_url'] = 'https://example.org/other.xml'
                path.write_text(json.dumps(data))
                with self.assertRaises(ValueError): b.import_bundle(self.c, path)
                self.assertEqual(self.c.db.execute('SELECT count(*) FROM ' + spec['table']).fetchone()[0], 2)

    def test_readonly_lookup_old_schema_and_no_vendor_inference(self):
        for dataset in b.DATASETS: b.import_bundle(self.c, self.bundle(dataset)[0])
        before = self.path.read_bytes()
        with sqlite3.connect(self.path.as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            report = lookup.run(db, 'Oracle', 1)
            sections = {s['dataset']: s for s in report['datasets']}
            self.assertEqual(sections['catalog']['matches'], 0)
            for spec in b.DATASETS.values():
                s = sections[spec['table']]
                self.assertEqual((s['matches'], s['shown'], s['truncated']), (2, 1, True))
                self.assertTrue(s['rows'][0]['source_url'].startswith('https://data.gov.tw/dataset/'))
        self.assertEqual(before, self.path.read_bytes())
        for spec in b.DATASETS.values(): self.c.db.execute('DROP TABLE ' + spec['table'])
        self.c.db.commit()
        sections = {s['dataset']: s for s in lookup.run(self.c.db, 'Oracle')['datasets']}
        for spec in b.DATASETS.values(): self.assertEqual(sections[spec['table']]['state'], 'not_loaded')

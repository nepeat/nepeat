import json
import sqlite3
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

from devicehunt import bsmi, lookup
from devicehunt.catalog import Catalog, digest


class BSMITests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.rows = [dict(vendor_code='00123', certificate_number='CI001', applicant='甲骨文公司',
                          expiry_raw='2020/01/02', product_name='伺服器',
                          primary_model=' X1 (本型式係Ampere CPU) ', status_raw='期限到期失效'),
                     dict(vendor_code='00999', certificate_number='CI001', applicant='Other Company',
                          expiry_raw='', product_name='The Oracle coffee machine',
                          primary_model='COFFEE-1', status_raw='認可')]

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def body(self):
        root = ET.Element('bsmi')
        for row in self.rows:
            element = ET.SubElement(root, 'row')
            for key, label in bsmi.FIELDS:
                ET.SubElement(element, label).text = row[key]
        return ET.tostring(root, encoding='utf-8', xml_declaration=True)

    def save(self, body=None):
        data = dict(schema_version=1, dataset_url=bsmi.PAGE_URL, download_url=bsmi.XML_URL,
                    license_url=bsmi.LICENSE_URL, expected=bsmi.summary(self.rows))
        meta = dict(dataset_url=bsmi.PAGE_URL, xml_url=bsmi.XML_URL,
                    capture_format='factual web-tool metadata extract')
        for key, content in [('metadata', json.dumps(meta).encode()),
                             ('xml', self.body() if body is None else body)]:
            path = self.root / key; path.write_bytes(content)
            data[key] = dict(path=key, sha256=digest(content))
        path = self.root / 'bundle.json'; path.write_text(json.dumps(data)); return path

    def test_raw_annotations_blanks_duplicates_and_scoped_refresh(self):
        path = self.save(); counts = bsmi.import_bundle(self.c, path)
        self.assertEqual(counts['duplicate_certificate_rows'], 1)
        rows = list(self.c.db.execute('SELECT * FROM bsmi_certificates ORDER BY row_number'))
        self.assertEqual([{k:r[k] for k, _ in bsmi.FIELDS} for r in rows], self.rows)
        for table in ['records', 'devices', 'affiliations']:
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ' + table).fetchone()[0], 0)
        bsmi.import_bundle(self.c, path)
        self.c.source('other', 'https://example.org', 'regulatory', 'test', 'test')
        self.c.db.execute("INSERT INTO bsmi_certificates SELECT 'other',row_number," +
                          ','.join(k for k, _ in bsmi.FIELDS) + ' FROM bsmi_certificates WHERE row_number=1')
        self.c.db.commit()
        self.rows = self.rows[:1]; bsmi.import_bundle(self.c, self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM bsmi_certificates').fetchone()[0], 2)

    def test_malformed_xml_schema_changes_entities_and_hash_fail_before_replacement(self):
        bsmi.import_bundle(self.c, self.save())
        body = self.body()
        bad = [b'<bsmi/>', b'<bsmi>', b'<other/>',
               body.replace(b'<row>', b'<row extra="new">', 1),
               body.replace(b'<row>', b'<row><extra/>', 1),
               body.replace(b'<bsmi>', b'<bsmi>unparsed text', 1),
               body.replace('伺服器'.encode(), b'<nested/>'),
               body.replace('伺服器'.encode(), b'&unknown;'),
               b'<!DOCTYPE bsmi [<!ENTITY x "text">]><bsmi/>',
               body.replace('申請人'.encode(), 'different'.encode())]
        for content in bad:
            with self.assertRaises(ValueError): bsmi.import_bundle(self.c, self.save(content))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM bsmi_certificates').fetchone()[0], 2)
        path = self.save(); (self.root / 'xml').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'checksum'): bsmi.import_bundle(self.c, path)
        path = self.save(); data = json.loads(path.read_text()); data['expected']['rows'] = 3
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'counts'): bsmi.import_bundle(self.c, path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM bsmi_certificates').fetchone()[0], 2)

    def test_search_product_text_does_not_infer_vendor_and_old_schema_is_read_only(self):
        bsmi.import_bundle(self.c, self.save()); before = (self.root / 'db.sqlite').read_bytes()
        with sqlite3.connect((self.root / 'db.sqlite').as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            result = lookup.run(db, 'ORACLE')
            section = next(s for s in result['datasets'] if s['dataset'] == 'bsmi')
            self.assertEqual(section['matches'], 1)
            self.assertEqual(section['rows'][0]['applicant'], 'Other Company')
            self.assertEqual(section['rows'][0]['source_url'], bsmi.PAGE_URL)
            self.assertEqual(result['datasets'][0]['matches'], 0)
        self.assertEqual(before, (self.root / 'db.sqlite').read_bytes())
        self.c.db.execute('DROP TABLE bsmi_certificates'); self.c.db.commit()
        section = next(s for s in lookup.run(self.c.db, 'Oracle')['datasets'] if s['dataset'] == 'bsmi')
        self.assertEqual(section['state'], 'not_loaded')

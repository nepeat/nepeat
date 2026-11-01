import base64
import csv
import html
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import nigeria, lookup, vendor_inventory
from devicehunt.catalog import Catalog, digest


class NigeriaTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.root = Path(self.tmp.name)
        self.c = Catalog(self.root/'catalog.sqlite')
        self.rows = [
            ['1', 'Agent', 'Amazon Data Services, Inc.', 'Server', 'ATLANTA23SF', 'Amazon Data Services, Inc.'],
            ['2', 'Oracle', 'Oracle', 'Router', 'OTHER-1', 'Other Corp'],
            ['3', 'Agent', 'Oracle', 'Grouped', 'ONE / TWO', 'Oracle'],
            ['4', 'Other', 'Other', 'Radio', 'ML1-60-35', 'Accton Tech. Corporation'],
            ['5', '다른 회사 (Oracle)', 'Holder', 'Server', 'X8-2L', 'Oracle'],
            ['6', 'Other', 'Other', 'Damaged \udcae name', 'CODE-6', 'Other Corp']]
        self.url = 'https://www.ncc.gov.ng/sites/default/files/2026-09/equipment.csv'

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def entry(self, name, body):
        (self.root/name).write_bytes(body)
        return dict(path=name, sha256=digest(body))

    def save(self, change=None):
        stream = io.StringIO(newline=''); csv.writer(stream).writerows([nigeria.HEADER]+self.rows)
        body = stream.getvalue().encode('utf-8', errors='surrogateescape')
        rows,_ = nigeria.csv_table(body)
        def page(rows):
            return ('<title>Approved Equipment | NCC</title><a href="/sites/default/files/2026-09/equipment.csv">CSV</a>'
                    '<table>'+''.join('<tr>'+''.join('<td>'+html.escape(c)+'</td>' for c in r)+'</tr>' for r in rows)+'</table>').encode()
        before = page(rows); after = page(rows)
        if change == 'html': after = page(rows[:-1])
        if change == 'csv': body = body.replace(b'ATLANTA23SF', b'OTHER23SF')
        data=dict(schema_version=1,source_url=nigeria.URL,
            csv_decoding='UTF-8 with flagged replacement and original invalid cell bytes',
            expected_rows=len(self.rows),page_before=self.entry('before.html',before),
            page_after=self.entry('after.html',after),csv=dict(**self.entry('equipment.csv',body),url=self.url))
        if change == 'url': data['csv']['url'] = 'https://example.org/equipment.csv'
        if change == 'count': data['expected_rows'] += 1
        if change == 'hash': data['csv']['sha256'] = '0'*64
        p=self.root/'bundle.json';p.write_text(json.dumps(data));return p

    def test_raw_register_role_isolation_and_damaged_cell_retention(self):
        result=nigeria.import_bundle(self.c,self.save())
        self.assertEqual(result,dict(registry_rows=6,model_assertions=2,rows_with_decoding_errors=1))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM nigeria_equipment').fetchone()[0],6)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0],0)
        # Grouped labels, holder-only rows and unreviewed abbreviations stay raw.
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},{'ATLANTA23SF','X8-2L'})
        self.assertIsNone(self.c.db.execute("SELECT target FROM affiliations WHERE company='다른 회사 (Oracle)'").fetchone()[0])
        raw=self.c.db.execute('SELECT * FROM nigeria_equipment WHERE row_number=6').fetchone()
        self.assertIn('\ufffd',raw['equipment_name'])
        self.assertEqual(base64.b64decode(json.loads(raw['invalid_cell_bytes_json'])['3']),b'Damaged \xae name')
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0],0)
        report=lookup.run(self.c.db,'ML1-60-35')
        sections={s['dataset']:s for s in report['datasets']}
        self.assertEqual(sections['nigeria']['matches'],1)
        self.assertEqual(sections['catalog']['matches'],0)
        self.assertEqual(vendor_inventory.run(self.c.db,scope='manufacturer',limit=-1)['unique_identities'],2)

    def test_invalid_capture_preserves_catalog_and_raw_rows(self):
        nigeria.import_bundle(self.c,self.save())
        before={t:[tuple(r) for r in self.c.db.execute('SELECT * FROM '+t)] for t in ('records','sources','nigeria_equipment')}
        for change in ('html','csv','url','count','hash'):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):nigeria.import_bundle(self.c,self.save(change))
                for t,rows in before.items():self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM '+t)],rows)

    def test_withdrawal_duplicate_models_and_source_isolation(self):
        self.rows.append(['7','Agent','Oracle','Server','X8-2L','Oracle'])
        self.rows.append(['8','Agent','Oracle','Damaged \udcae','BAD-8','Oracle'])
        nigeria.import_bundle(self.c,self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],2)
        self.c.source('other','https://example.org','vendor','test','test')
        self.c.add('other','1','https://example.org','Celestica','KEPT','Kept','test',{})
        self.c.finish('other',1)
        self.rows=[['1','Agent','Holder','Server','ONLY-RAW','Other Corp']]
        nigeria.import_bundle(self.c,self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM nigeria_equipment').fetchone()[0],1)
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT model FROM devices')],['KEPT'])

    def test_malformed_table_and_numbering_rejected(self):
        p=self.save();body=(self.root/'before.html').read_bytes()
        for broken in (body.replace(b'MANUFACTURER',b'BRAND'),body.replace(b'<td>2</td>',b'<td>1</td>'),body+body):
            with self.assertRaises(ValueError):nigeria.html_table(broken)
        with self.assertRaises(ValueError):nigeria.csv_table(b'SN,APPLICANT\n1,Agent\n')

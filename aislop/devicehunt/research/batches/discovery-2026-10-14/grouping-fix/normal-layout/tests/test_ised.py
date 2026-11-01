import csv
import io
import json
import tempfile
import unittest
from html import escape
from pathlib import Path

from devicehunt import ised
from devicehunt.catalog import Catalog, digest


class IsedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, total=2, mismatch=False, company='Amazon Data Services, Inc.', width=44, complete=True):
        rows = []
        for hvin in ['TE1A001', 'TE1A195']:
            row = [''] * width
            row[:9] = ['25869', company, hvin, 'Marketing family', 'Different host',
                       '1.3.1', 'unreviewed CSV field', '25869-TE1A001', '2023-07-10T06:31:48-04:00']
            rows.append(row)
        table = '<html><span id="foundItems">%d</span><table id="equipmentSearchResultTable">' % total
        table += '<th colspan=7><div>25869 - %s</div></th>' % escape(company)
        table += '<tr>' + ''.join('<th>%s</th>' % h for h in ised.HEADERS) + '</tr>'
        for r in rows:
            cells = [r[2], r[3], r[4], r[5], r[7], '2023-07-10']
            table += '<tr>' + ''.join('<td>%s</td>' % escape(v) for v in cells) + '</tr>'
        table += '</table></html>'
        if mismatch:
            rows[1][2] = 'TE1A105'
        out = io.StringIO(); csv.writer(out).writerows(rows)
        capture = {'schema_version': 1, 'company_query': 'Amazon Data Services',
                   'complete': complete, 'responses': []}
        for name, body in [('result.html', table), ('results.csv', out.getvalue())]:
            p = self.root / name; p.write_text(body)
            capture['responses'].append({'path': name, 'sha256': digest(p.read_bytes()),
                                         'url': ised.URL, 'final_url': ised.URL, 'method': 'GET'})
        p = self.root / 'capture.json'; p.write_text(json.dumps(capture))
        return p

    def test_shared_certificate_keeps_hardware_versions_separate_and_is_idempotent(self):
        p = self.save()
        for _ in range(2):
            self.assertEqual(ised.import_capture(self.c, p)['imported'], 2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
        self.assertEqual(len(self.c.search('TE1A195', target='Amazon Data Services')), 1)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE value='Different host'").fetchone()[0], 2)
        raw = self.c.db.execute('SELECT * FROM ised_equipment ORDER BY row_number').fetchall()
        self.assertEqual(len(raw), 2)
        self.assertTrue(all(r['matches_html_identity'] == 1 for r in raw))
        self.assertEqual(raw[0]['company_number'], '25869')
        self.assertEqual(raw[0]['approval_timestamp_raw'], '2023-07-10T06:31:48-04:00')
        self.assertEqual(json.loads(raw[0]['cells_json'])[6], 'unreviewed CSV field')

    def test_truncation_changed_id_company_and_csv_schema_reject_before_replacement(self):
        ised.import_capture(self.c, self.save())
        for options in [{'total': 3}, {'mismatch': True}, {'company': 'Unrelated Company'},
                        {'width': 43}, {'complete': False}]:
            with self.subTest(options=options), self.assertRaises(ValueError):
                ised.import_capture(self.c, self.save(**options))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ised_equipment').fetchone()[0], 2)

    def test_processing_page_is_never_empty_success(self):
        with self.assertRaises(ValueError):
            ised.html_rows('<html><h1>Processing…please wait.</h1></html>')

    def test_dependency_integrity_and_traversal_rejected(self):
        p = self.save(); (self.root / 'results.csv').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            ised.validate_capture(p)
        p = self.save(); data = json.loads(p.read_text())
        data['responses'][0]['path'] = '../result.html'; p.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'dependency'):
            ised.validate_capture(p)

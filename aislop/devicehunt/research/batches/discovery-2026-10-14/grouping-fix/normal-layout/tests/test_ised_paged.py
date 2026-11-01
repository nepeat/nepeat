import csv
import io
import json
import tempfile
import unittest
from html import escape
from pathlib import Path

from devicehunt import ised, lookup
from devicehunt.catalog import Catalog, digest


def page(total, start, groups):
    count = sum(len(rows) for _, rows in groups)
    html = '<html><h1>Radio Equipment List (REL) - Search Results</h1><span id="foundItems">%d</span>' % total
    if not total:
        return html + '</html>'
    html += 'Displayed Items: from %d to %d<table id="equipmentSearchResultTable">' % (start, start + count - 1)
    for company, rows in groups:
        html += '<th colspan=7>%s</th><tr>' % escape(company)
        html += ''.join('<th>%s</th>' % h for h in ised.HEADERS) + '</tr>'
        for row in rows:
            html += '<tr>' + ''.join('<td>%s</td>' % escape(v) for v in row) + '</tr>'
    return html + '</table></html>'


class PagedIsedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.first = ['SunSPOT', '', '', '', '1894B-3011', '2006-10-13']
        self.second = ['7 Inch CFD', 'Different family', '', '', '9870A-7CFD', '2022-11-06']
        self.group = '1894B - Oracle America, Inc.'

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def fixture(self, alter=None, zero=False):
        bodies = {'first.html': page(2, 1, [(self.group, [self.first])]),
                  'second.html': page(2, 2, [(self.group, [self.second])]),
                  'export.html': page(2, 2, [(self.group, [self.second])])}
        rows = []
        for number, cells in [('1894B', self.first), ('9870A', self.second), ('9870A', self.second)]:
            row = [''] * 44
            row[:9] = [number, 'Oracle America, Inc.', *cells[:4], 'unlabelled', cells[4], cells[5] + 'T00:00:00-04:00']
            rows.append(row)
        out = io.StringIO(); csv.writer(out).writerows(rows); bodies['results.csv'] = out.getvalue()
        selected = ['first.html', 'second.html']
        if zero:
            bodies = {'zero.html': page(0, 0, [])}; selected = ['zero.html']
        if alter:
            alter(bodies, selected)
        data = dict(schema_version=2, company_query='Oracle', complete=True, result_pages=selected, responses=[])
        for name, body in bodies.items():
            p = self.root / name; p.write_text(body)
            data['responses'].append(dict(path=name, sha256=digest(p.read_bytes()),
                                          url=ised.URL, final_url=ised.URL, method='GET'))
        p = self.root / 'capture.json'; p.write_text(json.dumps(data)); return p

    def test_radio_rows_expand_without_inventing_devices_or_company_numbers(self):
        p = self.fixture()
        for _ in range(2):
            result = ised.import_capture(self.c, p)
            self.assertEqual(result, dict(query_records=2, csv_rows=3, html_without_exact_csv=0,
                                          unmatched_csv_rows=0, imported=2))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
        facts = self.c.db.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id JOIN devices d ON d.id=r.device_id WHERE d.model='7 Inch CFD' AND f.name='ised_company_number'").fetchall()
        self.assertEqual([r[0] for r in facts], ['9870A'])
        raw = json.loads(self.c.db.execute("SELECT raw_json FROM records WHERE source_key='2'").fetchone()[0])
        self.assertEqual(len(raw['csv_matches']), 2)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        raw = self.c.db.execute('SELECT row_number,company_number,matches_html_identity FROM ised_equipment ORDER BY row_number').fetchall()
        self.assertEqual([tuple(r) for r in raw], [(1,'1894B',1),(2,'9870A',1),(3,'9870A',1)])

    def test_html_identity_is_preserved_when_csv_differs(self):
        p = self.fixture(lambda bodies, selected: bodies.update({'results.csv': bodies['results.csv'].replace('7 Inch CFD', 'OTHER')}))
        stats = ised.import_capture(self.c, p)
        self.assertEqual(stats['html_without_exact_csv'], 1)
        self.assertEqual(stats['unmatched_csv_rows'], 2)
        self.assertEqual(len(self.c.search('7 Inch CFD')), 1)
        self.assertEqual(self.c.search('OTHER'), [])
        raw = self.c.db.execute("SELECT hvin,matches_html_identity FROM ised_equipment ORDER BY row_number").fetchall()
        self.assertEqual([tuple(r) for r in raw], [('SunSPOT',1),('OTHER',0),('OTHER',0)])
        section = next(s for s in lookup.run(self.c.db, 'OTHER')['datasets'] if s['dataset']=='ised')
        self.assertEqual(section['matches'],2)
        self.assertEqual(len(section['rows'][0]['raw_cells']),44)

    def test_missing_overlapping_changed_or_out_of_order_pages_fail_before_replacement(self):
        ised.import_capture(self.c, self.fixture())
        def missing(b, s): s.pop()
        def order(b, s): s.reverse()
        def overlap(b, s): b['second.html'] = page(2, 1, [(self.group, [self.second])])
        def total(b, s): b['second.html'] = page(3, 2, [(self.group, [self.second])])
        def changed(b, s): b['export.html'] = b['export.html'].replace('7 Inch CFD', 'CHANGED')
        for alteration in [missing, order, overlap, total, changed]:
            with self.subTest(alteration=alteration.__name__), self.assertRaises(ValueError):
                ised.import_capture(self.c, self.fixture(alteration))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ised_equipment').fetchone()[0], 3)

    def test_explicit_zero_is_recorded_but_processing_or_error_is_rejected(self):
        ised.import_capture(self.c, self.fixture())
        self.assertEqual(ised.import_capture(self.c, self.fixture(zero=True))['query_records'], 0)
        self.assertEqual(self.c.db.execute('SELECT records_seen FROM sources').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ised_equipment').fetchone()[0],0)
        section = next(s for s in lookup.run(self.c.db, 'not found')['datasets'] if s['dataset']=='ised')
        self.assertEqual(section['state'],'loaded')
        self.assertEqual(section['stored_rows'],0)
        for body in ['<html>Processing…please wait.</html>', '<html>No results found.</html>']:
            with self.assertRaises(ValueError): ised.labelled_page(body)

    def test_multiple_company_groups_and_missing_certificate_remain_scoped(self):
        cells = ['MODEL-A', '', '', '', '', '2002-05-16']
        result = ised.labelled_page(page(2, 1, [('3857A - Accton Technology Corporation', [cells]),
                                               ('3857D - Accton Wireless Broadband Corp.', [cells])]))
        self.assertEqual([r['company'] for r in result['rows']],
                         ['Accton Technology Corporation', 'Accton Wireless Broadband Corp.'])
        self.assertEqual(result['rows'][0]['cells'][4], '')

    def test_non_target_csv_company_and_unlabelled_cells_remain_raw_only(self):
        def other(bodies, selected):
            rows=list(csv.reader(io.StringIO(bodies['results.csv'])))
            rows[1][1]='Unrelated Company';rows[1][6]='Rare unlabelled value'
            out=io.StringIO();csv.writer(out).writerows(rows);bodies['results.csv']=out.getvalue()
        result=ised.import_capture(self.c,self.fixture(other))
        self.assertEqual(result['unmatched_csv_rows'],1)
        section=next(s for s in lookup.run(self.c.db,'Rare unlabelled value')['datasets'] if s['dataset']=='ised')
        self.assertEqual(section['matches'],1)
        self.assertEqual(section['rows'][0]['company_name'],'Unrelated Company')
        self.assertEqual(section['rows'][0]['matches_html_identity'],0)
        self.assertEqual(self.c.search('Rare unlabelled value'),[])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM affiliations WHERE company='Unrelated Company'").fetchone()[0],0)

    def test_refresh_only_replaces_its_own_raw_source(self):
        ised.import_capture(self.c,self.fixture())
        self.c.source('other-query','https://example.org','regulatory','fixture','fixture')
        row=list(self.c.db.execute('SELECT * FROM ised_equipment LIMIT 1').fetchone())
        row[0]='other-query'
        self.c.db.execute('INSERT INTO ised_equipment VALUES ('+','.join('?' for _ in row)+')',row)
        self.c.finish('other-query',1)
        ised.import_capture(self.c,self.fixture(zero=True))
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT source_id FROM ised_equipment')],['other-query'])

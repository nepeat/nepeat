import json
import tempfile
import unittest
from html import escape
from pathlib import Path

from devicehunt import tec, marketplace, validation
from devicehunt.catalog import Catalog


class AdditionalSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.path = self.root / 'source.html'

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def table(self, rows):
        self.path.write_text('<table>' + ''.join('<tr>' + ''.join('<td>' + escape(x) + '</td>'
            for x in row) + '</tr>' for row in [tec.HEADER] + rows) + '</table>')
        return tec.import_table(self.c, self.path)

    def row(self, n='1', applicant='Omega QMS Private Limited', oem='Accton Technology Corporation'):
        return [n, applicant, 'LAN Switch', 'L2 LAN Switch', oem,
                'AS7712-32X', 'AS7760-32X,AS7712-32X', 'Mandatory', 'Regular',
                '20240719 19/07/2024', '20340718 18/07/2034', '379402315']

    def test_tec_oem_and_applicant_separate_and_models_not_aliases(self):
        self.table([self.row()])
        self.table([self.row()])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 2)
        self.assertEqual(len(self.c.search(target='Accton', role='declared_oem')), 2)
        self.assertEqual(self.c.search(target='Accton', role='regulatory_applicant'), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT value FROM facts WHERE name='certificate_issue_date' LIMIT 1").fetchone()[0], '2024-07-19')

    def test_tec_candidate_filter_and_malformed_refresh(self):
        self.table([self.row(), self.row('2', 'Accton-like reseller', 'Unrelated company')])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
        bad_date = self.row(); bad_date[9] = '20240718 19/07/2024'
        for rows in [[bad_date], [self.row('2')], [self.row(), self.row()], [self.row()[:-1]]]:
            with self.assertRaises(ValueError):
                self.table(rows)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)

    def test_marketplace_is_searchable_but_not_corroborated_identity(self):
        self.path.write_text('<h1><span>EDGE-CORE 7760-32X-A-AC-F Switch</span></h1>'
                             '<div itemprop="sku">I610661</div>')
        marketplace.import_ecrunch(self.c, self.path)
        fixture = self.root / 'inventory.json'
        fixture.write_text(json.dumps({'schema_version': 1, 'devices': [
            {'identifier': '7760-32X-A-AC-F', 'vendor': 'Edge-Core'}]}))
        result = validation.run(self.c.db, fixture)['results'][0]
        self.assertEqual(result['status'], 'gap')
        self.assertEqual(len(result['search_hits']), 1)
        self.assertIn('seller-reported', result['leads'][0]['reason'])
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.path.write_text('<h1>Recommended EDGE-CORE 7760-32X-A-AC-F</h1>')
        with self.assertRaises(ValueError):
            marketplace.import_ecrunch(self.c, self.path)
        self.assertEqual(len(self.c.search('7760-32X-A-AC-F')), 1)


if __name__ == '__main__':
    unittest.main()

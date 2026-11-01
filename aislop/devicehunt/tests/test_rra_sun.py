import csv
import io
import json
import unittest

from devicehunt import rra, rra_sun, vendor_inventory
from devicehunt.catalog import digest, target_for
from tests import test_rra


class RRAReorderingTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_rra.RRATests(); self.fixture.setUp()

    def tearDown(self):
        self.fixture.tearDown()

    def save(self, mode=True, change=None):
        path = self.fixture.save(); data = json.loads(path.read_text())
        rows = list(reversed(rra.csv_rows(self.fixture.csv)))
        for n, row in enumerate(rows, 1): row[0] = str(n)
        if change: change(rows)
        out = io.StringIO(); writer = csv.writer(out); writer.writerow(rra.HEADER); writer.writerows(rows)
        p = path.parent / 'after.csv'; p.write_bytes(out.getvalue().encode('cp949'))
        data['csv_after']['sha256'] = digest(p.read_bytes())
        if mode: data['csv_comparison'] = 'exact_rows_ignore_order'
        path.write_text(json.dumps(data)); return path

    def test_order_only_difference_requires_explicit_mode_and_retains_before_order(self):
        with self.assertRaises(ValueError): rra.validate(self.save(False))
        result = rra.import_bundle(self.fixture.c, self.save())
        self.assertEqual(result['registry_rows'], 2)
        row = self.fixture.c.db.execute('SELECT certificate_number,cells_json FROM rra_equipment WHERE row_number=1').fetchone()
        self.assertEqual(row[0], 'R-R-ZZR-MAIN10')
        self.assertEqual(json.loads(row[1]), rra.csv_rows(self.fixture.csv)[0])
        self.assertIn('certificate-keyed', self.fixture.c.db.execute('SELECT coverage FROM sources').fetchone()[0])

    def test_changed_missing_duplicate_and_extra_rows_still_reject_before_replacement(self):
        rra.import_bundle(self.fixture.c, self.save())
        before = [tuple(r) for r in self.fixture.c.db.execute('SELECT * FROM records')]
        changes = [lambda rows: rows[0].__setitem__(1, 'Changed applicant'),
                   lambda rows: rows[0].__setitem__(-2, 'Changed status'),
                   lambda rows: rows.pop(),
                   lambda rows: rows[1].__setitem__(-1, rows[0][-1]),
                   lambda rows: rows[1].__setitem__(0, '1'),
                   lambda rows: rows.append(['3', *rows[0][1:-1], 'EXTRA-3'])]
        for change in changes:
            with self.assertRaises(ValueError): rra.import_bundle(self.fixture.c, self.save(change=change))
            self.assertEqual([tuple(r) for r in self.fixture.c.db.execute('SELECT * FROM records')], before)


class SunPortfolioTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_rra.RRATests(); self.fixture.setUp(); self.c = self.fixture.c
        self.fixture.rows[0]['manufacturer'] = 'Sun Microsystems, Inc.'
        self.fixture.rows[0]['approval_date_raw'] = '2001-01-01'
        self.fixture.rows[1]['manufacturer'] = 'Other Company, Sun Microsystems Inc.'

    def tearDown(self):
        self.fixture.tearDown()

    def save(self, changes=None):
        p = self.fixture.save(); bundle = json.loads(p.read_text()); bundle['query']['maker'] = 'Sun Microsystems'
        proof = dict(schema_version=1, source_url=rra_sun.URL, capture_method=rra_sun.METHOD,
                     reviewed_on='2026-09-27', filing='Form 8-K', item='2.01',
                     registrant='Oracle Corporation', acquired_company='Sun Microsystems, Inc.',
                     completed_on='2010-01-26', announcement_on='2010-01-27',
                     relationship='wholly owned subsidiary')
        proof.update(changes or {})
        evidence = p.parent / 'lineage.json'; evidence.write_text(json.dumps(proof))
        bundle['sun_lineage'] = dict(path=evidence.name, sha256=digest(evidence.read_bytes()))
        p.write_text(json.dumps(bundle)); return p

    def test_historical_manufacturer_and_acquired_portfolio_are_separate_roles(self):
        for _ in range(2):
            self.assertEqual(rra_sun.import_bundle(self.c, self.save()),
                             dict(registry_rows=2, matched_manufacturer_rows=1, portfolio_assertions=3))
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT brand FROM devices')}, {'Sun Microsystems'})
        self.assertEqual(vendor_inventory.run(self.c.db, 'Oracle')['unique_identities'], 0)
        self.assertEqual(vendor_inventory.run(self.c.db, 'Oracle', scope='all')['unique_identities'], 3)
        self.assertEqual(len(self.c.search(target='Oracle', role=rra_sun.ROLE)), 3)
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertIsNone(target_for('Sun Microsystems'))
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        self.assertEqual(self.c.db.execute("SELECT DISTINCT value FROM facts WHERE name='lineage_completed_on'").fetchone()[0], '2010-01-26')
        self.fixture.rows[0]['manufacturer'] = 'Sun Microsystems Scotland B.V.'
        rra_sun.import_bundle(self.c, self.save())
        self.assertEqual(self.c.search(), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0], 2)

    def test_only_explicit_whole_names_match(self):
        self.assertEqual(rra_sun.company_basis('SUN MICROSYSTEMS'), 'reviewed short company name without Inc suffix')
        for name in ['Sun Microsystems Computer Corp.', 'Sun Microsystems Scotland B.V.',
                     'Sun Microsystems Company', 'Other (Sun Microsystems Inc.)',
                     '다른 회사 (Sun Microsystems Inc.)']:
            self.assertIsNone(rra_sun.company_basis(name))

    def test_missing_or_wrong_lineage_fails_before_either_source_changes(self):
        rra_sun.import_bundle(self.c, self.save())
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for changes in [dict(completed_on='2009-04-20'), dict(acquired_company='Another Company'),
                        dict(capture_method='original HTML'), dict(item='8.01')]:
            with self.assertRaises(ValueError): rra_sun.import_bundle(self.c, self.save(changes))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)
        p = self.save(); data = json.loads(p.read_text()); data.pop('sun_lineage'); p.write_text(json.dumps(data))
        with self.assertRaises(ValueError): rra_sun.import_bundle(self.c, p)
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)


if __name__ == '__main__':
    unittest.main()

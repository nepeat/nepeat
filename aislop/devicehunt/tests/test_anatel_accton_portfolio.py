import json
import unittest

from devicehunt import anatel, anatel_accton_portfolio as p, vendor_inventory
from devicehunt.catalog import digest
from tests import test_anatel


class PortfolioTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_anatel.AnatelTests(); self.fixture.setUp()
        self.c = self.fixture.c
        self.manufacturer = [k for k, _ in anatel.FIELDS].index('manufacturer')
        self.fixture.rows[0][self.manufacturer] = p.COMPANY

    def tearDown(self):
        self.fixture.tearDown()

    def save(self, changes=None):
        registry = self.fixture.save()
        proof = dict(p.PROOF, reviewed_on='2026-09-27'); proof.update(changes or {})
        evidence = registry.parent / 'company.json'; evidence.write_text(json.dumps(proof))
        entry = lambda file: dict(path=file.name, sha256=digest(file.read_bytes()))
        path = registry.parent / 'portfolio.json'
        path.write_text(json.dumps(dict(schema_version=1, registry=entry(registry),
                                        company_evidence=entry(evidence))))
        return path

    def test_roles_dates_duplicates_and_refresh_withdrawal(self):
        path = self.save()
        anatel.import_bundle(self.c, self.fixture.root / 'bundle.json')
        raw_before = [tuple(r) for r in self.c.db.execute('SELECT * FROM anatel_equipment')]
        for _ in range(2):
            self.assertEqual(p.import_bundle(self.c, path),
                             dict(registry_rows_checked=6, portfolio_assertions=2))
        self.assertEqual(vendor_inventory.run(self.c.db, 'Accton')['unique_identities'], 0)
        self.assertEqual(vendor_inventory.run(self.c.db, 'Accton', scope='all')['unique_identities'], 1)
        self.assertEqual(len(self.c.search(target='Accton', role=p.ROLE)), 1)
        self.assertFalse(self.c.search(hyperscaler=True))
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        facts = dict(self.c.db.execute("SELECT name,value FROM facts WHERE name LIKE 'lineage_%'"))
        self.assertEqual(facts['lineage_control_transferred_on'], '2025-04-30')
        self.assertEqual(facts['lineage_subsidiary_stated_on'], '2024-09-26')
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM anatel_equipment')], raw_before)
        self.fixture.rows[0][self.manufacturer] = 'Other (' + p.COMPANY + ')'
        p.import_bundle(self.c, self.save())
        self.assertFalse(self.c.search(target='Accton', role=p.ROLE))

    def test_unreviewed_companies_and_composite_names_are_not_mapped(self):
        for name in ['Joy Technology Ltd.', 'Accton Technology (China) CO. LTD',
                     p.COMPANY + ' / Other', 'Joysun Technology', '別 ' + p.COMPANY]:
            self.fixture.rows[0][self.manufacturer] = name
            self.assertEqual(p.import_bundle(self.c, self.save())['portfolio_assertions'], 0)

    def test_invalid_corporate_or_registry_evidence_preserves_existing_records(self):
        p.import_bundle(self.c, self.save())
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for changes in [dict(company='Other'), dict(control_transferred_on='2024-09-26'),
                        dict(capture_method='original PDF'), dict(disposal_note='12')]:
            with self.assertRaises(ValueError): p.import_bundle(self.c, self.save(changes))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)
        path = self.save()
        (path.parent / 'before.zip').write_bytes(b'changed')
        with self.assertRaises(ValueError): p.import_bundle(self.c, path)
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)

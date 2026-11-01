import json
import unittest

from devicehunt import rra, rra_company_names as names
from devicehunt.catalog import digest, target_for
from tests import test_rra


class AcctonCompanyTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_rra.RRATests()
        self.fixture.setUp()
        self.c = self.fixture.c
        self.fixture.rows[0]['manufacturer'] = 'ACCTON TECHNOLOGY CORP. PTE LTD.'
        self.fixture.rows[1]['manufacturer'] = 'Other Company (Accton Technology Corp. Pte Ltd)'
        self.capture = dict(schema_version=1, source_url=names.ACCTON_URL,
            capture_method=names.ACCTON_METHOD, reviewed_on='2026-09-27',
            reporting_year='2013', document='2013-Consolidated-Report.pdf',
            reported_parent='Edgecore Networks', relationship='wholly owned subsidiaries',
            subsidiaries=sorted(names.ACCTON_NAMES))

    def tearDown(self):
        self.fixture.tearDown()

    def save(self, evidence=True, changes=None):
        path = self.fixture.save()
        bundle = json.loads(path.read_text())
        bundle['query']['maker'] = 'Accton'
        if evidence:
            data = {**self.capture, **(changes or {})}
            p = path.parent / 'company.json'
            p.write_text(json.dumps(data))
            bundle['company_evidence'] = dict(path=p.name, sha256=digest(p.read_bytes()))
        path.write_text(json.dumps(bundle))
        return path

    def test_scoped_identity_preserves_raw_company_and_parent_provenance(self):
        self.assertEqual(rra.import_bundle(self.c, self.save(False))['model_assertions'], 0)
        self.assertEqual(rra.import_bundle(self.c, self.save())['model_assertions'], 3)
        rows = list(self.c.db.execute("SELECT company,target FROM affiliations WHERE role='registry_manufacturer'"))
        self.assertEqual({tuple(r) for r in rows}, {('ACCTON TECHNOLOGY CORP. PTE LTD.', 'Accton')})
        self.assertIsNone(target_for('ACCTON TECHNOLOGY CORP. PTE LTD.'))
        facts = dict(self.c.db.execute("SELECT name,value FROM facts WHERE name LIKE 'manufacturer_company_%'"))
        self.assertEqual(facts['manufacturer_company_reported_parent'], 'Edgecore Networks')
        self.assertEqual(facts['manufacturer_company_reporting_year'], '2013')
        self.assertEqual(facts['manufacturer_company_capture_method'], names.ACCTON_METHOD)
        self.assertEqual(self.c.search(target='Edge-Core'), [])
        coverage = self.c.db.execute('SELECT coverage FROM sources').fetchone()[0]
        self.assertIn('2013 Accton', coverage)
        self.assertNotIn('Celestica 2025', coverage)
        rra.import_bundle(self.c, self.save(False))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0], 2)

    def test_unreviewed_or_composite_names_do_not_inherit_mapping(self):
        path = self.save()
        mapping = names.load(path.parent, json.loads(path.read_text())['company_evidence'])
        for company in ['Accton Technology Inc.', 'Accton Technology Incorporation',
                        'Accton Technology(SHENZHEN) Corp.', 'SMC Networks Inc.',
                        'Accton Technology SG Pte Ltd', 'Other, Accton Technology Corp. Pte Ltd',
                        '다른 회사 (Accton Technology Corp. Pte Ltd)']:
            self.assertIsNone(rra.company_target(company, mapping), company)

    def test_changed_evidence_cannot_replace_prior_claims(self):
        rra.import_bundle(self.c, self.save())
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for changes in [dict(reporting_year='2025'), dict(reported_parent='Accton'),
                        dict(relationship='manufacturing factory'), dict(capture_method='original PDF'),
                        dict(subsidiaries=sorted(names.ACCTON_NAMES) + ['SMC Networks Inc.'])]:
            with self.assertRaises(ValueError):
                rra.import_bundle(self.c, self.save(changes=changes))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], before)


if __name__ == '__main__':
    unittest.main()

import json
import unittest

from devicehunt import rra, rra_company_names
from devicehunt.catalog import digest, target_for
from tests import test_rra


class RRACompanyNamesTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_rra.RRATests()
        self.fixture.setUp()
        self.c = self.fixture.c
        self.capture = dict(schema_version=1, source_url=rra_company_names.URL,
            capture_method=rra_company_names.METHOD, reviewed_on='2026-09-26',
            reporting_date='2025-12-31', registrant='Celestica Inc.', exhibit='21.1',
            heading='Subsidiaries of the Registrant', subsidiaries=[
                'Celestica Cayman Holdings 1 Limited', 'Celestica LLC',
                'Celestica (Thailand) Limited', 'Celestica (USA) Inc.',
                'Celestica Holdings Pte Limited'])

    def tearDown(self):
        self.fixture.tearDown()

    def save(self, capture=None):
        path = self.fixture.save()
        data = json.loads(path.read_text())
        evidence = self.fixture.root/'companies.json'
        evidence.write_text(json.dumps(self.capture if capture is None else capture))
        data['company_evidence'] = dict(path=evidence.name, sha256=digest(evidence.read_bytes()))
        path.write_text(json.dumps(data))
        return path

    def test_optional_evidence_enables_exact_legal_name_with_record_provenance(self):
        self.fixture.rows[0]['manufacturer'] = 'CELESTICA LLC'
        self.assertEqual(rra.import_bundle(self.c,self.fixture.save())['model_assertions'],0)
        path = self.save()
        self.assertEqual(rra.import_bundle(self.c,path)['model_assertions'],3)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM affiliations WHERE target='Celestica' AND role='registry_manufacturer'").fetchone()[0],3)
        self.assertEqual(self.c.db.execute("SELECT DISTINCT value FROM facts WHERE name='manufacturer_company_legal_name'").fetchone()[0],'Celestica LLC')
        self.assertEqual(self.c.db.execute("SELECT DISTINCT value FROM facts WHERE name='manufacturer_company_capture_method'").fetchone()[0],rra_company_names.METHOD)
        self.assertEqual(self.c.db.execute('SELECT manufacturer FROM rra_equipment WHERE row_number=1').fetchone()[0],'CELESTICA LLC')
        self.assertIsNone(target_for('CELESTICA LLC'))
        rra.import_bundle(self.c,self.fixture.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],0)

    def test_only_reviewed_whole_company_spellings_and_explicit_abbreviations_map(self):
        self.fixture.rows[0]['manufacturer'] = 'CELESTICA(THAILAND) LTD.'
        self.fixture.rows[0]['applicant'] = 'Celestica Holdings Pte Ltd.'
        self.fixture.rows[1]['manufacturer'] = '다른 회사 (Celestica LLC)'
        path = self.save()
        self.assertEqual(rra.import_bundle(self.c,path)['model_assertions'],3)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM affiliations WHERE target='Celestica' AND role='regulatory_applicant'").fetchone()[0],3)
        self.assertEqual(self.c.db.execute("SELECT DISTINCT value FROM facts WHERE name='manufacturer_company_mapping_basis'").fetchone()[0],'reviewed Ltd abbreviation of Limited')
        mappings = rra_company_names.load(path.parent,json.loads(path.read_text())['company_evidence'])
        for name in ('Celestica LLC, Other Corp', 'Other Company (Celestica LLC)',
                     '다른 회사 (Celestica LLC)', 'CELESTICA THAILAND HOLDINGS LTD',
                     'Celestica Cayman Holdings 1 Limited'):
            self.assertIsNone(rra.company_target(name,mappings),name)
        self.assertEqual(rra.company_target('Celestica Holdings Pte Limited',mappings),'Celestica')

    def test_invalid_evidence_fails_before_replacing_existing_records(self):
        self.fixture.rows[0]['manufacturer'] = 'Celestica LLC'
        rra.import_bundle(self.c,self.save())
        before = [tuple(row) for row in self.c.db.execute('SELECT * FROM records')]
        for change in ('hash','url','period','method','names','duplicate','path'):
            path = self.save(); bundle = json.loads(path.read_text())
            evidence = self.fixture.root/'companies.json'
            data = json.loads(evidence.read_text())
            if change=='hash':evidence.write_text('{}')
            elif change=='path':bundle['company_evidence']['path']='../companies.json'
            else:
                if change=='url':data['source_url']='https://example.org/filing'
                if change=='period':data['reporting_date']='2000-12-31'
                if change=='method':data['capture_method']='original HTML'
                if change=='names':data['subsidiaries'][1]='Another Company LLC'
                if change=='duplicate':data['subsidiaries'].append('Celestica LLC')
                evidence.write_text(json.dumps(data))
                bundle['company_evidence']['sha256']=digest(evidence.read_bytes())
            path.write_text(json.dumps(bundle))
            with self.assertRaises(ValueError):rra.import_bundle(self.c,path)
            self.assertEqual([tuple(row) for row in self.c.db.execute('SELECT * FROM records')],before)

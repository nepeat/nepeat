import json
import unittest

from devicehunt import rra, rra_company_names, rra_celestica_historical as historical
from devicehunt.catalog import digest, target_for
from tests import test_rra_company_names


class HistoricalCompanyTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_rra_company_names.RRACompanyNamesTests()
        self.fixture.setUp()
        self.c = self.fixture.c
        self.root = self.fixture.fixture.root

    def tearDown(self):
        self.fixture.tearDown()

    def entry(self, name, data):
        path = self.root / name
        path.write_text(json.dumps(data))
        return dict(path=name, sha256=digest(path.read_bytes()))

    def save(self, change=None):
        path = self.fixture.save()
        bundle = json.loads(path.read_text())
        data = dict(schema_version=2, kind='celestica-historical-company-names',
                    base=bundle['company_evidence'], exhibits={})
        for key, spec in historical.EXHIBITS.items():
            evidence = {k: spec[k] for k in ('source_url', 'capture_method', 'exhibit')}
            evidence.update(schema_version=1, registrant='Celestica Inc.',
                            relationship='subsidiaries of registrant',
                            reviewed_on='2026-09-27', evidence_period=historical.PERIOD,
                            subsidiaries=sorted(spec['names']))
            if key == 'sec-8.1' and change:
                if change == 'url': evidence['source_url'] = 'https://example.org'
                if change == 'period': evidence['evidence_period'] = '2018'
                if change == 'names': evidence['subsidiaries'][0] = 'Other Company'
                if change == 'method': evidence['capture_method'] = 'original HTML'
            data['exhibits'][key] = self.entry(key + '.json', evidence)
        if change == 'hash': data['exhibits']['sec-8.1']['sha256'] = '0' * 64
        if change == 'nested':
            data['base'] = self.entry('nested.json', dict(schema_version=2, kind=data['kind']))
        bundle['company_evidence'] = self.entry('historical.json', data)
        path.write_text(json.dumps(bundle))
        return path

    def test_new_names_preserve_scope_provenance_and_withdraw_on_refresh(self):
        self.fixture.fixture.rows[0]['manufacturer'] = 'Celestica(DongGuan-SSL) Technology Ltd.'
        self.fixture.fixture.rows[0]['applicant'] = 'CELESTICA LLC'
        path = self.save()
        self.assertEqual(rra.import_bundle(self.c, path)['model_assertions'], 3)
        facts = dict(self.c.db.execute('SELECT name,value FROM facts'))
        self.assertEqual(facts['manufacturer_company_evidence_period'], historical.PERIOD)
        self.assertIn('ownership at approval date', facts['manufacturer_company_scope'])
        self.assertEqual(facts['manufacturer_company_legal_name'],
                         'Celestica (Dongguan-SSL) Technology Limited')
        self.assertEqual(facts['applicant_company_reporting_date'], '2025-12-31')
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0], 0)
        self.assertIsNone(target_for('Celestica(DongGuan-SSL) Technology Ltd.'))
        rra.import_bundle(self.c, self.fixture.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 0)

    def test_whole_cells_only_without_geographic_or_composite_inference(self):
        path = self.save()
        mappings = rra_company_names.load(self.root, json.loads(path.read_text())['company_evidence'])
        for name in ('Dongguan Celestica Electronics Ltd.', 'Celestica Kladno s.r.o.',
                     'Celestica (SuZhou) Technology Co.Ltd', 'CELESTICA HONG KONG LTD'):
            self.assertEqual(rra.company_target(name, mappings), 'Celestica', name)
        for name in ('Other Corp, Celestica Hong Kong Ltd', 'Celestica Czech Republic',
                     '다른 회사 (Celestica Kladno s.r.o.)',
                     'Celestica (Dongguan-SSL) Technology Ltd Northern Area'):
            self.assertIsNone(rra.company_target(name, mappings), name)

    def test_bad_dependencies_fail_before_source_replacement(self):
        self.fixture.fixture.rows[0]['manufacturer'] = 'Celestica Hong Kong Ltd'
        rra.import_bundle(self.c, self.save())
        before = [tuple(row) for row in self.c.db.execute('SELECT * FROM records')]
        for change in ('url', 'period', 'names', 'method', 'hash', 'nested'):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    rra.import_bundle(self.c, self.save(change))
                self.assertEqual([tuple(row) for row in self.c.db.execute('SELECT * FROM records')], before)

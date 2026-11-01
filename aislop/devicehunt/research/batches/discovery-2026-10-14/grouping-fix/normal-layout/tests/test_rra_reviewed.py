import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import rra_reviewed as rra, vendor_inventory
from devicehunt.catalog import Catalog, digest


class ReviewedRRATests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.capture = dict(schema_version=1, capture_method=rra.METHOD,
            reviewed_on='2026-09-26', source_url='https://fccid.io/RR-ZZR-CS8000-32X-DC',
            language='en', derived_models=['CS8001-32X-DC-11'], fields={
                'Record Type': 'Radio conformity assessment',
                'Authorization / Certificate Number': 'RR-ZZR-CS8000-32X-DC',
                'Applicant / Company': 'Other Applicant', 'Device / Product Name': 'Switch',
                'Model': 'CS8000-32X-DC-11', 'Derived Models': 'CS8001-32X-DC-11',
                'Manufacturer': 'CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO LTD',
                'Country': 'China (Mexico, Thailand)', 'Date': '2020-05-19'})

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, captures=None):
        entries = []
        for n, capture in enumerate(captures or [self.capture]):
            file = self.root / ('capture' + str(n) + '.json')
            file.write_text(json.dumps(capture))
            entries.append(dict(path=file.name, sha256=digest(file.read_bytes())))
        path = self.root / 'bundle.json'
        path.write_text(json.dumps(dict(schema_version=1, captures=entries)))
        return path

    def test_main_and_derived_models_retain_roles_without_aliases_or_fcc_claims(self):
        result = rra.import_bundle(self.c, self.save())
        self.assertEqual(result, dict(sources=1, model_assertions=2))
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},
                         {'CS8000-32X-DC-11', 'CS8001-32X-DC-11'})
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT kind FROM identifiers')},
                         {'rra_certificate_id'})
        roles = [tuple(r) for r in self.c.db.execute(
            "SELECT target,role FROM affiliations ORDER BY role,target")]
        self.assertEqual(roles.count(('Celestica', 'registry_manufacturer')), 2)
        self.assertEqual(roles.count((None, 'regulatory_applicant')), 2)
        self.assertEqual({r[0] for r in self.c.db.execute(
            "SELECT value FROM facts WHERE name='rra_model_role'")}, {'main', 'derived'})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0], 0)
        inventory = vendor_inventory.run(self.c.db, scope='manufacturer', limit=-1)
        self.assertEqual(inventory['unique_identities'], 2)
        source = self.c.db.execute('SELECT * FROM sources').fetchone()
        self.assertEqual(source['tier'], 'regulatory_mirror')
        self.assertIn('not original HTML', source['coverage'])
        self.assertTrue(source['sha256'])

    def test_malformed_later_capture_and_changed_hash_cannot_replace_earlier_source(self):
        rra.import_bundle(self.c, self.save())
        original = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        bad = []
        for key, value in [('source_url', 'https://example.org'), ('reviewed_on', '2026-02-30'),
                           ('language', 'ko'), ('derived_models', ['different']),
                           ('derived_models', 'CS8001-32X-DC-11')]:
            d = copy.deepcopy(self.capture); d[key] = value; bad.append(d)
        for key, value in [('Model', 'ONE / TWO'), ('Manufacturer', 'Unknown'),
                           ('Date', 'not a date'), ('Country', 'two\nlines')]:
            d = copy.deepcopy(self.capture); d['fields'][key] = value; bad.append(d)
        d = copy.deepcopy(self.capture)
        d['derived_models'] = [d['fields']['Model']]
        d['fields']['Derived Models'] = d['fields']['Model']; bad.append(d)
        for invalid in bad:
            with self.assertRaises(ValueError):
                rra.import_bundle(self.c, self.save([self.capture, invalid]))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], original)
        with self.assertRaisesRegex(ValueError, 'Repeated'):
            rra.import_bundle(self.c, self.save([self.capture, self.capture]))
        path = self.save(); (self.root / 'capture0.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            rra.import_bundle(self.c, path)
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')], original)

    def test_reimport_and_refresh_do_not_remove_unrelated_evidence(self):
        path = self.save()
        rra.import_bundle(self.c, path); rra.import_bundle(self.c, path)
        self.c.source('other', 'https://example.org', 'vendor', 'fixture', 'fixture')
        self.c.add('other', 'retained', 'https://example.org', 'Celestica',
                   'CS8001-32X-DC-11', 'retained', 'switch', {})
        self.c.finish('other', 1)
        self.capture['derived_models'] = []
        self.capture['fields']['Derived Models'] = ''
        rra.import_bundle(self.c, self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM records WHERE source_id='other'").fetchone()[0], 1)

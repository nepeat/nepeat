import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import listings
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog


class SoftwareReferenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'db.sqlite'
        self.c = Catalog(self.path)
        self.c.source('vendor', 'https://example.org/v', 'vendor', 'fixture', 'Test fixture')
        self.c.source('proj', 'https://example.org/p', 'project', 'fixture', 'Test fixture')
        add = self.c.add
        add('vendor', 'hw', 'https://example.org/hw', 'Accton', 'Wedge 100BF-32QS', 't', 'network switch', {}, [('Accton', 'brand')])
        add('vendor', 'hw2', 'https://example.org/hw2', 'Accton', 'Zebra 1000', 't', 'switch', {}, [('Accton', 'brand')])
        add('proj', 'sw1', 'https://example.org/sw1', 'Meta OpenBMC platform', 'wedge', 't', 'BMC firmware platform definition (software reference)', {}, [])
        add('proj', 'sw2', 'https://example.org/sw2', 'Some New Project', 'zebra', 't', 'host firmware definition', {}, [])
        add('proj', 'sw3', 'https://example.org/sw3', 'OpenBMC machine', 'zaius', 't', '', {}, [])
        self.c.finish('vendor', 2); self.c.finish('proj', 3)

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def test_default_excludes_by_brand_and_by_kind(self):
        m = listings.Matcher(self.c.db)
        got = m.match('Wedge 100BF-32QS Zebra 1000 zaius zebra wedge')
        self.assertEqual([(r['brand'], r['best_match']) for r in got['candidates']],
                         [('Accton', 'model_mention'), ('Accton', 'model_mention')])
        self.assertNotIn('software_reference_mention', got['candidate_counts'])
        self.assertNotIn('software_references_included', got)
        self.assertEqual(m.match('Fuji zaius wedge')['status'], 'no_catalog_mention')

    def test_included_sorts_after_hardware_under_small_limit(self):
        m = listings.Matcher(self.c.db, include_software_references=True)
        title = 'Wedge 100BF-32QS Zebra 1000 zaius zebra wedge'
        got = m.match(title)
        self.assertEqual([r['best_match'] for r in got['candidates']],
                         ['model_mention', 'model_mention'] + ['software_reference_mention'] * 3)
        self.assertEqual(got['candidate_counts']['software_reference_mention'], 3)
        self.assertTrue(got['software_references_included'])
        small = m.match(title, limit=2)
        self.assertEqual({r['brand'] for r in small['candidates']}, {'Accton'})
        self.assertTrue(small['truncated'])
        only = m.match('zaius')
        self.assertEqual(only['status'], 'software_references_only')

    def test_hardware_results_identical_with_or_without_flag(self):
        title = 'Wedge 100BF-32QS'
        a = listings.Matcher(self.c.db).match(title)['candidates']
        b = [r for r in listings.Matcher(self.c.db, include_software_references=True).match(title)['candidates']
             if r['best_match'] != 'software_reference_mention']
        self.assertEqual(a, b)

    def test_cli_flag(self):
        def run(*extra):
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(main(['--db', str(self.path), 'match-listing', '--title', 'wedge zaius',
                                       '--format', 'json', *extra]), 0)
            return json.loads(out.getvalue())['results'][0]['result']
        self.assertEqual(run()['candidates'], [])
        got = run('--include-software-references')
        self.assertEqual({r['best_match'] for r in got['candidates']}, {'software_reference_mention'})

    def test_definition(self):
        self.assertTrue(listings.is_software_reference('SONiC platform', 'whatever'))
        self.assertTrue(listings.is_software_reference('X', 'Linux BMC device tree source'))
        self.assertFalse(listings.is_software_reference('Accton', 'network platform'))

    def test_every_listed_brand_hidden_with_empty_kind(self):
        for brand in sorted(listings.SOFTWARE_REFERENCE_BRANDS):
            self.assertTrue(listings.is_software_reference(brand, ''), brand)
            self.assertTrue(listings.is_software_reference(brand.upper(), ''), brand)
        self.assertFalse(listings.is_software_reference('Accton', 'switch'))
        self.assertFalse(listings.is_software_reference('Wiwynn', 'multi-node server platform'))

import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.listings import Matcher


class AccessoryRevisionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'catalog.sqlite'
        self.catalog = Catalog(self.path)
        self.catalog.source('vendor', 'https://example.org', 'vendor', 'fixture', 'Fixture')
        for index, (brand, model, kind, aliases) in enumerate([
            ('Unattributed accessory', 'YM-2651Y B01R', 'Power Supply FRU', []),
            ('Unattributed accessory', 'YM-2651Y, B01R', 'Power Supply FRU', []),
            ('Unattributed accessory', 'YM-2651Y C01R', 'Power Supply FRU', []),
            ('Unattributed accessory', 'DPS-850AB-5 A', 'FRU', []),
            ('Unattributed accessory', 'YM-2651Y D01R', 'ordering item (type not printed)', []),
            ('Example', 'YM-2651Y B01R', 'server', []),
            ('Unattributed accessory', 'OTHER-100', 'FRU', [('alias', 'YM-2651Y B01R')]),
        ]):
            self.catalog.add('vendor', str(index), 'https://example.org/' + str(index),
                             brand, model, model, kind, {}, [('Edge-Core', 'brand')], aliases,
                             [('power_description', 'Fixture rating; exact record only')])
        self.catalog.finish('vendor', 7)
        self.matcher = Matcher(self.catalog.db, include_specifications=True)

    def tearDown(self):
        self.catalog.db.close()
        self.tmp.cleanup()

    def test_bare_model_returns_distinct_revision_leads_without_specs(self):
        before = self.path.read_bytes()
        result = self.matcher.match('YM-2651Y power supply')
        self.assertEqual(result['status'], 'variant_leads_only')
        self.assertEqual({c['model'] for c in result['candidates']},
                         {'YM-2651Y B01R', 'YM-2651Y, B01R', 'YM-2651Y C01R'})
        for candidate in result['candidates']:
            self.assertEqual(candidate['brand'], 'Unattributed accessory')
            for match in candidate['matches']:
                self.assertEqual(match['related_title_tokens'], ['ym-2651y'])
                self.assertEqual(match['specifications'], [])
        self.assertEqual(self.path.read_bytes(), before)

    def test_hyphenated_revision_is_only_a_lead_to_same_printed_revision(self):
        for title in ['YM-2651Y-B01R PSU', 'ym\u20112651y\u2011b01r PSU']:
            result = self.matcher.match(title)
            self.assertEqual(result['status'], 'variant_leads_only')
            self.assertEqual({c['model'] for c in result['candidates']},
                             {'YM-2651Y B01R', 'YM-2651Y, B01R'})
            for candidate in result['candidates']:
                for match in candidate['matches']:
                    self.assertEqual(match['related_title_tokens'], ['ym-2651y-b01r'])
                    self.assertEqual(match['specifications'], [])

    def test_exact_forms_keep_identity_and_specification_behavior(self):
        for title in ['YM-2651Y B01R', 'YM-2651Y, B01R']:
            result = self.matcher.match(title)
            exact = [c for c in result['candidates'] if c['best_match'] == 'model_mention']
            self.assertTrue(exact)
            self.assertTrue(all(c['model'] == title for c in exact))
            self.assertTrue(all(c['matches'][0]['specifications'] for c in exact))
            leads = [c for c in result['candidates'] if c['best_match'] == 'variant_lead']
            self.assertTrue(leads)
            self.assertTrue(all(not m['specifications'] for c in leads for m in c['matches']))

    def test_other_fru_revision_and_boundaries(self):
        for title in ['DPS-850AB-5', 'DPS-850AB-5-A']:
            result = self.matcher.match(title)
            self.assertEqual([(c['model'], c['best_match']) for c in result['candidates']],
                             [('DPS-850AB-5 A', 'variant_lead')])
        for title in ['XYM-2651Y', 'YM-2651YA', 'YM-2651Y_B01R', 'YM-2651Y-B01RX',
                      'YM-2651Y-B01R+', 'YM-2651Y-C99R', 'YM-2651Y-B01R_extra']:
            self.assertEqual(self.matcher.match(title)['matches'], 0, title)

    def test_vendor_filter_and_limit_are_preserved(self):
        self.assertEqual(Matcher(self.catalog.db, vendor='Oracle').match('YM-2651Y')['matches'], 0)
        result = Matcher(self.catalog.db, vendor='Edge-Core').match('YM-2651Y', limit=0)
        self.assertEqual(result['matches'], 3)
        self.assertEqual(result['shown'], 0)
        self.assertTrue(result['truncated'])


if __name__ == '__main__':
    unittest.main()

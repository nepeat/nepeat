"""Use unchanged production routing against an isolated proposed dataset."""
from pathlib import Path
import tempfile
import unittest

from devicehunt import listings
from devicehunt.__main__ import shopping_links
from devicehunt.catalog import Catalog
from devicehunt.importers import curated

ROOT = Path(__file__).resolve().parents[1]


class SourceReferenceIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.catalog = Catalog(Path(self.tmp.name) / 'candidate.sqlite')
        self.addCleanup(self.catalog.db.close)
        self.assertEqual(curated(self.catalog, ROOT / 'candidate.json'), 11)

    def test_all_listings_recognized_as_software_references(self):
        rows = list(self.catalog.db.execute('SELECT brand,kind FROM devices'))
        self.assertEqual(len(rows), 11)
        self.assertTrue(all(listings.is_software_reference(r['brand'], r['kind']) for r in rows))

    def test_default_listing_matcher_excludes_hcl_reference(self):
        result = listings.Matcher(self.catalog.db).match('AS7726-32X AS4610-30T AG5648')
        self.assertEqual(result['candidates'], [])
        self.assertEqual(result['status'], 'no_catalog_mention')

    def test_explicit_reference_flag_preserves_separate_software_match_kind(self):
        result = listings.Matcher(self.catalog.db, include_software_references=True).match('AS7726-32X AS4610-30T AG5648')
        self.assertEqual(len(result['candidates']), 3)
        self.assertTrue(all(r['best_match'] == listings.SOFTWARE_MATCH for r in result['candidates']))
        self.assertEqual(result['status'], 'software_references_only')

    def test_shopping_links_empty_for_every_hcl_listing(self):
        for row in self.catalog.db.execute('SELECT id FROM devices'):
            self.assertEqual(shopping_links(self.catalog.detail(row['id'])), [])


if __name__ == '__main__':
    unittest.main()

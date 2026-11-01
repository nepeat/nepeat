import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ul, validation
from devicehunt.catalog import Catalog


class UlCaptureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'devices.sqlite')
        self.path = self.root / 'capture.json'
        self.data = {'schema_version': 1, 'capture_format': 'web_tool_model_extract',
                     'source_url': 'https://productiq.ulprospector.com/en/profile/2858121/azot.e362364',
                     'document': ul.DOCUMENT, 'listing_holder': ul.HOLDER, 'section': 'Models',
                     'retrieved_at': '2026-09-26 12:00:00 UTC',
                     'extraction_pattern': r'\b' + ul.MODEL_PATTERN + r'\b',
                     'models': ['CS8200-32X-UN-00', 'CS8210-32X-DC-11']}

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def ingest(self, data):
        self.path.write_text(json.dumps(data))
        return ul.import_capture(self.c, self.path)

    def test_listing_holder_and_exact_suffixes_with_explicit_capture_provenance(self):
        self.ingest(self.data)
        self.ingest(self.data)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        fixture = self.root / 'inventory.json'
        fixture.write_text(json.dumps({'schema_version': 1, 'devices': [
            {'identifier': 'CS8200-32X-UN-00', 'vendor': 'Celestica'},
            {'identifier': 'CS8210-32X-UN-00', 'vendor': 'Celestica'}]}))
        report = validation.run(self.c.db, fixture)
        self.assertEqual([r['status'] for r in report['results']], ['hit', 'gap'])
        evidence = report['results'][0]['evidence'][0]
        self.assertEqual(evidence['source_tier'], 'certification_body')
        self.assertIn('not original HTML', evidence['source_coverage'])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)

    def test_wrong_holder_source_or_incomplete_model_rejected_before_replacement(self):
        self.ingest(self.data)
        mutations = [('listing_holder', 'UNRELATED COMPANY'), ('source_url', 'https://example.org/'),
                     ('capture_format', 'raw_html'), ('models', ['CS8210-32X']),
                     ('models', ['CS8200-32X-UN-00'] * 2), ('retrieved_at', ''),
                     ('models', [])]
        for field, value in mutations:
            bad = copy.deepcopy(self.data)
            bad[field] = value
            with self.assertRaises(ValueError):
                self.ingest(bad)
            self.assertEqual(len(self.c.search('CS8200')), 1)


if __name__ == '__main__':
    unittest.main()

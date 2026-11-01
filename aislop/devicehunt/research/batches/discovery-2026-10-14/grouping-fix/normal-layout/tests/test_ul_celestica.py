import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ul_celestica as ul
from devicehunt.catalog import Catalog


class CelesticaUlTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, text, alter=None):
        data = dict(schema_version=1, capture_format='web_search_models_section',
                    source_url='https://productiq.ulprospector.com/en/profile/2858121/azot.e362364?term=switch',
                    document=ul.DOCUMENT, listing_holder=ul.HOLDER, section='Models',
                    extraction_scope=ul.SCOPE, retrieved_at='2026-09-27 02:10:48 UTC',
                    model_section_text=text, models=sorted(ul.extract(text)))
        if alter:
            alter(data)
        path = self.root / 'capture.json'
        path.write_text(json.dumps(data))
        return path

    def test_complete_qualifiers_case_and_duplicate_spans(self):
        text = 'DS4000-F 02 DS4000-R 02 SRU 0101 Voyager 1.1 Voyager PS390e DS4000-F 02'
        models = ul.extract(text)
        self.assertEqual(set(models), {'DS4000-F 02', 'DS4000-R 02', 'SRU 0101', 'Voyager 1.1', 'Voyager', 'PS390e'})
        self.assertEqual(len(models['DS4000-F 02']), 2)
        for model, spans in models.items():
            for start, end in spans:
                self.assertEqual(text[start:end], model)
        for text in ['DS4000-F 03', 'DX030 AC', 'DX030 (new)', 'Voyager 1.2', 'DS4000 48V']:
            with self.assertRaisesRegex(ValueError, 'qualifier'):
                ul.extract(text)

    def test_selected_scope_does_not_invent_trademark_or_component_aliases(self):
        models = ul.extract('CS8200-32X-UN-00 PLXI-001(for trademark HPE) BRIXIA IcePack Canister Wedge400C-48VDC-F DX030.rev DS4000_x S2123-xxxx')
        self.assertEqual(set(models), {'PLXI-001', 'BRIXIA', 'Wedge400C-48VDC-F'})

    def test_idempotent_holder_only_records_and_refresh(self):
        path = self.save('PLXI-001(for trademark HPE) Minipack2-AC DS4000-F 02')
        for _ in range(2):
            self.assertEqual(ul.import_capture(self.c, path), {'model_assertions': 3})
        self.assertEqual(len(self.c.search(target='Celestica', role='listing_holder')), 3)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(role='brand'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        ul.import_capture(self.c, self.save('Minipack2-AC'))
        self.assertEqual(self.c.search('PLXI-001'), [])
        self.assertEqual(len(self.c.search()), 1)

    def test_invalid_captures_preserve_existing_source(self):
        ul.import_capture(self.c, self.save('DX030'))
        for alter in [lambda d: d.update(listing_holder='HPE'),
                      lambda d: d.update(models=['DX031']),
                      lambda d: d.update(model_section_text='DX031'),
                      lambda d: d.update(capture_format='original_html'),
                      lambda d: d.update(retrieved_at='not a timestamp')]:
            with self.assertRaises(ValueError):
                ul.import_capture(self.c, self.save('DX030', alter))
            self.assertEqual(len(self.c.search('DX030')), 1)

    def test_pipeline_and_same_affiliation_role_filter(self):
        from devicehunt.pipeline import import_source
        from devicehunt.listings import Matcher
        import_source(self.c, 'ul-celestica-platforms', self.save('SSE-T8032S RM-400X3'))
        result = Matcher(self.c.db, 'Celestica').match('Surplus SSE-T8032S switch', 20)
        self.assertEqual(result['status'], 'mentions_found')
        self.assertEqual(self.c.search('SSE-T8032S', target='Celestica', role='manufacturer'), [])


if __name__ == '__main__':
    unittest.main()

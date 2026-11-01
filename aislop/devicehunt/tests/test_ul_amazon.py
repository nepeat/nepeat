import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ul_amazon as ul
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source


class AmazonUlTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, entries, alter=None):
        models, patterns = ul.extract(entries)
        data = dict(schema_version=1, capture_format='web_search_reviewed_model_entries',
                    source_url='https://productiq.ulprospector.com/en/profile/2859718/azot7.e515139',
                    document=ul.DOCUMENT, listing_holder=ul.HOLDER, section='Models',
                    extraction_scope=ul.SCOPE, retrieved_at='2026-09-27 05:45:10 UTC',
                    model_section_entries=entries, models=sorted(models),
                    unexpanded_patterns=sorted(patterns))
        if alter:
            alter(data)
        path = self.root / 'capture.json'
        path.write_text(json.dumps(data))
        return path

    def test_exact_names_and_patterns_are_disjoint(self):
        models, patterns = ul.extract(['Tetra', 'Tetra', 'BARRELEYE COMPUTE',
                                      'CS42_1RP_G2', 'AZ9075-64X-DC-42',
                                      'Tiger_Woods', 'Tiger_Woods-xxx', 'TE1AXYZ'])
        self.assertEqual(models['Tetra'], [0, 1])
        self.assertEqual(set(patterns), {'Tiger_Woods-xxx', 'TE1AXYZ'})
        self.assertNotIn('AZ9075-64X', models)
        self.assertNotIn('BARRELEYE', models)
        for name in ['Mystery*****', 'Mystery-xxx', 'Mystery AC', 'A,B', ' HP89j', 'HP89j.rev']:
            with self.assertRaises(ValueError):
                ul.extract(['Tetra', name])

    def test_holder_only_idempotent_and_withdrawn_records(self):
        path = self.save(['BARRELEYE COMPUTE', 'CS42_1RP_G2', 'Tiger_Woods-xxx'])
        for _ in range(2):
            self.assertEqual(import_source(self.c, 'ul-amazon-platforms', path),
                             {'model_assertions': 2, 'unexpanded_patterns': 1})
        self.assertEqual(len(self.c.search(target='Amazon Data Services', role='listing_holder')), 2)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(role='brand'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.search('Tiger_Woods'), [])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        ul.import_capture(self.c, self.save(['Wukong']))
        self.assertEqual(self.c.search('BARRELEYE'), [])
        self.assertEqual(len(self.c.search()), 1)

    def test_invalid_capture_preserves_prior_evidence(self):
        ul.import_capture(self.c, self.save(['Wukong']))
        for changes in [dict(listing_holder='Amazon.com Services'), dict(document='AZOT.E515139'),
                        dict(models=['Tetra']), dict(unexpanded_patterns=[]),
                        dict(capture_format='original_html'), dict(retrieved_at='invalid'),
                        dict(model_section_entries=['Wukong', 'Unreviewed-xxx'])]:
            with self.assertRaises(ValueError):
                ul.import_capture(self.c, self.save(['Wukong', 'TE1AXYZ'], lambda d: d.update(changes)))
            self.assertEqual(len(self.c.search('Wukong')), 1)


if __name__ == '__main__':
    unittest.main()

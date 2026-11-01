import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ocp
from devicehunt.catalog import Catalog


def product(pid, model):
    return dict(product_id=pid, url='https://www.opencompute.org/products/'+str(pid)+'/reviewed-model',
                heading_brand='Edgecore Networks', target_solution_provider='Edgecore Networks',
                retrieval_method='web_open_result', model_field=model, models=[model], facts=[])


class OcpTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'capture.json'
        self.c = Catalog(Path(self.tmp.name) / 'test.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def save(self, rows):
        self.path.write_text(json.dumps(dict(schema_version=1, capture_format='reviewed_web_tool_fields',
                                            retrieved_at='2026-09-26T23:25:00+00:00', products=rows)))
        return self.path

    def test_variants_are_separate_and_provider_is_not_manufacturer(self):
        rows = [product(97, 'AS7816-64X-B'), product(119, 'AS7816-64X-F')]
        rows[0]['facts'] = [['source_caveat', 'Contribution refers to AS7800-64X; identity unproven']]
        for _ in range(2):
            self.assertEqual(ocp.import_capture(self.c, self.save(rows)),
                             {'product_pages': 2, 'model_assertions': 2})
        self.assertEqual({r['model'] for r in self.c.search(target='Edge-Core', role='solution_provider')},
                         {'AS7816-64X-B', 'AS7816-64X-F'})
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 2)

    def test_explicit_group_expands_but_port_counts_do_not(self):
        row = product(678, 'unused')
        row.update(heading_brand='Celestica', target_solution_provider='Celestica',
                   model_field='Celestica ES1010/ES1050 1U 48-port 1GbE / 2.5GbE (PoE) Access Switch',
                   models=['ES1010', 'ES1050'])
        self.assertEqual(ocp.import_capture(self.c, self.save([row]))['model_assertions'], 2)
        self.assertEqual({r['model'] for r in self.c.search()}, {'ES1010', 'ES1050'})
        self.assertEqual(self.c.db.execute('SELECT records_seen FROM sources').fetchone()[0], 1)

    def test_bad_later_row_preserves_existing_source(self):
        old = product(99, 'AS5915-18X-O')
        ocp.import_capture(self.c, self.save([old]))
        changes = [dict(models=['AS5915-18X']), dict(target_solution_provider='Accton'),
                   dict(url='https://www.opencompute.org/products/100/wrong-id'),
                   dict(facts=[['manufacturer', 'Accton']]), dict(retrieval_method='original_html')]
        for change in changes:
            invalid = copy.deepcopy(old); invalid.update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                ocp.import_capture(self.c, self.save([product(97, 'AS7816-64X-B'), invalid]))
            self.assertEqual({r['model'] for r in self.c.search()}, {'AS5915-18X-O'})

    def test_numeric_product_identity_deduplicates_stale_slugs(self):
        first = product(97, 'AS7816-64X-B')
        second = copy.deepcopy(first)
        second['url'] = 'https://www.opencompute.org/products/97/old-unrelated-server-slug'
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            ocp.import_capture(self.c, self.save([first, second]))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM sources').fetchone()[0], 0)

import tempfile
import unittest
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import inventec
from devicehunt.pipeline import import_source


def card(model, slug=None, gated=False):
    return ('<a href="/en/product/Server/2U/' + (slug or model) + '"><h3>' + model + '</h3>'
            + ('<div class="member-mask">Login to view full product information</div>' if gated else '')
            + '<div class="p-title">Public <span>server</span> title</div></a>')


class InventecTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.path = self.root / 'index.html'
        self.c = Catalog(self.root / 'catalog.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def test_exact_headings_keep_parentheses_underscore_and_space(self):
        self.path.write_text(card('P9000AG7 (AC)') + card('K905G6_G6P') + card('EdgePro 2U'))
        result = import_source(self.c, 'inventec-server-index', self.path)
        self.assertEqual(result['model_assertions'], 3)
        self.assertEqual({r['model'] for r in self.c.search(target='Inventec')},
                         {'P9000AG7 (AC)', 'K905G6_G6P', 'EdgePro 2U'})
        self.assertEqual(self.c.search(target='Inventec', role='manufacturer'), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0], 0)

    def test_gated_card_keeps_public_name_without_promoting_details(self):
        self.path.write_text(card('NX-H1331', gated=True))
        result = inventec.import_page(self.c, self.path)
        self.assertEqual(result['public_names_with_gated_details'], 1)
        facts = dict(self.c.db.execute('SELECT name,value FROM facts'))
        self.assertEqual(facts['detail_access_status'], 'login required; not fetched')
        self.assertNotIn('public_index_subtitle', facts)

    def test_navigation_and_external_model_links_are_not_cards(self):
        html = (card('Horsea') + '<a href="/en/product/Server/2U/Horsea">View more</a>'
                + '<a href="https://example.org/en/product/Server/2U/Fake"><h3>Fake</h3></a>'
                + '<a href="/en/product/Storage/2U/Storage"><h3>Storage</h3></a>')
        self.assertEqual([r['model'] for r in inventec.parse(html)], ['Horsea'])

    def test_invalid_capture_preserves_previous_source(self):
        self.path.write_text(card('Horsea'))
        inventec.import_page(self.c, self.path)
        for text in ('<html>login</html>', card('Horsea') + card('Horsea'),
                     '<a href="/en/product/Server/2U/Broken"><h3>Broken</h3>'):
            self.path.write_text(text)
            with self.assertRaises(ValueError):
                inventec.import_page(self.c, self.path)
            self.assertEqual([r['model'] for r in self.c.search(target='Inventec')], ['Horsea'])

    def test_reimport_replaces_only_its_source_and_is_idempotent(self):
        self.c.source('other', 'https://example.org', 'vendor', 'fixture', 'fixture')
        self.c.add('other', 'old', 'https://example.org', 'Inventec', 'Other', 'Other', 'server', {}, [('Inventec', 'brand')])
        self.c.finish('other', 1)
        self.path.write_text(card('Horsea'))
        inventec.import_page(self.c, self.path)
        inventec.import_page(self.c, self.path)
        self.path.write_text(card('Seadra'))
        inventec.import_page(self.c, self.path)
        self.assertEqual({r['model'] for r in self.c.search(target='Inventec')}, {'Other', 'Seadra'})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)

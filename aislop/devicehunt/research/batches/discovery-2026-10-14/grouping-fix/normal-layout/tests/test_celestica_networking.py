import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import celestica_networking as n
from devicehunt.catalog import Catalog, digest


def tile(title, description, href='/example'):
    return ('<div class="market-tile same-height"><div><div><img src="x" /></div>'
            '<h3><a href="' + href + '">' + title + '</a></h3><p><p>' + description +
            '</p><p>Optimized for:</p><ul><li>Hyperscale marketing</li></ul></p>'
            '<a class="link-more" href="' + href + '">Learn More</a></div></div>')


def body():
    return ('<title>Networking Switches | Celestica</title>' +
            tile('DS6000', 'DS6000 is a 3U 64-port switch.') +
            tile('DS6001', 'DS6001 is a hybrid-cooled 2 OU switch.') +
            tile('DS4000/DS4001', 'DS4000/DS4001 is a 1U switch with port alternatives.') +
            tile('ES1500 - Newest Addition', 'ES1500 is a 1U switch.') +
            tile('Related Resources', 'Other things.'))


class CelesticaNetworkingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root/'db.sqlite')
        self.c.source(n.LEGACY_SOURCE, n.URL, 'vendor', 'test', 'unhashed review')
        self.c.add(n.LEGACY_SOURCE, 'DS6000', n.URL, 'Celestica', 'DS6000', 'Celestica DS6000',
                   'switch', {}, [('Celestica', 'brand')])
        self.c.finish(n.LEGACY_SOURCE, 1)
        self.c.source('other', 'https://example.org', 'vendor', 'test', 'test')
        self.c.add('other', 'DS6001', 'https://example.org', 'Celestica', 'DS6001', 'Celestica DS6001',
                   'switch', {}, [('Celestica', 'brand')])
        self.c.finish('other', 1)

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def bundle(self, text=None):
        text = body() if text is None else text
        html=self.root/'page.html';html.write_text(text)
        data=dict(schema_version=1, url=n.URL, html=dict(path=html.name,sha256=digest(html.read_bytes())),
                  expected=n.product_cards(body()))
        path=self.root/'bundle.json';path.write_text(json.dumps(data));return path

    def test_migration_grouped_models_qualified_facts_and_refresh_isolation(self):
        for _ in range(2):
            result=n.import_bundle(self.c,self.bundle())
            self.assertEqual((result['product_cards'],result['model_assertions']), (4,5))
            self.assertEqual(result['ignored_headings'],['Related Resources'])
        self.assertIsNone(self.c.db.execute('SELECT id FROM sources WHERE id=?',(n.LEGACY_SOURCE,)).fetchone())
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM records WHERE source_id='other'").fetchone()[0],1)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],5)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0],0)
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual(self.c.search(target='Celestica',role='manufacturer'),[])
        facts=list(self.c.db.execute("SELECT model,value FROM device_facts WHERE name='description_scope'"))
        self.assertEqual({r['model'] for r in facts},{'DS4000','DS4001'})
        self.assertTrue(all('not allocated' in r['value'] for r in facts))
        self.assertEqual(self.c.db.execute("SELECT value FROM device_facts WHERE model='DS6001' AND name='form_factor'").fetchone()[0],'2 OU')
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE value LIKE '%Hyperscale marketing%'").fetchone()[0],0)

    def test_bad_snapshot_never_replaces_legacy_or_other_records(self):
        malformed=[body().replace('DS6000 is','Different is'),body().replace('DS4000/DS4001','DS4000/DS4000'),
                   body().replace('ES1500 - Newest Addition','ES1500 - arbitrary suffix'),
                   body().replace('href="/example"','href="https://example.org/foreign"'),
                   body().replace('DS6000 is a 3U','DS6000 is a 4U'),body()[:-6],
                   '<title>unsupported browser</title>',body()+tile('DS6000','DS6000 is a switch.')]
        for text in malformed:
            with self.assertRaises(ValueError):n.import_bundle(self.c,self.bundle(text))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],2)
            self.assertIsNotNone(self.c.db.execute('SELECT id FROM sources WHERE id=?',(n.LEGACY_SOURCE,)).fetchone())
        path=self.bundle();(self.root/'page.html').write_text('changed')
        with self.assertRaisesRegex(ValueError,'checksum'):n.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],2)

    def test_nesting_entity_text_and_same_named_foreign_source_are_checked(self):
        text=body().replace('64-port','64-port &amp; management')
        parsed=n.product_cards(text)
        self.assertIn('64-port & management',parsed['cards'][0]['description'])
        with self.assertRaises(ValueError):n.product_cards(body().replace('<div><div>','<div class="market-tile"><div>',1))
        self.c.db.execute('UPDATE sources SET url=? WHERE id=?',('https://example.org/foreign',n.LEGACY_SOURCE))
        with self.assertRaisesRegex(ValueError,'Legacy'):n.import_bundle(self.c,self.bundle())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],2)

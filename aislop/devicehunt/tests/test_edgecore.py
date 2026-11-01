import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.edgecore import import_bundle, import_notice, parse_product, product_urls, notice_dates, notice_urls, resolve_models, import_notices


def product(name='DCS501', extra=''):
    p = {'@type': 'Product', 'name': name+' - Edgecore Networks',
         'description': '32 x 100G ports', 'category': 'Data center',
         'additionalProperty': [{'name': 'pa_modelname', 'value': 'COMPATIBLE-OTHER'}]}
    return '<script type="application/ld+json">'+json.dumps({'@graph': [p]})+'</script>'+extra


class EdgecoreTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root/'catalog.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def snapshot(self, name, body, url=None):
        (self.root/name).write_text(body)
        row = {'path': name, 'sha256': hashlib.sha256(body.encode()).hexdigest()}
        if url:
            row['url'] = url
        return row

    def bundle(self):
        prefix = 'https://www.edge-core.com/'
        index = self.snapshot('index.xml', '<sitemapindex><sitemap><loc>'+prefix+'product-sitemap1.xml</loc></sitemap></sitemapindex>')
        smap = self.snapshot('map.xml', '<urlset><url><loc>'+prefix+'product/dcs501/</loc></url></urlset>', prefix+'product-sitemap1.xml')
        page = self.snapshot('dcs501.html', product(extra='<a href="doc">DCS501 (AS7712-32X) Datasheet</a>'), prefix+'product/dcs501/')
        return {'index': index, 'sitemaps': [smap], 'pages': [page], 'expected_pages': 1}

    def load(self, bundle):
        path = self.root/'bundle.json'
        path.write_text(json.dumps(bundle))
        return import_bundle(self.c, path)

    def test_sitemap_ignores_translations_and_image_locations(self):
        body = '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:image="http://www.google.com/schemas/sitemap-image/1.1"><url><loc>https://www.edge-core.com/product/dcs501/</loc><image:image><image:loc>https://example.org/a.png</image:loc></image:image></url><url><loc>https://www.edge-core.com/jp/product/dcs501/</loc></url></urlset>'
        self.assertEqual(product_urls(body), ['https://www.edge-core.com/product/dcs501/'])

    def test_alias_requires_explicit_document_title_mapping(self):
        body = product(extra='<h3>DCS501 <span>(AS7712-32X)</span> Datasheet</h3><a href="b">DCS500 (AS7816-64X) Datasheet</a>')
        name, p, aliases = parse_product(body)
        self.assertEqual(name, 'DCS501')
        self.assertEqual(aliases, ['AS7712-32X'])
        self.assertNotIn('COMPATIBLE-OTHER', aliases)
        self.assertEqual(parse_product(product('AS9516-32Dv2'))[0], 'AS9516-32Dv2')

    def test_incomplete_or_tampered_snapshot_preserves_prior_data(self):
        good = self.bundle()
        self.load(good)
        for change in [lambda b: b['pages'].clear(), lambda b: b['sitemaps'].clear(),
                       lambda b: b['pages'].append(b['pages'][0]),
                       lambda b: b['pages'][0].update(sha256='bad'),
                       lambda b: b['pages'][0].update(path='../outside.html')]:
            bad = copy.deepcopy(good); change(bad)
            with self.assertRaises(ValueError):
                self.load(bad)
            self.assertEqual(len(self.c.search('AS7712')), 1)
        self.load(good)
        self.assertEqual(len(self.c.search()), 1)
        self.assertEqual(self.c.search(role='manufacturer'), [])

    def test_no_product_schema_is_an_error_not_an_empty_refresh(self):
        with self.assertRaises(ValueError):
            parse_product('<h1>Service unavailable</h1>')

    def test_eol_replacements_are_not_aliases_and_suffixes_remain_distinct(self):
        path = self.root/'notice.html'
        path.write_text('<link rel="canonical" href="https://www.edge-core.com/eol-notice/fixture/">'
                        '<table><tr><th>EOL Equipment</th><th>Replacement</th></tr>'
                        '<tr><td>Wedge100BF-32X-O-48V-F</td><td>NEW100</td></tr>'
                        '<tr><td>Wedge100BF-32X-O-AC-F</td><td>Not Available</td></tr></table>')
        for _ in range(2):
            self.assertEqual(import_notice(self.c, path), 2)
        self.assertEqual(len(self.c.search()), 2)
        self.assertFalse(self.c.db.execute("SELECT 1 FROM identifiers WHERE value='NEW100'").fetchall())
        self.assertFalse(self.c.db.execute("SELECT 1 FROM devices WHERE model='NEW100'").fetchall())
        self.assertEqual(self.c.search(role='manufacturer'), [])

    def test_ambiguous_eol_family_is_rejected_before_source_replacement(self):
        path = self.root/'notice.html'
        path.write_text('<link rel="canonical" href="https://www.edge-core.com/eol-notice/fixture/">'
                        '<table><tr><th>EOL Model</th><th>Replacement Model</th></tr>'
                        '<tr><td>MODEL100 / MODEL200</td><td>MODEL300</td></tr></table>')
        with self.assertRaises(ValueError):
            import_notice(self.c, path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM sources').fetchone()[0], 0)

    def test_effective_date_ignores_stale_hidden_metadata(self):
        body = '<script>Effective January 1, 2025</script><p>Effective January 1, 2026: products are now EOL.</p>'
        self.assertIn(('notice_effective_date', '2026-01-01'), notice_dates(body))
        self.assertNotIn(('notice_effective_date', '2025-01-01'), notice_dates(body))
        self.assertIn(('notice_effective_date', '2024-10-16'), notice_dates('<p>Effective 16Oct , 2024:</p>'))
        self.assertIn(('notice_effective_date', '2017-11-11'), notice_dates('<p>Effective Nov 11 <sup>th</sup>, 2017</p>'))
        self.assertFalse(any('support' in k for k, _ in notice_dates(body)))

    def test_reviewed_groups_and_order_codes_preserve_identity(self):
        self.assertEqual(resolve_models('ECS4110-28T/52T/28P'), ['ECS4110-28T', 'ECS4110-52T', 'ECS4110-28P'])
        self.assertEqual(resolve_models('Module:EM4626H-XG10GSFP+ (for ECS4610-26T/50T)'), ['EM4626H-XG10GSFP+'])
        for literal in ['5600-52X-D2-AC-F', 'ES4548D UK', 'AS7716-24SC(ACO)', 'Minipack-AS8000-RAIL KIT']:
            self.assertEqual(resolve_models(literal), [literal])

    def test_notice_index_pagination_must_not_be_silently_ignored(self):
        with self.assertRaises(ValueError):
            notice_urls('<a href="https://www.edge-core.com/eol-notice/a/">a</a><a href="/category/eol-notice/page/2/">next</a>')

    def test_notice_bundle_is_checked_before_any_source_replacement(self):
        url = 'https://www.edge-core.com/eol-notice/fixture/'
        index = self.snapshot('index.html', '<a href="'+url+'">notice</a>')
        notice = self.snapshot('notice.html', '<link rel="canonical" href="'+url+'"><table><tr><th>EOS Equipment</th><th>Part Number</th></tr><tr><td>5600-52X-O-AC-F</td><td>ABC123</td></tr></table>', url)
        bundle = {'index': index, 'pages': [notice], 'expected_pages': 1}
        path = self.root/'notices.json'
        path.write_text(json.dumps(bundle))
        self.assertEqual(import_notices(self.c, path)['affected_model_assertions'], 1)
        self.assertEqual(len(self.c.search('ABC123')), 1)
        for change in [lambda b: b['pages'].clear(), lambda b: b['pages'][0].update(sha256='bad')]:
            bad = copy.deepcopy(bundle); change(bad); path.write_text(json.dumps(bad))
            with self.assertRaises(ValueError):
                import_notices(self.c, path)
            self.assertEqual(len(self.c.search('ABC123')), 1)


if __name__ == '__main__':
    unittest.main()

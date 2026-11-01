import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import fcc_sitemaps as f, lookup, fcc_index
from devicehunt.catalog import Catalog, digest


class SitemapTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def save(self, ids=('HEDTEST', 'HEDTEST')):
        def capture(name, body):
            p = self.root / name
            p.write_text(body)
            return dict(path=name, sha256=digest(p.read_bytes()))
        urls = ['https://fccid.co/sitemaps/applications-%s.xml' % y for y in (2026, 2025, 1980)]
        index = capture('index.xml', '<sitemapindex xmlns="' + f.NS[1:-1] + '">' +
                        ''.join('<sitemap><loc>' + u + '</loc></sitemap>' for u in urls) + '</sitemapindex>')
        page = capture('page.xml', '<urlset xmlns="' + f.NS[1:-1] + '">' +
                       ''.join('<url><loc>https://fccid.co/fcc/' + x + '</loc><lastmod>2026-09-26</lastmod></url>' for x in ids) + '</urlset>')
        empty = capture('empty.xml', '<urlset xmlns="' + f.NS[1:-1] + '"/>')
        data = dict(schema_version=1, source_url=f.URL, index=index, children=[
            dict(url=urls[0], status='captured', page=page, row_count=len(ids)),
            dict(url=urls[1], status='captured', page=empty, row_count=0),
            dict(url=urls[2], status='unavailable', issue='HTTP 400')])
        p = self.root / 'bundle.json'
        p.write_text(json.dumps(data))
        return p

    def snapshot(self):
        return {t: [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + t)]
                for t in ('sources', 'fcc_sitemap_pages', 'fcc_sitemap_urls')}

    def test_duplicate_raw_ids_empty_vs_unavailable_and_refresh(self):
        for _ in range(2):
            self.assertEqual(f.import_bundle(self.c, self.save()),
                             dict(sitemaps=3, unavailable=1, url_rows=2, catalog_assertions=0))
        db = self.c.db
        self.assertEqual([r[0] for r in db.execute('SELECT row_count FROM fcc_sitemap_pages ORDER BY sitemap_url')], [None, 0, 2])
        self.assertEqual(db.execute('SELECT lastmod_raw FROM fcc_sitemap_urls LIMIT 1').fetchone()[0], '2026-09-26')
        for table in ('devices', 'records', 'fcc_applications', 'fcc_id_index', 'hyperscaler_evidence'):
            self.assertEqual(db.execute('SELECT count(*) FROM ' + table).fetchone()[0], 0)
        self.assertEqual(fcc_index.detail_index_rows(db), [])
        section = next(r for r in lookup.run(db, 'HEDTEST')['datasets'] if r['dataset'] == 'fcc_sitemaps')
        self.assertEqual(section['matches'], 2)
        f.import_bundle(self.c, self.save(('HEDNEW',)))
        self.assertEqual([r[0] for r in db.execute('SELECT fcc_id FROM fcc_sitemap_urls')], ['HEDNEW'])

    def test_bad_bundle_or_hash_preserves_prior_data(self):
        f.import_bundle(self.c, self.save()); before = self.snapshot()
        for change in (lambda d: d['children'].pop(),
                       lambda d: d['children'][0].update(row_count=99),
                       lambda d: d['children'][2].update(row_count=0),
                       lambda d: d['children'][0]['page'].update(sha256='0' * 64)):
            p = self.save(); data = json.loads(p.read_text()); change(data); p.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                f.import_bundle(self.c, p)
            self.assertEqual(self.snapshot(), before)

    def test_dependency_change_during_import_rolls_back_source_and_rows(self):
        f.import_bundle(self.c, self.save()); before = self.snapshot()
        p = self.save(('HEDNEW',)); original = f.dependency; seen = 0
        def changed(*args):
            nonlocal seen
            seen += 1
            if seen == 4:  # Index and both captures validated; replacement has begun.
                raise ValueError('Capture changed')
            return original(*args)
        with patch.object(f, 'dependency', side_effect=changed), self.assertRaises(ValueError):
            f.import_bundle(self.c, p)
        self.assertEqual(self.snapshot(), before)

    def test_invalid_xml_hosts_and_duplicate_index_entries(self):
        for body in ('<urlset/>', '<!DOCTYPE urlset><urlset/>',
                     '<urlset xmlns="' + f.NS[1:-1] + '"><url><loc>https://example.org/fcc/HEDTEST</loc></url></urlset>',
                     '<urlset xmlns="' + f.NS[1:-1] + '"><url><loc>https://fccid.co/fcc/HEDTEST</loc><loc>https://fccid.co/fcc/HEDNEW</loc></url></urlset>'):
            with self.assertRaises(ValueError): f.parse_urls(body.encode())
        entry = '<sitemap><loc>https://fccid.co/sitemaps/main.xml</loc></sitemap>'
        with self.assertRaises(ValueError):
            f.parse_index(('<sitemapindex xmlns="' + f.NS[1:-1] + '">' + entry * 2 + '</sitemapindex>').encode())

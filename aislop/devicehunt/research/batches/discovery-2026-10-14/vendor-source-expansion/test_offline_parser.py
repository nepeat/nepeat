"""Standalone fixture tests; no network, project DB or production writes."""
import copy
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from offline_parser import Document, sitemap_rows, endpoint, extract, build, sha


HTML = '''<h1>Switches</h1><div class="well c2a3"><div class="media-body">
<h3 class="media-heading">DCS203 (AS7326-56X) DS R10</h3>
<div class="text-muted text-small">Datasheet&nbsp; <i></i>3 MB</div></div><div>
<a href="/download/dcs203-as7326-56x-ds-r09/?wpdmdl=997&amp;refresh=a">Down<b>load</b></a>
<a href="/download/dcs203-as7326-56x-ds-r09/?wpdmdl=997&amp;refresh=b">Download</a>
</div></div><a href="https://elsewhere.example/download/x/">elsewhere</a>
<a href="/product/no/">navigation</a>'''


class ParserTests(unittest.TestCase):
    def test_card_label_exact_revision_metadata_and_raw_query(self):
        rows = Document(HTML).links('https://www.edge-core.com/product/x/')
        self.assertEqual(rows[0]['printed_label_raw'], 'Download')
        self.assertEqual(rows[0]['document_heading'], 'DCS203 (AS7326-56X) DS R10')
        self.assertEqual(rows[0]['document_metadata_raw'], 'Datasheet\xa0 3 MB')
        self.assertIn('&refresh=a', rows[0]['href_raw'])
        self.assertGreater(rows[0]['source_line'], 0)
        self.assertNotIn('document_heading', rows[2])

    def test_endpoint_is_comparison_only(self):
        a, b = 'https://www.edge-core.com/download/r/?wpdmdl=1&refresh=a', 'https://www.edge-core.com/download/r/?wpdmdl=1&refresh=b'
        self.assertEqual(endpoint(a), endpoint(b))
        self.assertNotEqual(a, b)
        self.assertNotEqual(endpoint(a), endpoint('http://www.edge-core.com/download/r/'))

    def test_sitemap_duplicates_lastmod_and_html_failure(self):
        rows = sitemap_rows(b'<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>https://example.test/a</loc><lastmod>2026-01-02</lastmod></url><url><loc>https://example.test/a</loc></url></urlset>')
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[0]['lastmod_raw'], '2026-01-02')
        self.assertEqual(rows[1]['row_number'], 2)
        with self.assertRaises(ValueError):
            sitemap_rows(b'<html><body>404</body></html>')

    def fixture(self, root):
        (root / 'page.html').write_text(HTML)
        db = sqlite3.connect(root / 'catalog.sqlite')
        db.executescript('CREATE TABLE sources(url TEXT); CREATE TABLE records(id TEXT, device_id TEXT,source_url TEXT); CREATE TABLE devices(id TEXT,brand TEXT,model TEXT); CREATE TABLE identifiers(record_id TEXT,kind TEXT,value TEXT);')
        db.execute('INSERT INTO devices VALUES(?,?,?)', ('full', 'Printed Brand', 'AS7326-56X'))
        db.execute('INSERT INTO devices VALUES(?,?,?)', ('family', 'Other Brand', 'AS7326'))
        db.execute('INSERT INTO sources VALUES(?)', ('https://www.edge-core.com/download/dcs203-as7326-56x-ds-r09/?wpdmdl=997&refresh=old',))
        db.commit()
        db.close()
        return dict(catalog_path='catalog.sqlite', sources=[dict(id='edgecore_product_downloads', source_role='product_page_link_publisher',same_host=True,path_pattern=r'^/download/',inputs=[dict(path='page.html',sha256=sha(root/'page.html'),url='https://www.edge-core.com/product/x/',format='html',status=200)])],restrictions=[dict(pattern='dcs203.*r09',reason='prior missing revision')])

    def test_pipeline_preserves_rows_filters_nav_and_roles_and_avoids_family_merge(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            config = self.fixture(root)
            before = sha(root / 'catalog.sqlite')
            result = build(config, root)
            self.assertEqual(result['summary']['rows'], 2)
            self.assertEqual(result['summary']['distinct_query_free_endpoints'], 1)
            self.assertEqual(result['summary']['query_free_endpoints_absent_from_catalog'], 0)
            r = result['rows'][0]
            self.assertEqual(r['wpdmdl_values'], ['997'])
            self.assertEqual(r['revision_strings_as_printed'], ['R10'])
            self.assertEqual(r['url_revision_strings'], ['r09'])
            self.assertTrue(r['revision_label_url_disagreement'])
            self.assertEqual(r['complete_label_model_matches'], {})
            self.assertEqual([m['device_id'] for m in r['token_model_matches_leads_only']['AS7326-56X']], ['full'])
            self.assertFalse(r['manufacturer_assertion'])
            self.assertEqual(r['source_role'], 'product_page_link_publisher')
            self.assertEqual(r['capture_scheduling'], 'excluded_known_restriction_or_missing')
            self.assertEqual(sha(root / 'catalog.sqlite'), before)
            self.assertEqual(result, build(config, root))

    def test_hash_status_and_path_validation_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            config = self.fixture(root)
            for field, value in [('sha256', '0'*64), ('status', 404), ('path', '../outside.html')]:
                changed = copy.deepcopy(config)
                changed['sources'][0]['inputs'][0][field] = value
                with self.subTest(field=field), self.assertRaises(ValueError):
                    extract(changed, root)
            missing = copy.deepcopy(config)
            missing['catalog_path'] = 'missing.sqlite'
            with self.assertRaises(FileNotFoundError):
                build(missing, root)
            self.assertFalse((root/'missing.sqlite').exists())

    def test_complete_labels_include_suffixes_without_alias_expansion(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            config = self.fixture(root)
            (root/'page.html').write_text('<a href="/download/x/">AS7326-56X</a><a href="/download/y/">AS7326-56X-R2</a>')
            config['sources'][0]['inputs'][0]['sha256'] = sha(root/'page.html')
            rows = build(config, root)['rows']
            self.assertEqual(list(rows[0]['complete_label_model_matches']), ['AS7326-56X'])
            self.assertEqual(rows[1]['complete_label_model_matches'], {})


class IndexIntegrationTests(unittest.TestCase):
    def test_oracle_tooltip_rows_keep_language_sections_and_shared_urls(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            config = ParserTests().fixture(root)
            (root/'page.html').write_text('''<h3 class="Group">SPARCT3 and T4 Servers (Simplified Chinese)</h3><table id="table10"><tr><td class="linktitlecell"><a class="tooltip" href="https://www.edge-core.com/cd/E1/index.html">SPARC T4-1 Server Documentation</a><a class="itooltip" href="#">l</a></td></tr><tr><td class="linktitlecell"><a class="tooltip" href="https://www.edge-core.com/cd/E1/index.html">SPARC T4-2 Server Documentation</a></td></tr></table>''')
            source = config['sources'][0]
            source.update(id='oracle_older_sparc_indexes',path_pattern=r'^/cd/',required_anchor_class='tooltip',required_ancestor_class='linktitlecell')
            source['inputs'][0]['sha256']=sha(root/'page.html')
            rows = build(config,root)['rows']
            self.assertEqual(len(rows),2)
            self.assertEqual(len({r['url'] for r in rows}),1)
            self.assertEqual(rows[0]['section_label'],'SPARCT3 and T4 Servers (Simplified Chinese)')
            self.assertEqual(rows[0]['table_id'],'table10')
            self.assertNotEqual(rows[0]['printed_label'],rows[1]['printed_label'])

    def test_private_sql_index_search_and_no_replacement(self):
        from build_discovery_db import create, search
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            config=ParserTests().fixture(root)
            dataset=build(config,root)
            (root/'discovery.json').write_text(json.dumps(dataset))
            create(root/'discovery.json',root/'discovery.sqlite')
            before=sha(root/'discovery.sqlite')
            results=search(root/'discovery.sqlite','"AS7326-56X"')
            self.assertEqual(len(results),2)
            self.assertEqual(results[0]['source_role'],'product_page_link_publisher')
            self.assertTrue(results[0]['revision_disagreement'])
            self.assertEqual(sha(root/'discovery.sqlite'),before)
            with self.assertRaises(FileExistsError):
                create(root/'discovery.json',root/'discovery.sqlite')
            self.assertEqual(sha(root/'discovery.sqlite'),before)

if __name__ == '__main__':
    unittest.main()

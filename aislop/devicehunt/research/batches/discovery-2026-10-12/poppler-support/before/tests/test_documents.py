import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import documents, lookup
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog, digest


class DocumentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'catalog.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def bundle(self, text='TE1A001 radio antenna'):
        pdf = self.root / '2avpl_te1a001_testreport_4843964_abcdef.pdf'
        pdf.write_bytes(b'%PDF-1.4\nFixture only: not production evidence')
        index = self.root / 'index.html'
        index.write_text('<h1>2AVPL-TE1A001 IoT Device</h1><section><h3>Test Report</h3>'
                         '<a title="Open Fixture report" href="/api/files/s1/' + pdf.name + '">PDF</a></section>')
        side = self.root / 'pages.json'
        side.write_text(json.dumps({'pdf_sha256': digest(pdf.read_bytes()),
                                   'extraction_method': 'pypdf embedded text; no OCR',
                                   'pages': [{'page': 1, 'text': text}, {'page': 2, 'text': '  '}]}))
        def entry(p): return {'path': p.name, 'sha256': digest(p.read_bytes())}
        data = {'schema_version': 1, 'fcc_id': '2AVPL-TE1A001',
                'index_url': 'https://fccid.co/fcc/2AVPL-TE1A001', 'index': entry(index),
                'documents': {'4843964': {'pdf': entry(pdf), 'text': entry(side), 'page_count': 2}}}
        path = self.root / 'bundle.json'; path.write_text(json.dumps(data)); return path

    def test_page_search_and_refresh_preserve_blanks_without_catalog_evidence(self):
        result = documents.import_bundle(self.c, self.bundle())
        self.assertEqual(result, {'documents': 1, 'pages': 2, 'pages_with_text': 1})
        rows = documents.search(self.c.db, '', limit=-1)['rows']
        self.assertEqual(rows[1]['text_status'], 'no_embedded_text')
        self.assertTrue(Path(rows[0]['local_pdf']).is_file())
        self.assertEqual(documents.search(self.c.db, 'radio ant')['matches'], 1)
        self.assertEqual(documents.search(self.c.db, 'antenna', fcc_id='wrong')['matches'], 0)
        self.assertEqual(self.c.search(), [])
        documents.import_bundle(self.c, self.bundle('replacement text'))
        self.assertEqual(documents.search(self.c.db, 'antenna')['matches'], 0)
        self.assertEqual(documents.search(self.c.db, 'replacement')['matches'], 1)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM document_search').fetchone()[0], 2)

    def test_limits_literal_tokens_and_metadata_distinction(self):
        documents.import_bundle(self.c, self.bundle())
        for limit, shown in [(0, 0), (1, 1), (-1, 2)]:
            result = documents.search(self.c.db, limit=limit)
            self.assertEqual((result['matches'], result['shown']), (2, shown))
        for query in ['"', "' OR 1=1 --", 'Fixture']:
            self.assertEqual(documents.search(self.c.db, query)['matches'], 0)
        section = next(s for s in lookup.run(self.c.db, 'Fixture')['datasets'] if s['dataset'] == 'documents')
        self.assertEqual(section['matches'], 2)  # Generic lookup also searches document metadata.
        with self.assertRaises(ValueError): documents.search(self.c.db, limit=-2)

    def test_invalid_dependencies_leave_previous_snapshot_unchanged(self):
        path = self.bundle(); documents.import_bundle(self.c, path)
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM sources')]
        data = json.loads(path.read_text())
        data['documents']['4843964']['pdf']['sha256'] = '0' * 64
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'checksum'): documents.import_bundle(self.c, path)
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM sources')])
        self.assertEqual(documents.search(self.c.db, 'radio')['matches'], 1)
        data['index']['path'] = '../escape.html'; path.write_text(json.dumps(data))
        with self.assertRaises(ValueError): documents.import_bundle(self.c, path)

    def test_source_association_page_order_and_complete_index_required(self):
        path = self.bundle(); data = json.loads(path.read_text())
        side = self.root / 'pages.json'; capture = json.loads(side.read_text())
        for changes in [{'pdf_sha256': '0' * 64}, {'pages': [{'page': 2, 'text': 'wrong order'}]},
                        {'extraction_method': 'unreviewed OCR'}]:
            bad = dict(capture, **changes)
            with self.assertRaises(ValueError): documents.read_pages(json.dumps(bad), capture['pdf_sha256'])
        data['documents'] = {}; path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'all public'): documents.import_bundle(self.c, path)
        body = (self.root / 'index.html').read_bytes()
        with self.assertRaisesRegex(ValueError, 'identity'): documents.index_documents(body, 'HED-DIFFERENT')

    def test_readonly_cli_fts_sql_and_missing_database(self):
        documents.import_bundle(self.c, self.bundle())
        before = self.c.path.read_bytes()
        for command in [['documents', 'radio', '--format', 'json'],
                        ['sql', "SELECT text FROM document_search WHERE document_search MATCH 'radio'", '--format', 'json']]:
            with contextlib.redirect_stdout(io.StringIO()) as out:
                self.assertEqual(main(['--db', str(self.c.path)] + command), 0)
                self.assertIn('radio', out.getvalue())
        self.assertEqual(before, self.c.path.read_bytes())
        absent = self.root / 'absent.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertNotEqual(main(['--db', str(absent), 'documents']), 0)
        self.assertFalse(absent.exists())
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertNotEqual(main(['--db', str(self.c.path), 'sql', 'DELETE FROM document_search']), 0)
        self.assertEqual(before, self.c.path.read_bytes())

    def test_legacy_database_and_caller_transaction_preserved(self):
        db = sqlite3.connect(':memory:'); db.row_factory = sqlite3.Row
        self.assertEqual(documents.search(db)['state'], 'not_loaded')
        self.assertFalse(db.in_transaction); db.close()
        self.c.db.execute('BEGIN')
        self.assertEqual(documents.search(self.c.db)['state'], 'not_loaded')
        self.assertTrue(self.c.db.in_transaction)

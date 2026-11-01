import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import documents, lookup, reference_documents as refs
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog, digest


class ReferenceDocumentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def bundle(self, names=('one', 'two')):
        entries = []
        def dep(p): return {'path': p.name, 'sha256': digest(p.read_bytes())}
        for name in names:
            pdf = self.root / (name + '.pdf'); pdf.write_bytes(b'%PDF-1.4\nFixture only ' + name.encode())
            text = self.root / (name + '.json')
            text.write_text(json.dumps({'pdf_sha256': digest(pdf.read_bytes()),
                                       'extraction_method': 'pypdf embedded text; no OCR',
                                       'pages': [{'page': 1, 'text': 'AMHT-1 power supply ' + name},
                                                 {'page': 2, 'text': ''}]}))
            entries.append({'id': name, 'pdf': dep(pdf), 'text': dep(text), 'page_count': 2,
                            'url': 'https://vendor.example/' + pdf.name, 'title': name + ' appliance guide',
                            'tier': 'vendor', 'category': 'safety guide', 'scope': 'Test fixture scope.'})
        path = self.root / 'references.json'
        path.write_text(json.dumps({'schema_version': 1, 'collection': refs.COLLECTION, 'documents': entries}))
        return path

    def test_pages_search_without_identity_inference_and_readonly_cli(self):
        result = refs.import_bundle(self.c, self.bundle())
        self.assertEqual(result, {'documents': 2, 'pages': 4, 'pages_with_text': 2})
        self.assertEqual(self.c.search(), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 0)
        hits = documents.search(self.c.db, 'power', source='reference-pdf-two')
        self.assertEqual((hits['state'], hits['matches']), ('loaded', 1))
        self.assertEqual(hits['rows'][0]['fcc_id'], '')
        self.assertEqual(hits['rows'][0]['page_number'], 1)
        self.assertTrue(Path(hits['rows'][0]['local_pdf']).is_file())
        self.assertEqual(documents.search(self.c.db, 'power', fcc_id='2AVPL-TE1A001')['matches'], 0)
        result = next(s for s in lookup.run(self.c.db, 'AMHT-1')['datasets'] if s['dataset'] == 'documents')
        self.assertEqual((result['state'], result['matches']), ('loaded', 2))
        self.assertEqual(len(result['sources']), 2)
        before = self.c.path.read_bytes()
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(main(['--db', str(self.c.path), 'documents', 'power', '--source',
                                   'reference-pdf-two', '--format', 'json']), 0)
        self.assertEqual(json.loads(out.getvalue())['matches'], 1)
        self.assertEqual(self.c.path.read_bytes(), before)

    def test_refresh_removes_stale_document_sources_and_keeps_other_collections(self):
        fixture = self.root / 'fcc.json'; fixture.write_text('{}')
        self.c.source('fcc-documents-existing', 'https://fcc.example', 'regulatory', '', '', fixture)
        self.c.db.execute('INSERT INTO documents VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
                          ('existing', 'fcc-documents-existing', 'TEST-FCC', '1234567', 'Existing', 'test',
                           'https://fcc.example/a.pdf', 'a.pdf', '0' * 64, 1, 1, 'pypdf embedded text; no OCR'))
        self.c.db.execute('INSERT INTO document_pages VALUES (?,?,?,?)', ('existing', 1, 'Existing antenna', 'embedded_text'))
        refs.import_bundle(self.c, self.bundle())
        refs.import_bundle(self.c, self.bundle(('two',)))
        self.assertEqual(documents.search(self.c.db, 'power')['matches'], 1)
        self.assertEqual(documents.search(self.c.db, 'antenna', fcc_id='TEST-FCC')['matches'], 1)
        self.assertIsNone(self.c.db.execute("SELECT 1 FROM sources WHERE id='reference-pdf-one'").fetchone())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM document_search').fetchone()[0], 3)
        self.assertEqual(self.c.db.execute('PRAGMA foreign_key_check').fetchall(), [])

    def test_refresh_order_does_not_change_fts_row_ids(self):
        refs.import_bundle(self.c, self.bundle())
        before = [tuple(r) for r in self.c.db.execute('SELECT rowid,* FROM document_search ORDER BY rowid')]
        refs.import_bundle(self.c, self.bundle(('two', 'one')))
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT rowid,* FROM document_search ORDER BY rowid')])

    def test_corrupt_last_dependency_does_not_remove_previous_collection(self):
        path = self.bundle(); refs.import_bundle(self.c, path)
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')]
        data = json.loads(path.read_text()); data['documents'][-1]['text']['sha256'] = '0' * 64
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'checksum'): refs.import_bundle(self.c, path)
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')])
        self.assertEqual(documents.search(self.c.db, 'power')['matches'], 2)

    def test_duplicate_wrong_scope_bad_metadata_and_path_escape_rejected(self):
        alterations = [lambda d: d['documents'].append(d['documents'][0]),
                       lambda d: d.update(collection='other'),
                       lambda d: d['documents'][0].update(url='javascript:alert(1)'),
                       lambda d: d['documents'][0].update(tier='seller'),
                       lambda d: d['documents'][0].update(page_count=True),
                       lambda d: d['documents'][0]['pdf'].update(path='../outside.pdf'),
                       lambda d: d['documents'][0].update(title='')]
        for change in alterations:
            path = self.bundle(); data = json.loads(path.read_text()); change(data); path.write_text(json.dumps(data))
            with self.assertRaises(ValueError): refs.prepare(path)

    def test_pdf_text_binding_and_no_ocr_contract(self):
        path = self.bundle(); data = json.loads(path.read_text()); entry = data['documents'][0]
        text = self.root / entry['text']['path']; capture = json.loads(text.read_text())
        for key, value in [('pdf_sha256', '0' * 64), ('extraction_method', 'OCR')]:
            broken = dict(capture, **{key: value}); text.write_text(json.dumps(broken))
            entry['text']['sha256'] = digest(text.read_bytes()); path.write_text(json.dumps(data))
            with self.assertRaises(ValueError): refs.prepare(path)

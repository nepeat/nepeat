import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ntra, validation
from devicehunt.catalog import Catalog


class NtraTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.raw = self.root / 'snapshot'
        self.raw.write_text('synthetic positional table fixture')
        self.rows = [
            ['11954', 'SERVER', 'EDGE-CORE', 'DMC-12-48C-AS0', 'DMC-12-48R-AS0'],
            ['11963', 'SWITCH', 'EDGE-CORE', 'AS7772-32X', ''],
            ['11999', 'SERVER', 'UNRELATED', 'AS7772-32X', ''],
        ]

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def test_registry_brand_and_linked_model_are_distinct_evidence(self):
        for _ in range(2):
            result = ntra.save_rows(self.c, self.raw, [(189, [self.rows])], 760)
        self.assertEqual(result['imported'], 2)
        fixture = self.root / 'inventory.json'
        fixture.write_text(json.dumps({'schema_version': 1, 'devices': [
            {'identifier': 'DMC-12-48R-AS0', 'vendor': 'Edge-Core'},
            {'identifier': 'AS7772-32X', 'vendor': 'Edge-Core'}]}))
        result = validation.run(self.c.db, fixture)
        self.assertEqual([r['status'] for r in result['results']], ['gap', 'hit'])
        self.assertTrue(result['results'][0]['search_hits'])
        self.assertEqual(result['results'][1]['evidence'][0]['source_url'], ntra.URL + '#page=189')
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])

    def test_malformed_or_duplicate_rows_preserve_existing_data(self):
        ntra.save_rows(self.c, self.raw, [(189, [self.rows])], 760)
        for bad in [self.rows + [self.rows[0]], self.rows + [['12000', 'SWITCH', 'EDGE-CORE']],
                    self.rows + [['12000', 'SWITCH', 'EDGE-CORE', '', '']]]:
            with self.assertRaises(ValueError):
                ntra.save_rows(self.c, self.raw, [(189, [bad])], 760)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)

    def test_unreviewed_pdf_is_rejected_before_optional_dependency_loading(self):
        with self.assertRaisesRegex(ValueError, 'differs from the reviewed snapshot'):
            ntra.import_pdf(self.c, self.raw)


class NtraSavedTests(unittest.TestCase):
    def setUp(self):
        from unittest.mock import patch
        from devicehunt import ntra_register
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        self.c=Catalog(self.root/'catalog.sqlite')
        self.addCleanup(self.c.db.close)
        self.pdf=self.root/'original.pdf'
        self.pdf.write_bytes(b'%PDF-synthetic original fixture')
        self.pdfsha=__import__('hashlib').sha256(self.pdf.read_bytes()).hexdigest()
        self.patch_pdf=patch.object(ntra,'PDF_SHA256',self.pdfsha)
        self.patch_pdf.start();self.addCleanup(self.patch_pdf.stop)
        self.patch_pages=patch.object(ntra_register,'PAGE_COUNT',2)
        self.patch_pages.start();self.addCleanup(self.patch_pages.stop)
        self.tables={'pages':[{'page':1,'tables':[[ntra_register.LABELS,['1','SWITCH','EDGE-CORE','AS7772-32X','linked']]]},
                              {'page':2,'tables':[[['2','PHONE','OTHER','PHONE-X','']]]}]}
        self.text={'pdf_sha256':self.pdfsha,'extraction_method':'pypdf embedded text; no OCR',
                   'pages':[{'page':1,'text':'Type Approved Equipment List EDGE-CORE'},
                            {'page':2,'text':'Type Approved Equipment List OTHER'}]}
        self.bundle=self.root/'bundle.json'
        self.write_bundle()

    def write_bundle(self):
        from devicehunt import ntra_register
        import hashlib
        data={'schema_version':1,'source_url':ntra.URL,'capture_format':'pdfplumber_positional_tables'}
        for key,name,raw in [('pdf','original.pdf',self.pdf.read_bytes()),('tables','tables.json',json.dumps(self.tables).encode()),('text','text.json',json.dumps(self.text).encode())]:
            (self.root/name).write_bytes(raw)
            data[key]={'path':name,'sha256':hashlib.sha256(raw).hexdigest()}
        data['expected']=ntra_register.summary(ntra_register.parse_pages(self.tables,expected_pages=2))
        self.bundle.write_text(json.dumps(data))
        return data

    def seed(self):
        ntra.save_rows(self.c,self.pdf,[(1,[[['1','SWITCH','EDGE-CORE','AS7772-32X','linked']]])],2)
        self.before=list(self.c.db.execute('SELECT source_key,raw_json FROM records'))

    def assert_preserved(self):
        self.assertEqual(list(self.c.db.execute('SELECT source_key,raw_json FROM records')),self.before)

    def test_saved_import_uses_original_pdf_metadata_and_no_optional_packages(self):
        self.seed()
        from unittest.mock import patch
        import builtins
        original=builtins.__import__
        def forbidden(name,*args,**kwargs):
            if name in ('pypdf','pdfplumber'):raise AssertionError('optional dependency forbidden')
            return original(name,*args,**kwargs)
        with patch('builtins.__import__',side_effect=forbidden):
            result=ntra.import_saved(self.c,self.bundle)
        self.assertEqual(result,{'pages_screened':2,'candidate_pages':1,'imported':1})
        self.assert_preserved()
        source=self.c.db.execute('SELECT sha256,cache_path FROM sources WHERE id=?',(ntra.SOURCE,)).fetchone()
        self.assertEqual(source['sha256'],self.pdfsha)
        self.assertEqual(source['cache_path'],str(self.pdf))

    def test_whole_noncandidate_table_malformed_preserves_source(self):
        self.seed()
        self.tables['pages'][1]['tables'][0][0]=['2','broken']
        # Rehash malformed bytes: shape validation must still reject them.
        data=json.loads(self.bundle.read_text())
        body=json.dumps(self.tables).encode();(self.root/'tables.json').write_bytes(body)
        data['tables']['sha256']=__import__('hashlib').sha256(body).hexdigest()
        self.bundle.write_text(json.dumps(data))
        with self.assertRaises(ValueError):ntra.import_saved(self.c,self.bundle)
        self.assert_preserved()

    def test_wrong_hash_missing_symlink_and_traversal_preserve(self):
        self.seed()
        for key in ('pdf','tables','text'):
            data=self.write_bundle();data[key]['sha256']='0'*64;self.bundle.write_text(json.dumps(data))
            with self.assertRaises(ValueError):ntra.import_saved(self.c,self.bundle)
            self.assert_preserved()
        self.write_bundle();(self.root/'text.json').unlink()
        with self.assertRaises(FileNotFoundError):ntra.import_saved(self.c,self.bundle)
        self.assert_preserved()
        data=self.write_bundle();(self.root/'text.json').unlink();(self.root/'text.json').symlink_to(self.pdf)
        with self.assertRaises(ValueError):ntra.import_saved(self.c,self.bundle)
        self.assert_preserved();(self.root/'text.json').unlink()
        data=self.write_bundle();data['tables']['path']='../tables.json';self.bundle.write_text(json.dumps(data))
        with self.assertRaises(ValueError):ntra.import_saved(self.c,self.bundle)
        self.assert_preserved()

    def test_incomplete_text_wrong_pdf_and_screen_gap_preserve(self):
        self.seed()
        for mutate in (lambda:self.text['pages'].pop(),lambda:self.text.update(pdf_sha256='0'*64),lambda:self.text['pages'][0].update(text='Type Approved Equipment List OTHER')):
            saved=json.loads(json.dumps(self.text));mutate();self.write_bundle()
            with self.assertRaises(ValueError):ntra.import_saved(self.c,self.bundle)
            self.assert_preserved();self.text=saved


if __name__ == '__main__':
    unittest.main()

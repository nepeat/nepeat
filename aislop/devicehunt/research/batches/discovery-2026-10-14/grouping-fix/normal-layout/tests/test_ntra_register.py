import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import ntra_register as ntra, lookup
from devicehunt.catalog import Catalog, digest


class NtraRegisterTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'catalog.sqlite')
        self.pdf = self.root / 'register.pdf'; self.pdf.write_bytes(b'synthetic PDF fixture')
        self.pages = {'pages': [
            {'page': 1, 'tables': [[ntra.LABELS, ['1', 'SERVER', 'AWS', 'AtlantaB1', 'Atlanta15 / Atlanta13']]]},
            {'page': 2, 'tables': [[['2', 'SWITCH', 'Edge-Core', 'AS7772-32X', ''],
                                    ['3', 'DEVICE', '(cid:123)', 'Unknown_100%', None]]]}]}

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def bundle(self):
        tables = self.root / 'tables.json'; tables.write_text(json.dumps(self.pages))
        data = dict(schema_version=1, source_url=ntra.URL, capture_format='pdfplumber_positional_tables',
                    expected=ntra.summary(ntra.parse_pages(self.pages, expected_pages=2)))
        for key,p in [('pdf', self.pdf), ('tables', tables)]:
            data[key] = dict(path=p.name, sha256=digest(p.read_bytes()))
        p = self.root / 'bundle.json'; p.write_text(json.dumps(data)); return p

    def import_fixture(self, path):
        with patch.object(ntra, 'PDF_SHA256', digest(self.pdf.read_bytes())), patch.object(ntra, 'PAGE_COUNT', 2):
            return ntra.import_bundle(self.c, path)

    def test_all_brands_and_linked_cells_preserved_without_catalog_inference(self):
        for _ in range(2): result = self.import_fixture(self.bundle())
        self.assertEqual(result, {'pages': 2, 'rows': 3, 'identified': 2, 'review_required': 1,
                                  'sequence_is_contiguous': True})
        self.assertEqual(self.c.search(), [])
        aws = ntra.search(self.c.db, 'AWS')[0]
        self.assertEqual((aws['brand'], aws['model'], aws['linked_models']), ('AWS', 'AtlantaB1', 'Atlanta15 / Atlanta13'))
        review = ntra.search(self.c.db, 'Unknown_100%')[0]
        self.assertIsNone(review['model']); self.assertEqual(review['parse_status'], 'review_required')
        self.assertEqual(json.loads(review['cells_json'])[-1], None)
        found = next(s for s in lookup.run(self.c.db, 'Atlanta13')['datasets'] if s['dataset']=='ntra')
        self.assertEqual(found['matches'], 1)
        self.assertEqual(found['rows'][0]['model'], 'AtlantaB1')

    def test_page_header_shape_and_sequence_audit(self):
        for mutation in [lambda d: d['pages'].pop(),
                         lambda d: d['pages'][1].update(page=1),
                         lambda d: d['pages'][0]['tables'][0][0].__setitem__(2, 'wrong column'),
                         lambda d: d['pages'][1]['tables'][0][0].pop()]:
            data = copy.deepcopy(self.pages); mutation(data)
            with self.assertRaises(ValueError): ntra.parse_pages(data, expected_pages=2)
        data=copy.deepcopy(self.pages); data['pages'][1]['tables'][0][0][0]='1'
        self.assertFalse(ntra.summary(ntra.parse_pages(data, expected_pages=2))['sequence_is_contiguous'])

    def test_checksum_counts_and_dependency_paths_preserve_existing_rows(self):
        self.import_fixture(self.bundle())
        for field,value in [('sha256','0'*64), ('path','../outside.pdf')]:
            p=self.bundle(); data=json.loads(p.read_text()); data['pdf'][field]=value;p.write_text(json.dumps(data))
            with self.assertRaises(ValueError): self.import_fixture(p)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ntra_equipment').fetchone()[0],3)
        p=self.bundle();data=json.loads(p.read_text());data['expected']['rows']=4;p.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'counts'): self.import_fixture(p)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ntra_equipment').fetchone()[0],3)

    def test_unicode_literal_search_and_readonly_old_schema_lookup(self):
        self.pages['pages'][1]['tables'][0][1][3]='Straße_100%'
        self.import_fixture(self.bundle())
        self.assertEqual(len(ntra.search(self.c.db,'STRASSE_100%')),1)
        self.assertEqual(ntra.search(self.c.db,'null'),[])
        self.c.db.execute('DROP TABLE ntra_equipment'); self.c.db.commit()
        result=lookup.run(self.c.db,'AWS')
        section=next(s for s in result['datasets'] if s['dataset']=='ntra')
        self.assertEqual(section['state'],'not_loaded')
        self.assertFalse(self.c.db.execute("SELECT 1 FROM sqlite_master WHERE name='ntra_equipment'").fetchone())

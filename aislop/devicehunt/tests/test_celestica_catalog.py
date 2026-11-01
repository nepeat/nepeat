import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import celestica_catalog as cat
from devicehunt.catalog import Catalog, digest


def pages():
    result = [{'page': n, 'text': ''} for n in range(1, 9)]
    result[0]['text'] = 'Celestica Platform Solutions'
    result[7]['text'] = 'Copyright Celestica Inc. 2025'
    result[3]['text'] = '\n'.join(
        heading + '\n1U 24 / 48-port 1GbE\nAccess Switch'
        for heading in ['ES1000 / ES1050', 'DS4000 / DS4001'] + ['DS' + str(5000+n) for n in range(8)])
    result[4]['text'] = '\n'.join(
        heading + '\n' + desc for heading, desc in [
            ('SC6100', '2U 24 SSD PCIe Gen 5\nStorage Controller'),
            ('SC4200', '2U 24 SSD NVMe PCIe Gen 4\nStorage Controller Controller'),
            ('SD6200', '4U 90 HDD SAS-4\nExpansion System'),
            ('SD4100', '2U 12 HDD SAS-4\nExpansion System'),
            ('SE4200', '2U 24 SSD NVMe\nExpansion System'),
            ('EG1050', '1U 48-port\nGateway with Wi-Fi and 5G'),
            ('DS1000', '1U 48-port 1GbE\nData Center Switch')])
    return result


class CelesticaCatalogTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root/'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def bundle(self):
        pdf=self.root/'catalog.pdf'; pdf.write_bytes(b'%PDF-1.4\nTest only')
        side=self.root/'text.json'
        side.write_text(json.dumps({'pdf_sha256':digest(pdf.read_bytes()),
                                   'extraction_method':'pypdf embedded text; no OCR','pages':pages()}))
        def entry(p): return {'path':p.name,'sha256':digest(p.read_bytes())}
        p=self.root/'bundle.json'; p.write_text(json.dumps({'schema_version':1,'url':cat.URL,'pdf':entry(pdf),'text':entry(side)}))
        return p

    def test_grouped_models_stay_separate_and_descriptions_qualified(self):
        for _ in range(2):
            self.assertEqual(cat.import_bundle(self.c,self.bundle()),{'product_cards':17,'model_assertions':19,'pages':8})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],19)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers').fetchone()[0],0)
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual(self.c.search(target='Celestica',role='manufacturer'),[])
        rows=[tuple(r) for r in self.c.db.execute("SELECT model,value FROM device_facts WHERE name='description_scope'")]
        self.assertEqual({r[0] for r in rows},{'ES1000','ES1050','DS4000','DS4001'})
        self.assertTrue(all('not allocated' in r[1] for r in rows))
        self.assertEqual(self.c.db.execute("SELECT value FROM device_facts WHERE model='SD6200' AND name='form_factor'").fetchone()[0],'4U')
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='extraction_note'").fetchone()[0],1)

    def test_malformed_missing_duplicate_or_wrong_category_card_rejected(self):
        for before,after in [('ES1000 / ES1050','ES1000 / ES1000'),
                             ('DS5000','DS1000'),
                             ('4U 90 HDD SAS-4','unknown format'),
                             ('SE4200','SC4201'),
                             ('DS5007\n1U 24 / 48-port 1GbE\nAccess Switch','')]:
            bad=pages()
            for p in bad: p['text']=p['text'].replace(before,after)
            with self.assertRaises(ValueError): cat.product_cards(bad)
        bad=pages(); bad[4]['text']='SE4200\n2U 24 SSD'
        with self.assertRaises(ValueError): cat.product_cards(bad)
        bad=pages(); bad[7]['text']='Copyright Someone 2026'
        with self.assertRaises(ValueError): cat.product_cards(bad)

    def test_failed_refresh_preserves_source_and_records(self):
        p=self.bundle(); cat.import_bundle(self.c,p)
        old=[tuple(r) for r in self.c.db.execute('SELECT * FROM sources')]
        b=json.loads(p.read_text()); b['text']['sha256']='0'*64; p.write_text(json.dumps(b))
        with self.assertRaisesRegex(ValueError,'checksum'): cat.import_bundle(self.c,p)
        self.assertEqual(old,[tuple(r) for r in self.c.db.execute('SELECT * FROM sources')])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],19)
        b['pdf']['path']='../outside.pdf'; p.write_text(json.dumps(b))
        with self.assertRaises(ValueError): cat.import_bundle(self.c,p)

    def test_swapped_pdf_text_capture_rejected(self):
        p=self.bundle(); b=json.loads(p.read_text()); side=self.root/b['text']['path']
        d=json.loads(side.read_text()); d['pdf_sha256']='0'*64; side.write_text(json.dumps(d))
        b['text']['sha256']=digest(side.read_bytes()); p.write_text(json.dumps(b))
        with self.assertRaisesRegex(ValueError,'association'): cat.import_bundle(self.c,p)

import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_reviewed_index as f, lookup
from devicehunt.catalog import Catalog, digest


class ReviewedFCCIndexTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        self.c=Catalog(self.root/'db.sqlite')
        self.capture=dict(schema_version=1,grantee_code='MRF',grantee_name='Celestica Incorporated',
            source_url='https://fccid.io/MRF',capture_method=f.METHOD,reviewed_on='2026-09-26',
            coverage='selected visible table rows; historical completeness unverified',reviewed_rows=2,
            table_lines=['MRF13316C2','1997-09-22 Wireless LAN Card Maximum Output Power: 153.5 mW',
                         'Class II Permissive Change','MRF13316C','1997-05-16 WLAN PCMCIA Card Maximum output power: 58 mW',
                         'Class II Permissive Change'])

    def tearDown(self):
        self.c.db.close();self.temp.cleanup()

    def save(self,capture=None):
        p=self.root/'capture.json';p.write_text(json.dumps(self.capture if capture is None else capture))
        b=self.root/'bundle.json';b.write_text(json.dumps({'schema_version':1,'captures':[{'path':p.name,'sha256':digest(p.read_bytes())}]}));return b

    def test_mirror_disagreement_and_missing_class_do_not_create_models_or_grants(self):
        self.c.source('fcc-index-mrf','https://fccid.co/company/MRF','regulatory_mirror','fixture','not found in this mirror')
        self.c.db.commit()
        self.assertEqual(f.import_bundle(self.c,self.save()),{'sources':1,'index_rows':2})
        self.assertEqual(self.c.db.execute("SELECT records_seen FROM sources WHERE id='fcc-index-mrf'").fetchone()[0],0)
        for table in ['devices','records','fcc_applications','fcc_exhibits']:
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM '+table).fetchone()[0],0)
        rows=list(self.c.db.execute('SELECT * FROM fcc_id_index ORDER BY fcc_id'))
        self.assertEqual(rows[0]['equipment_class'],'')
        self.assertEqual(rows[0]['listed_date'],'1997-05-16')
        self.assertEqual(rows[0]['source_url'],'https://fccid.io/MRF13316C')
        result=lookup.run(self.c.db,'Celestica')
        section=next(s for s in result['datasets'] if s['dataset']=='fcc_ids')
        self.assertEqual(section['matches'],2)
        self.assertEqual({s['id'] for s in section['sources']},{'fcc-index-mrf','fcc-index-reviewed-mrf'})
        self.assertTrue(any('not original HTML' in s['coverage'] for s in section['sources']))

    def test_bad_capture_shapes_dates_ids_counts_and_hashes_preserve_data(self):
        f.import_bundle(self.c,self.save())
        bad=[]
        for key,value in [('grantee_name','Another Company'),('reviewed_rows',True),('reviewed_rows',3),
                          ('reviewed_on','invalid'),('table_lines',[]),('source_url','https://example.org')]:
            d=copy.deepcopy(self.capture);d[key]=value;bad.append(d)
        for index,value in [(0,'HED13316C2'),(1,'1997-02-31 Wireless LAN Card'),(2,'bad\nline')]:
            d=copy.deepcopy(self.capture);d['table_lines'][index]=value;bad.append(d)
        d=copy.deepcopy(self.capture);d['table_lines'][3]=d['table_lines'][0];bad.append(d)
        for data in bad:
            with self.assertRaises(ValueError):f.import_bundle(self.c,self.save(data))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_id_index').fetchone()[0],2)
        p=self.save();(self.root/'capture.json').write_text('{}')
        with self.assertRaisesRegex(ValueError,'checksum'):f.import_bundle(self.c,p)

    def test_repeat_and_scoped_refresh_preserve_other_mirrors(self):
        p=self.save();f.import_bundle(self.c,p);f.import_bundle(self.c,p)
        self.c.source('other','https://example.org','regulatory_mirror','fixture','fixture')
        self.c.db.execute("INSERT INTO fcc_id_index SELECT 'other',fcc_id,grantee_code,grantee_name,listed_date,product_description,equipment_class,application_purpose,source_url,page_url FROM fcc_id_index WHERE fcc_id='MRF13316C'")
        self.c.db.commit()
        self.capture['table_lines']=self.capture['table_lines'][:3];self.capture['reviewed_rows']=1
        f.import_bundle(self.c,self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_id_index').fetchone()[0],2)
        self.assertEqual(self.c.db.execute("SELECT fcc_id FROM fcc_id_index WHERE source_id='other'").fetchone()[0],'MRF13316C')

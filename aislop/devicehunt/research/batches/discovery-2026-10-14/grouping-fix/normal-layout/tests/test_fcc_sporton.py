import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_sporton as f, vendor_inventory
from devicehunt.catalog import Catalog,digest
from test_fcc_details import html


def cover(model='EXAMPLE-1', manufacturer='Accton Technology Corporation', brand='Edge-Core'):
    return ("SPORTON International Inc. FCC RADIO TEST REPORT Applicant’s company Accton Technology Corporation "
            "Applicant Address One road FCC ID HEDTEST0 Manufacturer’s company "+manufacturer+
            " Manufacturer Address Two road Product Name Access point Brand Name "+brand+
            " Model Name "+model+" Test Rule 47 CFR FCC Part 15 Statement sample only")


class SportonCoverTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name);self.c=Catalog(self.root/'db.sqlite')

    def tearDown(self):
        self.c.db.close();self.tmp.cleanup()

    def capture(self,text=None):
        text=text or cover();p=self.root/'page.html';p.write_bytes(html(text=text))
        row={'fcc_id':'HEDTEST0','excerpt_ordinal':1,'excerpt_sha256':digest(text.encode()),
             'source_url':'https://fccid.co/fcc/HEDTEST0','fields':f.fields(text,'HEDTEST0'),
             'html':{'path':p.name,'sha256':digest(p.read_bytes())}}
        p=self.root/'capture.json';p.write_text(json.dumps({'schema_version':1,'capture_format':'reviewed_sporton_cover_roles','entries':[row]}))
        return p

    def test_roles_are_explicit_and_groups_are_not_brand_pairings_or_aliases(self):
        p=self.capture(cover(model='EXAMPLE-1, EXAMPLE-2',brand='Accton, Edge-Core'))
        for _ in range(2):
            self.assertEqual(f.import_capture(self.c,p),{'reviewed_covers':1,'model_assertions':2})
        self.assertEqual(len(vendor_inventory.run(self.c.db,'Accton')['rows']),2)
        self.assertEqual(vendor_inventory.run(self.c.db,'Edge-Core',scope='all')['rows'],[])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0],0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0],0)
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='mirror_excerpt_truncated' AND value='true'").fetchone()[0],2)

    def test_applicant_does_not_inherit_another_manufacturer(self):
        for manufacturer in ['Edimax Technology co., Ltd','Joy Technology(ShenZhen)Co.,Ltd.','Accton Networks Corporation','漢Accton Technology Corporation']:
            with self.subTest(manufacturer=manufacturer):
                f.import_capture(self.c,self.capture(cover(manufacturer=manufacturer)))
                self.assertEqual(vendor_inventory.run(self.c.db,scope='manufacturer')['rows'],[])
                self.assertEqual(len(vendor_inventory.run(self.c.db,'Accton',scope='all')['rows']),1)
                row=self.c.db.execute("SELECT company,target FROM affiliations WHERE role='manufacturer'").fetchone()
                self.assertEqual(row['company'],manufacturer);self.assertIsNone(row['target'])

    def test_ambiguous_models_wrong_identity_and_body_tables_rejected(self):
        for model in ['MODEL-1 / MODEL-2','MODEL-1 (Brand X)','MODEL-1, model-1','MODEL-1,','MODEL-1 2','Access Point 120 a/b/g']:
            self.assertIsNone(f.fields(cover(model=model),'HEDTEST0'))
        for text in ['x'*1800+cover(),cover()+cover(),cover().replace('HEDTEST0','HEDWRONG'),
                     'FCC ID HEDWRONG '+cover(),cover().replace('SPORTON International Inc.','Other lab'),
                     cover().replace('Applicant’s company Accton','Applicant’s company 漢Accton'),
                     cover().replace('FCC RADIO TEST REPORT','TEST EQUIPMENT LIST')]:
            self.assertIsNone(f.fields(text,'HEDTEST0'))
        self.assertIsNotNone(f.fields(cover().replace('’',"'").replace('Model Name','Model No.'),'HEDTEST0'))

    def test_changed_capture_or_hash_is_rejected_before_existing_source_changes(self):
        p=self.capture();f.import_capture(self.c,p)
        old=[tuple(r) for r in self.c.db.execute('SELECT * FROM sources')]
        for change in ['manufacturer','models','ordinal','excerpt_hash','html_hash','duplicate']:
            p=self.capture();d=json.loads(p.read_text());r=d['entries'][0]
            if change=='manufacturer':r['fields']['manufacturer']='Invented'
            elif change=='models':r['fields']['models']=['INVENTED-2']
            elif change=='ordinal':r['excerpt_ordinal']=2
            elif change=='excerpt_hash':r['excerpt_sha256']='0'*64
            elif change=='html_hash':r['html']['sha256']='0'*64
            else:d['entries']*=2
            p.write_text(json.dumps(d))
            with self.subTest(change=change),self.assertRaises(ValueError):f.import_capture(self.c,p)
            self.assertEqual(old,[tuple(r) for r in self.c.db.execute('SELECT * FROM sources')])

    def test_path_boundaries_and_refresh_remove_old_models(self):
        for name in ['../outside.html','/tmp/outside.html',None]:
            p=self.capture();d=json.loads(p.read_text());d['entries'][0]['html']['path']=name;p.write_text(json.dumps(d))
            with self.assertRaises(ValueError):f.import_capture(self.c,p)
        f.import_capture(self.c,self.capture(cover(model='OLD-1, OLD-2')))
        f.import_capture(self.c,self.capture(cover(model='NEW-1')))
        self.assertEqual([r['model'] for r in self.c.search()],['NEW-1'])

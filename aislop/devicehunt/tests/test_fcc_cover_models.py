import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_cover_models as f
from devicehunt.catalog import Catalog,digest
from test_fcc_details import html


def cover(models='EXAMPLE-1, EXAMPLE-2',fid='HEDTEST0',applicant='Accton Technology Corporation'):
    return ('FCC ID: '+fid+' FCC TEST REPORT REPORT NO.: RF123A MODEL NO.: '+models+
            ' RECEIVED: Jan. 1, 2001 TESTED: Jan. 2, 2001 APPLICANT: '+applicant+
            ' ADDRESS: Test address ISSUED BY: Test laboratory')


class ReportCoverTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name);self.c=Catalog(self.root/'db.sqlite')

    def tearDown(self):self.c.db.close();self.tmp.cleanup()

    def capture(self):
        t=cover(); p=self.root/'page.html';p.write_bytes(html(text=t))
        row=dict(f.cover_fields(t,'HEDTEST0'),fcc_id='HEDTEST0',excerpt_ordinal=1,
                 excerpt_sha256=digest(t.encode()),source_url='https://fccid.co/fcc/HEDTEST0',
                 html={'path':p.name,'sha256':digest(p.read_bytes())})
        p=self.root/'capture.json';p.write_text(json.dumps({'schema_version':1,'capture_format':'reviewed_fcc_cover_fields','entries':[row]}));return p

    def test_exact_model_list_and_applicant_only_preserve_scope(self):
        p=self.capture()
        for _ in range(2):self.assertEqual(f.import_capture(self.c,p),{'reviewed_report_covers':1,'model_assertions':2})
        self.assertEqual({r['model'] for r in self.c.search(target='Accton',role='regulatory_applicant')},{'EXAMPLE-1','EXAMPLE-2'})
        self.assertEqual(self.c.search(target='Accton',role='manufacturer'),[])
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0],0)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='mirror_excerpt_truncated' AND value='true'").fetchone()[0],2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0],0)

    def test_ambiguous_group_ocr_footnotes_and_peripheral_tables_excluded(self):
        for models in ['WN3302A 1','SMC2632W V.2','A300-2 (Brand: X)','A300-2, WG4005A 1-17',
                       'WN3201C OEM MODEL NO.: F5D6001','A300-2 A300-3','A300-2, A300-2']:
            self.assertIsNone(f.cover_fields(cover(models),'HEDTEST0'))
        self.assertIsNone(f.cover_fields('TEST PERIPHERALS MANUFACTURER: ACCTON MODEL NO.: EXAMPLE-1','HEDTEST0'))
        self.assertIsNone(f.cover_fields('x'*1800+cover(),'HEDTEST0'))
        self.assertIsNone(f.cover_fields(cover(fid='HEDWRONG'),'HEDTEST0'))
        self.assertIsNone(f.cover_fields(cover(applicant='Test Laboratory'),'HEDTEST0'))
        self.assertIsNone(f.cover_fields(cover()+cover(),'HEDTEST0'))

    def test_changed_fields_hash_association_and_duplicate_rejected_before_replace(self):
        p=self.capture();f.import_capture(self.c,p)
        old=[tuple(r) for r in self.c.db.execute('SELECT * FROM sources')]
        for field,value in [('models',['INVENTED-1']),('applicant','Different Company'),
                            ('excerpt_ordinal',2),('excerpt_sha256','0'*64)]:
            p=self.capture();d=json.loads(p.read_text());d['entries'][0][field]=value;p.write_text(json.dumps(d))
            with self.assertRaises(ValueError):f.import_capture(self.c,p)
            self.assertEqual(old,[tuple(r) for r in self.c.db.execute('SELECT * FROM sources')])
        p=self.capture();d=json.loads(p.read_text());d['entries']*=2;p.write_text(json.dumps(d))
        with self.assertRaisesRegex(ValueError,'Duplicate'):f.import_capture(self.c,p)

    def test_snapshot_hash_and_path_boundaries(self):
        p=self.capture();d=json.loads(p.read_text());d['entries'][0]['html']['sha256']='0'*64;p.write_text(json.dumps(d))
        with self.assertRaisesRegex(ValueError,'checksum'):f.import_capture(self.c,p)
        for name in ['../outside.html','/tmp/outside.html',None]:
            p=self.capture();d=json.loads(p.read_text());d['entries'][0]['html']['path']=name;p.write_text(json.dumps(d))
            with self.assertRaises(ValueError):f.import_capture(self.c,p)

import contextlib
import hashlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt import fcc_coverage
from devicehunt.__main__ import main


class FCCCoverageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name)/'db.sqlite'
        self.c = Catalog(self.path)
        for sid in ['catalog', 'mirror', 'mirror2', 'seller']:
            self.c.source(sid, 'https://example.org/'+sid,
                          'marketplace' if sid=='seller' else 'regulatory_mirror', '', '')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def claim(self, sid, key, model, ids=(), affiliations=()):
        self.c.add(sid,key,'https://example.org/'+key,'Brand',model,model,'device',{},affiliations,ids)
        self.c.db.commit()
        return self.c.db.execute('SELECT id FROM records WHERE source_id=? AND source_key=?',(sid,key)).fetchone()[0]

    def index(self, fid, sid='mirror', code='HED'):
        self.c.db.execute('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
            (sid,fid,code,'Printed company','printed date','description','','','https://example.org/id','https://example.org/index'))
        self.c.db.commit()

    def detail(self, fid, status):
        self.c.db.execute('INSERT INTO fcc_detail_pages VALUES (?,?,?,?,?,?,?,?,?)',
            ('mirror',fid,'HED','Printed company','','{}','https://example.org/id',status,
             'HTTP 403' if status=='unavailable' else ''))
        self.c.db.commit()

    def test_collection_stages_preserve_duplicates_failures_and_source_roles(self):
        rid=self.claim('catalog','a','ModelA',[('fcc_id','HED-A')],[('Accton','regulatory_applicant')])
        self.index('HED-A');self.index('HED-A','mirror2');self.detail('HED-A','unavailable')
        self.c.db.execute('INSERT INTO fcc_applications VALUES (?,?,?,?,?,?,?,?,?,?)',
            (rid,'HED-A','application1','Accton','','','','','','partial exhibits'))
        self.c.db.execute('INSERT INTO documents VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
            ('doc','mirror','HED-A','doc1','title','Test Report','https://example.org/doc','x.pdf','hash',3,2,'embedded text'))
        self.c.db.commit()
        result=fcc_coverage.run(self.c.db,missing='detail',limit=-1);r=result['rows'][0]
        self.assertEqual(result['summary']['known_fcc_ids'],1)
        self.assertEqual(r['counts'],dict(index_entries=2,detail_attempts=1,captured_detail_pages=0,
            application_records=1,pdf_documents=1,catalog_claims=1))
        self.assertEqual(r['detail'][0]['issue'],'HTTP 403')
        self.assertEqual(r['collection'],dict(index='collected',detail='not_collected',application='collected',pdf='collected'))
        self.assertEqual(r['catalog_claims'][0]['role'],'regulatory_applicant')
        self.assertEqual(fcc_coverage.run(self.c.db,missing='pdf')['matches'],0)
        self.c.db.execute("UPDATE fcc_detail_pages SET status='captured',issue=''");self.c.db.commit()
        self.assertEqual(fcc_coverage.run(self.c.db,missing='detail')['matches'],0)

    def test_vendor_is_same_record_and_never_guessed_from_identifier_prefix(self):
        self.claim('catalog','a','Shared',affiliations=[('Accton','manufacturer')])
        self.claim('catalog','b','Shared',[('fcc_id','HED-OTHER')],[('Oracle','brand')])
        self.claim('seller','c','S1',[('fcc_id','SELLER-ID')],[('Accton','seller_reported_brand')])
        self.claim('catalog','d','A1',[('related_fcc_id','HED-RELATED')],[('Accton','manufacturer')])
        self.index('MRF-ONE',code='MRF')
        accton=fcc_coverage.run(self.c.db,'Accton',limit=-1)
        self.assertEqual([r['fcc_id'] for r in accton['rows']],['SELLER-ID'])
        self.assertEqual(accton['rows'][0]['catalog_claims'][0]['tier'],'marketplace')
        self.assertEqual(fcc_coverage.run(self.c.db,'Oracle')['rows'][0]['fcc_id'],'HED-OTHER')
        self.assertEqual(fcc_coverage.run(self.c.db,'Celestica')['rows'][0]['fcc_id'],'MRF-ONE')

    def test_literal_ids_limits_and_independent_application_seed(self):
        self.claim('catalog','a','A',[('fcc_id',' hed-A ')],[('Accton','manufacturer')])
        self.index('HED-A');self.index('HEDA');self.index('HED-A-2')
        rid=self.claim('catalog','b','B',affiliations=[('Oracle','regulatory_applicant')])
        self.c.db.execute('INSERT INTO fcc_applications VALUES (?,?,?,?,?,?,?,?,?,?)',
            (rid,'A4H-ONLY','a2','Oracle','','','','','',''))
        self.c.db.commit()
        result=fcc_coverage.run(self.c.db,limit=-1)
        self.assertEqual(result['summary']['known_fcc_ids'],4)
        r=next(r for r in result['rows'] if r['fcc_id']=='HED-A')
        self.assertEqual(r['printed_ids'],[' hed-A ','HED-A'])
        self.assertEqual(r['counts']['catalog_claims'],1)
        self.assertEqual(fcc_coverage.run(self.c.db,limit=0)['shown'],0)
        self.assertTrue(fcc_coverage.run(self.c.db,limit=1)['truncated'])
        for kwargs in [dict(limit=True),dict(limit=-2),dict(vendor='unknown'),dict(missing='everything')]:
            with self.assertRaises(ValueError):fcc_coverage.run(self.c.db,**kwargs)

    def test_absent_datasets_are_unknown_and_caller_transaction_is_preserved(self):
        self.index('HED-A')
        self.c.db.execute('DROP TABLE documents');self.c.db.commit()
        result=fcc_coverage.run(self.c.db)
        self.assertEqual(result['rows'][0]['collection']['pdf'],'not_loaded')
        self.assertEqual(fcc_coverage.run(self.c.db,missing='pdf')['matches'],0)
        self.assertFalse(self.c.db.in_transaction)
        self.c.db.execute('BEGIN')
        self.c.db.execute("UPDATE fcc_id_index SET product_description='pending'")
        self.assertEqual(fcc_coverage.run(self.c.db)['rows'][0]['index'][0]['product_description'],'pending')
        self.assertTrue(self.c.db.in_transaction)
        self.c.db.rollback()
        with sqlite3.connect(':memory:') as db:
            db.row_factory=sqlite3.Row
            empty=fcc_coverage.run(db)
            self.assertEqual(empty['summary']['known_fcc_ids'],0)
            self.assertTrue(all(v=='not_loaded' for v in empty['datasets'].values()))

    def test_cli_readonly_json_and_csv(self):
        self.index('HED-A')
        before=hashlib.sha256(self.path.read_bytes()).hexdigest()
        output=io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(main(['--db',str(self.path),'fcc-coverage','--missing','pdf','--format','json']),0)
        self.assertEqual(json.loads(output.getvalue())['matches'],1)
        output=io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(main(['--db',str(self.path),'fcc-coverage','--format','csv']),0)
        self.assertIn('pdf_state',output.getvalue());self.assertIn('HED-A',output.getvalue())
        self.assertEqual(before,hashlib.sha256(self.path.read_bytes()).hexdigest())


if __name__=='__main__':unittest.main()

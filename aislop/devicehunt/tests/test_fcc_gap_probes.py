import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_gap_probes as f, fcc_coverage, lookup
from devicehunt.catalog import Catalog, digest


class FCCGapProbeTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.root=Path(self.temp.name)
        self.c=Catalog(self.root/'db.sqlite')
        self.body='<html><head><title>'+f.TITLE+'</title></head><body>Not found</body></html>'
        self.c.source('known','https://example.org','community','','')
        self.c.add('known','1','https://example.org','Accton','M1','M1','radio',{},
                   [('Accton','brand')],[('fcc_id','HEDTEST')])
        self.c.finish('known',1)

    def tearDown(self):
        self.c.db.close();self.temp.cleanup()

    def save(self, ids=('HEDTEST',), body=None):
        entries=[]
        for fid in ids:
            p=self.root/(fid+'.html');p.write_text(self.body if body is None else body)
            entries.append(dict(fcc_id=fid,url=f.BASE+'/fcc/'+fid,path=p.name,sha256=digest(p.read_bytes()),
                                state='not_found_in_mirror',html_title=f.TITLE))
        data=dict(schema_version=1,capture_method=f.METHOD,reviewed_on='2026-09-27',entries=entries)
        p=self.root/'review.json';p.write_text(json.dumps(data));return p

    def test_failure_is_searchable_but_never_a_collected_filing_or_company_claim(self):
        self.assertEqual(f.import_bundle(self.c,self.save()),dict(probes=1,captured_details=0,unavailable=1))
        r=dict(self.c.db.execute('SELECT * FROM fcc_detail_pages').fetchone())
        self.assertEqual((r['grantee_code'],r['grantee_name'],r['status']),('','','unavailable'))
        self.assertEqual(json.loads(r['fields_json'])['probe_state'],'not_found_in_mirror')
        for table in ['fcc_id_index','fcc_applications','fcc_exhibits','fcc_detail_links','fcc_text_excerpts']:
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM '+table).fetchone()[0],0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],1)
        r=fcc_coverage.run(self.c.db,missing='detail')['rows'][0]
        self.assertEqual(r['counts']['detail_attempts'],1)
        self.assertEqual(r['counts']['captured_detail_pages'],0)
        section=next(s for s in lookup.run(self.c.db,'HEDTEST')['datasets'] if s['dataset']=='fcc_details')
        self.assertEqual(section['matches'],1)
        self.assertIn(f.SOURCE,{s['id'] for s in section['sources']})
        for query in ['unavailable','FCC existence unverified']:
            section=next(s for s in lookup.run(self.c.db,query)['datasets'] if s['dataset']=='fcc_details')
            self.assertEqual(section['matches'],1)

    def test_refresh_is_scoped_and_does_not_replace_successful_mirror_capture(self):
        self.c.source('other','https://example.org','regulatory_mirror','','')
        self.c.db.execute('INSERT INTO fcc_detail_pages VALUES (?,?,?,?,?,?,?,?,?)',
            ('other','HEDTEST','HED','Accton Technology Corp','','{}','https://example.org','captured',''))
        self.c.db.commit()
        p=self.save(('HEDTEST','HEDTEST2'));f.import_bundle(self.c,p);f.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_detail_pages').fetchone()[0],3)
        f.import_bundle(self.c,self.save(('HEDTEST2',)))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_detail_pages').fetchone()[0],2)
        self.assertEqual(fcc_coverage.run(self.c.db,missing='detail')['matches'],0)

    def test_invalid_or_contradictory_evidence_preserves_prior_rows(self):
        f.import_bundle(self.c,self.save())
        for body in [self.body.replace(f.TITLE,'Access denied'),self.body+'<h1>HEDTEST Device</h1>',
                     self.body+'<h2>Application Details</h2>',self.body+'<a href="/api/files/a.pdf">PDF</a>']:
            with self.assertRaises(ValueError):f.import_bundle(self.c,self.save(body=body))
        p=self.save();data=json.loads(p.read_text())
        variants=[]
        for key,value in [('url','https://example.org'),('path','../HEDTEST.html'),('sha256','0'*64),
                          ('state','captured'),('fcc_id','HED/TEST')]:
            bad=copy.deepcopy(data);bad['entries'][0][key]=value;variants.append(bad)
        bad=copy.deepcopy(data);bad['entries']*=2;variants.append(bad)
        for bad in variants:
            p.write_text(json.dumps(bad))
            with self.assertRaises(ValueError):f.import_bundle(self.c,p)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_detail_pages').fetchone()[0],1)


if __name__=='__main__':unittest.main()

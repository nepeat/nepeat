import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_company as f, fcc_index, lookup
from devicehunt.catalog import Catalog, digest


class FCCCompanyTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.root=Path(self.tmp.name)
        self.c=Catalog(self.root/'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def save(self, alter=None):
        pages=[]
        for page in range(1,4):
            rows=[dict(web_text_line=n+30,listed_date='2001-01-01',fcc_id='HEDTEST',
                       model_cell='TEST',purpose='Original Equipment',frequency='')
                  for n in range(100 if page<3 else 2)]
            if page==3:
                rows[0].update(fcc_id='HED-TEST',frequency='5.18 - 5.32 MHz')
                rows[1].update(listed_date='2000-01-01',purpose='Class II Permissive Change')
            data=dict(schema_version=1,capture_method=f.METHOD,reviewed_on='2026-09-27',
                      company=f.COMPANY,company_frn='0005033246',page_url=f.page_url(page),
                      page=page,total_pages=3,reported_refresh='2026-07-29 21:35:46',rows=rows)
            pages.append(data)
        if alter:alter(pages)
        entries=[]
        for n,data in enumerate(pages,1):
            p=self.root/('page-'+str(n)+'.json');p.write_text(json.dumps(data))
            entries.append(dict(path=p.name,sha256=digest(p.read_bytes())))
        p=self.root/'bundle.json';p.write_text(json.dumps(dict(schema_version=1,source_url=f.URL,pages=entries,row_count=202)))
        return p

    def test_duplicate_rows_dates_hyphens_and_frequency_text_survive_without_models(self):
        for _ in range(2):
            self.assertEqual(f.import_bundle(self.c,self.save()),
                             dict(company_rows=202,distinct_fcc_ids=2,catalog_assertions=0))
        db=self.c.db
        self.assertEqual(db.execute('SELECT count(*) FROM devices').fetchone()[0],0)
        self.assertEqual(db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0],0)
        self.assertEqual(db.execute('SELECT count(*) FROM fcc_company_rows WHERE fcc_id=?',('HEDTEST',)).fetchone()[0],201)
        self.assertEqual({r[0] for r in db.execute('SELECT fcc_id FROM fcc_id_index')},{'HEDTEST','HED-TEST'})
        self.assertEqual(db.execute('SELECT frequency_raw FROM fcc_company_rows WHERE fcc_id=?',('HED-TEST',)).fetchone()[0],'5.18 - 5.32 MHz')
        self.assertEqual(db.execute('SELECT listed_date FROM fcc_id_index WHERE fcc_id=?',('HEDTEST',)).fetchone()[0],'2001-01-01')
        result=lookup.run(db,'HED-TEST')
        raw=next(s for s in result['datasets'] if s['dataset']=='fcc_company')
        self.assertEqual(raw['matches'],1)

    def test_invalid_capture_rejects_before_replacement(self):
        f.import_bundle(self.c,self.save());before=[tuple(r) for r in self.c.db.execute('SELECT * FROM fcc_company_rows')]
        for change in [lambda p:p.pop(),lambda p:p[0]['rows'].pop(),
                       lambda p:p[1].update(page=1),lambda p:p[1].update(company='Other'),
                       lambda p:p[2].update(capture_method='original HTML'),
                       lambda p:p[0]['rows'][0].update(fcc_id='OTHER-ID'),
                       lambda p:p[0]['rows'][1].update(web_text_line=30),
                       lambda p:p[2].update(reported_refresh='2026-01-01 00:00:00')]:
            with self.assertRaises(ValueError):f.import_bundle(self.c,self.save(change))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM fcc_company_rows')],before)
        p=self.save();(self.root/'page-1.json').write_text('changed')
        with self.assertRaises(ValueError):f.import_bundle(self.c,p)
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM fcc_company_rows')],before)

    def test_refresh_withdrawal_and_original_mirror_collector_isolation(self):
        self.c.source('fcc-index-hed','https://fccid.co/company/HED','regulatory_mirror','fixture','fixture')
        self.c.db.execute('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
                          ('fcc-index-hed','HEDOTHER','HED',f.COMPANY,'2000-01-01','Other','','Original Equipment','https://fccid.co/fcc/HEDOTHER','https://fccid.co/company/HED'))
        self.c.db.commit()
        f.import_bundle(self.c,self.save())
        self.assertEqual([r['fcc_id'] for r in fcc_index.detail_index_rows(self.c.db)],['HEDOTHER'])
        def change(p):p[2]['rows'][0]['fcc_id']='HEDNEW'
        f.import_bundle(self.c,self.save(change))
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT fcc_id FROM fcc_id_index')},{'HEDTEST','HEDNEW','HEDOTHER'})

    def amazon(self, **updates):
        data=dict(schema_version=1,capture_method=f.METHOD,reviewed_on='2026-09-27',
                  company='Amazon Data Services, Inc.',company_frn='0029198181',
                  page_url=f.AMAZON_URL,page=1,total_pages=1,reported_refresh='2026-07-29 21:23:42',
                  rows=[dict(web_text_line=38,listed_date='2022-01-27',fcc_id='2AVPL-CUBSCOUT',
                             model_cell='CUBSCOUT',purpose='Original Equipment',frequency='13.553 - 13.567 MHz')])
        data.update(updates)
        p=self.root/'amazon.json';p.write_text(json.dumps(data))
        b=self.root/'amazon-bundle.json';b.write_text(json.dumps(dict(schema_version=1,
            source_url=f.AMAZON_URL,pages=[dict(path=p.name,sha256=digest(p.read_bytes()))],row_count=1)))
        return b

    def test_amazon_separate_fcc_identity_and_source_scoped_refresh(self):
        f.import_bundle(self.c,self.save())
        for _ in range(2):
            self.assertEqual(f.import_bundle(self.c,self.amazon()),dict(company_rows=1,distinct_fcc_ids=1,catalog_assertions=0))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_company_rows').fetchone()[0],203)
        result=next(s for s in lookup.run(self.c.db,'2AVPL-CUBSCOUT')['datasets'] if s['dataset']=='fcc_company')
        self.assertEqual(result['matches'],1)
        source=next(s for s in result['sources'] if s['id']=='fcc-company-amazon')
        self.assertIn('download was not attempted',source['coverage'])
        self.assertNotIn('HTTP 403',source['coverage'])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],0)
        self.assertEqual(fcc_index.detail_index_rows(self.c.db),[])

    def test_amazon_uls_identity_and_other_company_url_are_rejected(self):
        f.import_bundle(self.c,self.amazon())
        before=[tuple(r) for r in self.c.db.execute('SELECT * FROM fcc_company_rows')]
        for updates in [dict(company_frn='0032960056'),dict(company_frn='0022171425'),
                        dict(page_url='https://fcc.report/company/amazon-data-services-inc-3'),
                        dict(total_pages=3)]:
            with self.assertRaises(ValueError):f.import_bundle(self.c,self.amazon(**updates))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM fcc_company_rows')],before)

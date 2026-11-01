import copy
import csv
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import energystar, energystar_datacenter as e, lookup
from devicehunt.catalog import Catalog, digest
from devicehunt.pipeline import import_source


class DatacenterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def dependencies(self, b, entries):
        for key, name in entries:
            b[key] = dict(path=name, sha256=digest((self.root/name).read_bytes()))
        p=self.root/'bundle.json';p.write_text(json.dumps(b));return p

    def current(self, kind='storage-file', change_rows=None, change_meta=None, change_after=None):
        dataset,name,id_field,identifier,certified,available=e.SPECS[kind]
        headers=[id_field,*e.IDENTITY,identifier,certified,available,
                 'Storage Controller Manufacturer Name','Stream Ready Idle Test (GB/W)']
        ids=['2,382,932','2,382,933'] if kind=='storage-file' else ['2382932','2382933']
        rows=[[ids[0],'Other Company','Other Brand','Other array','OTHER-1','ES-1','','','Oracle','999'],
              [ids[1],'Oracle Corporation','Oracle','Test array','TEST-ARRAY','ES-2','','','Other Company','123']]
        if change_rows:change_rows(rows)
        cols=[dict(name=h,fieldName='pd_id' if n==0 else h,dataTypeName='number' if n==0 else 'text') for n,h in enumerate(headers)]
        cols[0]['cachedContents']=dict(null='0',non_null=str(len(rows)),count=str(len(rows)))
        meta=dict(id=dataset,name=name,rowsUpdatedAt=123,columns=cols)
        if change_meta:change_meta(meta)
        after=copy.deepcopy(meta)
        if change_after:change_after(after)
        for f,d in [('before.json',meta),('after.json',after)]: (self.root/f).write_text(json.dumps(d))
        with (self.root/'rows.csv').open('w',newline='') as stream:csv.writer(stream).writerows([headers,*rows])
        b=dict(schema_version=1,dataset_kind=kind,dataset_url='https://data.energystar.gov/d/'+dataset,
               metadata_url='https://data.energystar.gov/api/views/'+dataset,
               csv_url='https://data.energystar.gov/api/v3/views/'+dataset+'/export.csv?accessType=DOWNLOAD')
        return self.dependencies(b,[('metadata_before','before.json'),('metadata_after','after.json'),('csv','rows.csv')])

    def archive(self, kind='storage-file-v1', change=None, change_bundle=None):
        url,width=e.ARCHIVES[kind]
        headers=['field-'+str(n) for n in range(width)]
        headers[:8]=['PD_ID',*e.IDENTITY,'ENERGY STAR Model Identifier','Date Qualified','Date Available On Market']
        values=['']*width;values[:8]=[123,'Other Company','Other Brand','Other array','OTHER-OLD','ES-OLD','2019-01-01','2018-01-01']
        sheet=dict(name='Storage QPL V1',rows=[['']*width for _ in range(3)]+[headers,values,['']*width],merged_ranges=[])
        original=b'fixture workbook';(self.root/'book.xlsx').write_bytes(original)
        data=dict(schema_version=1,workbook_sha256=digest(original),
                  extraction_method='openpyxl 3.1.5; stored cell values; formulas rejected',sheets=[sheet])
        b=dict(schema_version=1,dataset_kind=kind,url=url,index_url=e.INDEX,expected_rows=1,
               header_sha256=digest(json.dumps(headers,ensure_ascii=False).encode()))
        if change:change(data)
        (self.root/'cells.json').write_text(json.dumps(data))
        (self.root/'index.html').write_text('<a href="'+url+'">archive</a>')
        p=self.dependencies(b,[('workbook','book.xlsx'),('cells','cells.json'),('index','index.html')])
        if change_bundle:
            b=json.loads(p.read_text());change_bundle(b);p.write_text(json.dumps(b))
        return p

    def test_grouped_numeric_id_keeps_raw_fields_and_component_roles(self):
        for _ in range(2):
            self.assertEqual(import_source(self.c,'energystar-datacenter',self.current()),dict(registry_rows=2,model_assertions=1))
        row=self.c.db.execute("SELECT * FROM energystar_datacenter WHERE product_id='2382932'").fetchone()
        self.assertEqual(json.loads(row['raw_json'])['PD_ID'],'2,382,932')
        self.assertTrue(row['source_url'].endswith('/2382932'))
        self.assertEqual(len(self.c.search(target='Oracle')),1)
        self.assertEqual(self.c.search(target='Oracle',role='manufacturer'),[])
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT kind,value FROM identifiers')],[('energystar_id','2382933')])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='power_description'").fetchone()[0],0)
        result=lookup.run(self.c.db,'999')
        section=next(s for s in result['datasets'] if s['dataset']=='energystar_datacenter')
        self.assertEqual(section['matches'],1)

    def test_all_brand_zero_target_source_stays_loaded_and_sources_are_independent(self):
        for kind in e.SPECS:
            e.import_bundle(self.c,self.current(kind))
        def other(rows):rows[1][1:5]=['Other Company','Other Brand','Other array','OTHER-2']
        e.import_bundle(self.c,self.current(change_rows=other))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM energystar_datacenter').fetchone()[0],6)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],2)
        for kind in ['network','storage-block']:e.import_bundle(self.c,self.current(kind,change_rows=other))
        self.assertEqual(self.c.search(),[])
        section=next(s for s in lookup.run(self.c.db,'not present')['datasets'] if s['dataset']=='energystar_datacenter')
        self.assertEqual(section['state'],'loaded');self.assertEqual(len(section['sources']),3)

    def test_grouped_id_and_metadata_failures_preserve_existing_data(self):
        e.import_bundle(self.c,self.current())
        before=[tuple(r) for r in self.c.db.execute('SELECT * FROM energystar_datacenter')]
        changes=[dict(change_rows=lambda r:r[0].__setitem__(0,'23,82,932')),
                 dict(change_rows=lambda r:r[1].__setitem__(0,'2382932')),
                 dict(change_meta=lambda m:m['columns'][0].update(dataTypeName='text')),
                 dict(change_after=lambda m:m.update(rowsUpdatedAt=456))]
        for change in changes:
            with self.subTest(change=change),self.assertRaises(ValueError):e.import_bundle(self.c,self.current(**change))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM energystar_datacenter')],before)
        with self.assertRaises(ValueError):energystar.parse_id('2,382,932')
        for bad in ['2,38,2932','2.382932e6','2 382 932','02,382,932','2,382,932.0']:
            with self.assertRaises(ValueError):energystar.parse_id(bad,True)

    def test_historical_rows_preserve_provenance_without_target_inference(self):
        for kind in e.ARCHIVES:
            for _ in range(2):self.assertEqual(e.import_bundle(self.c,self.archive(kind)),dict(registry_rows=1,model_assertions=0))
        self.assertEqual(self.c.search(),[])
        rows=self.c.db.execute('SELECT sheet,row_number,certified_date_raw,available_date_raw FROM energystar_datacenter').fetchall()
        self.assertEqual([tuple(r) for r in rows],[('Storage QPL V1',5,'2019-01-01','2018-01-01')]*2)

    def test_invalid_historical_extraction_rejected_before_replacement(self):
        e.import_bundle(self.c,self.archive())
        before=[tuple(r) for r in self.c.db.execute('SELECT * FROM energystar_datacenter')]
        changes=[lambda d:d.update(workbook_sha256='0'*64),
                 lambda d:d['sheets'][0].update(name='Wrong sheet'),
                 lambda d:d['sheets'][0]['rows'][3].__setitem__(0,'Unknown ID'),
                 lambda d:d['sheets'][0]['rows'].pop(4),
                 lambda d:d['sheets'][0]['rows'][4].pop(),
                 lambda d:d['sheets'][0]['rows'].append(copy.deepcopy(d['sheets'][0]['rows'][4])),
                 lambda d:d['sheets'][0].update(merged_ranges=[[3,4,0,2]])]
        for change in changes:
            with self.subTest(change=change),self.assertRaises(ValueError):e.import_bundle(self.c,self.archive(change=change))
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM energystar_datacenter')],before)
        with self.assertRaises(ValueError):e.import_bundle(self.c,self.archive(change_bundle=lambda b:b['index'].update(sha256='0'*64)))


if __name__=='__main__':unittest.main()

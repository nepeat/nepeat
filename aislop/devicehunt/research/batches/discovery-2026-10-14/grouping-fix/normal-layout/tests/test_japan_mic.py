import copy
import html
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import japan_mic as j, lookup
from devicehunt.catalog import Catalog,digest


class JapanMICTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        self.c=Catalog(self.root/'db.sqlite')

    def tearDown(self):self.c.db.close();self.temp.cleanup()

    def page(self,query,rows):
        fields=['techCode','number','date','name','radioEquipmentCode','typeName','elecWave','spuriousRules','bodySar','note','organName','fullText']
        body='<html><head><title>技適検索</title></head><body><form method="GET">'
        body+=''.join('<input name="'+f+'" value="'+html.escape(query if f=='name' else '')+'">' for f in fields)
        body+='</form><table><tr>'+''.join('<th>'+s+'</th>' for s in ['技術基準適合証明等の種類','番号','年月日','氏名又は名称','特定無線設備の種別','型式又は名称'])+'</tr>'
        for row in rows:
            body+='<tr>'+''.join('<td>'+('<a href="'+html.escape(v)+'">'+html.escape(v)+'</a>' if n==1 else html.escape(v))+'</td>' for n,v in enumerate(row))+'</tr>'
        return (body+'</table>※最大2000件までの表示です</body></html>').encode()

    def bundle(self, rows=None, collection='base'):
        rows=rows if rows is not None else [['認証','020-123456','raw date','Accton Corp','class A','M1, M1-DC'],
                                           ['認証','020-123456','raw date','Accton Corp','class A','M1, M1-DC']]
        entries=[]
        for key,query in j.COLLECTIONS[collection].items():
            selected=rows if key==('accton' if collection=='base' else 'accton-ja') else []
            p=self.root/(key+'.html');p.write_bytes(self.page(query,selected));entry=dict(path=p.name,sha256=digest(p.read_bytes()))
            entries.append(dict(key=key,query=query,url=j.url(query),before=entry,after=entry,rows=len(selected)))
        p=self.root/'bundle.json';p.write_text(json.dumps(dict(schema_version=1,mirror=j.BASE,collection=collection,queries=entries)))
        return p

    def test_supplemental_names_keep_zero_query_scopes_and_never_merge_companies(self):
        j.import_bundle(self.c,self.bundle())
        row=['認証','NYCA0053','2000-10-03','アクトンテクノロジィ株式会社','class','AD11A-1']
        p=self.bundle([row],collection='name_variants');j.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM japan_equipment').fetchone()[0],3)
        section=next(s for s in lookup.run(self.c.db,'AD11A-1')['datasets'] if s['dataset']=='japan')
        self.assertEqual(section['matches'],1);self.assertEqual(len(section['sources']),12)
        self.assertEqual(section['rows'][0]['registered_name'],row[3])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM affiliations').fetchone()[0],0)
        bad=json.loads(p.read_text());bad['collection']='base';p.write_text(json.dumps(bad))
        with self.assertRaises(ValueError):j.import_bundle(self.c,p)
        j.import_bundle(self.c,self.bundle([],collection='name_variants'))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM japan_equipment').fetchone()[0],2)
        self.assertEqual(len(next(s for s in lookup.run(self.c.db,'M1')['datasets'] if s['dataset']=='japan')['sources']),12)

    def test_raw_duplicate_grouped_rows_and_zero_queries_are_searchable(self):
        result=j.import_bundle(self.c,self.bundle())
        self.assertEqual(result['rows'],2);self.assertEqual(result['model_assertions'],0)
        rows=list(self.c.db.execute('SELECT * FROM japan_equipment'))
        self.assertEqual([r['model_field'] for r in rows],['M1, M1-DC']*2)
        self.assertEqual(rows[0]['registered_name'],'Accton Corp')
        self.assertEqual(rows[0]['detail_url'],j.BASE+'020-123456')
        for t in ['devices','records','affiliations']:self.assertEqual(self.c.db.execute('SELECT count(*) FROM '+t).fetchone()[0],0)
        section=next(s for s in lookup.run(self.c.db,'M1-DC')['datasets'] if s['dataset']=='japan')
        self.assertEqual(section['matches'],2);self.assertEqual(len(section['sources']),6)
        self.assertEqual(section['rows'][0]['raw_cells'][5],'M1, M1-DC')
        self.assertEqual(self.c.db.execute("SELECT records_seen FROM sources WHERE id='japan-mic-mirror-amazon'").fetchone()[0],0)

    def test_bad_headers_filters_links_counts_hashes_and_changed_captures_preserve_data(self):
        p=self.bundle();j.import_bundle(self.c,p);original=json.loads(p.read_text())
        variants=[]
        for key,value in [('query','Oracle'),('rows',True),('rows',3),('url','https://example.org')]:
            bad=copy.deepcopy(original);bad['queries'][0][key]=value;variants.append(bad)
        for key,value in [('sha256','0'*64),('path','../accton.html')]:
            bad=copy.deepcopy(original);bad['queries'][0]['before'][key]=value;variants.append(bad)
        bad=copy.deepcopy(original);bad['queries'][1]=bad['queries'][0];variants.append(bad)
        for bad in variants:
            p.write_text(json.dumps(bad))
            with self.assertRaises(ValueError):j.import_bundle(self.c,p)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM japan_equipment').fetchone()[0],2)
        for changed in [self.page('Oracle',[]),self.page('Accton',[]),
                        self.page('Accton',[]).replace('番号'.encode(),b'other'),
                        self.page('Accton',[]).replace(b'</table>',b'')]:
            file=self.root/'changed.html';file.write_bytes(changed)
            bad=copy.deepcopy(original);bad['queries'][0]['after']=dict(path=file.name,sha256=digest(changed));p.write_text(json.dumps(bad))
            with self.assertRaises(ValueError):j.import_bundle(self.c,p)
        row=['認証','020-1','2026-01-01','Other Corp','class','M']
        with self.assertRaises(ValueError):j.parse(self.page('Accton',[row]),'Accton')
        row[3]='Accton';body=self.page('Accton',[row]).replace(b'href="020-1"',b'href="https://evil.example/020-1"')
        with self.assertRaises(ValueError):j.parse(body,'Accton')

    def test_display_cap_and_scoped_refresh(self):
        row=['認証','020-1','','Accton','class','Ｍ-1']
        with self.assertRaisesRegex(ValueError,'cap'):j.parse(self.page('Accton',[row]*2000),'Accton')
        self.c.source('unrelated','https://example.org','regulatory','','');self.c.db.commit()
        p=self.bundle([row]);j.import_bundle(self.c,p);j.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM japan_equipment').fetchone()[0],1)
        j.import_bundle(self.c,self.bundle([]))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM japan_equipment').fetchone()[0],0)
        self.assertIsNotNone(self.c.db.execute("SELECT 1 FROM sources WHERE id='unrelated'").fetchone())
        self.assertEqual(j.parse(self.page('Accton',[row]),'Accton')[0]['cells'][5],'Ｍ-1')


if __name__=='__main__':unittest.main()

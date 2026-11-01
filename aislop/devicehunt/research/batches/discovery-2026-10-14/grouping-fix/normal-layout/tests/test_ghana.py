import html
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ghana, lookup, vendor_inventory
from devicehunt.catalog import Catalog, digest


class GhanaTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
        self.c=Catalog(self.root/'db.sqlite')
        self.rows=[['5N4-3H-7DF-22F','Oracle America Inc.','Oracle','Server',"Oracle's Netra Sparc T3-1",''],
                   ['','Other Corp','Oracle','Radio','RADIO-2',''],
                   ['OTHER-ID','Oracle','Other Brand','Grouped','ONE / TWO',''],
                   ['4','다른 회사 (Oracle)','Other Brand','Server','WRONG-4',''],
                   ['5','Oracle','Other Brand','Server','OEM-5','']]

    def tearDown(self):
        self.c.db.close();self.tmp.cleanup()

    def body(self, rows=None):
        rows=self.rows if rows is None else rows
        table='<table class="data display ta_datatable"><tr>'+''.join('<th>'+s+'</th>' for s in ghana.HEADER)+'</tr>'
        for i,row in enumerate(rows,1):
            table+='<tr>'+''.join('<td>'+html.escape(s)+'</td>' for s in row[:5])
            table+='<td><a href="search_type_approval_view_details.php?typeApproveDetailID='+str(i)+'"><img alt="details"></a></td></tr>'
        table+='</table>'
        return ('<title>'+ghana.TITLE+'</title><table><tr><td>'+table+'</td></tr></table>').encode()

    def save(self, change=None):
        def entry(name,body):
            (self.root/name).write_bytes(body);return dict(path=name,sha256=digest(body))
        before=self.body();after=self.body(self.rows[:-1]) if change=='changed' else before
        data=dict(schema_version=1,source_url=ghana.URL,expected_rows=len(self.rows),
                  before=entry('before.html',before),after=entry('after.html',after))
        if change=='count':data['expected_rows']+=1
        if change=='hash':data['before']['sha256']='0'*64
        if change=='url':data['source_url']='https://example.org'
        p=self.root/'bundle.json';p.write_text(json.dumps(data));return p

    def test_full_raw_table_and_whole_manufacturer_roles(self):
        result=ghana.import_bundle(self.c,self.save())
        self.assertEqual(result,dict(registry_rows=5,model_assertions=2,blank_approval_numbers=1))
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},
                         {"Oracle's Netra Sparc T3-1",'OEM-5'})
        self.assertEqual(self.c.db.execute("SELECT brand FROM devices WHERE model='OEM-5'").fetchone()[0],'Other Brand')
        self.assertEqual(vendor_inventory.run(self.c.db,scope='manufacturer',limit=-1)['unique_identities'],2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0],0)
        raw=self.c.db.execute('SELECT * FROM ghana_equipment WHERE row_number=2').fetchone()
        self.assertEqual(raw['approval_number'],'')
        self.assertEqual(len(json.loads(raw['cells_json'])),6)
        results={r['dataset']:r for r in lookup.run(self.c.db,'RADIO-2')['datasets']}
        self.assertEqual(results['ghana']['matches'],1);self.assertEqual(results['catalog']['matches'],0)
        facts=dict(self.c.db.execute('SELECT name,value FROM facts'))
        self.assertIn('not imported',facts['detail_page_scope'])
        self.assertIn('typeApproveDetailID=',facts['detail_url'])

    def test_bad_capture_fails_before_replacing_records_or_raw_rows(self):
        ghana.import_bundle(self.c,self.save())
        before={t:[tuple(r) for r in self.c.db.execute('SELECT * FROM '+t)] for t in ('records','sources','ghana_equipment')}
        for change in ('changed','count','hash','url'):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):ghana.import_bundle(self.c,self.save(change))
                for t,rows in before.items():self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM '+t)],rows)

    def test_duplicate_links_malformed_headers_and_unexpected_link_targets_fail(self):
        b=self.body()
        for broken in (b.replace(b'Name of Manufacture',b'Applicant'),
                       b.replace(b'typeApproveDetailID=2',b'typeApproveDetailID=1'),
                       b.replace(b'search_type_approval_view_details.php?',b'https://example.org/?'),
                       b.replace(b'typeApproveDetailID=1',b'typeApproveDetailID=01')):
            with self.assertRaises(ValueError):ghana.parse(broken)

    def test_duplicate_model_assertions_and_withdrawal_do_not_affect_other_source(self):
        self.rows.append(['REPEAT','Oracle America Inc.','Oracle','Server',"Oracle's Netra Sparc T3-1",''])
        ghana.import_bundle(self.c,self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],2)
        self.c.source('other','https://example.org','vendor','test','test')
        self.c.add('other','1','https://example.org','Edge-Core','KEPT','Kept','test',{});self.c.finish('other',1)
        self.rows=[['','Other Corp','Other Brand','Radio','RAW-ONLY','']]
        ghana.import_bundle(self.c,self.save())
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT model FROM devices')],['KEPT'])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ghana_equipment').fetchone()[0],1)

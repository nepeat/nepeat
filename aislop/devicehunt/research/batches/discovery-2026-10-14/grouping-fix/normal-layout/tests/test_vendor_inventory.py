import contextlib
import csv
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import vendor_inventory as v
from devicehunt.__main__ import main, report
from devicehunt.catalog import Catalog


class VendorInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.path = self.root/'db.sqlite'; self.c = Catalog(self.path)
        proof = self.root/'source.txt';proof.write_text('Synthetic source fixture')
        for sid,tier,path in [('vendor','vendor',proof),('seller','marketplace',proof),('unhashed','vendor',None)]:
            self.c.source(sid,'https://example.org/'+sid,tier,'fixture','Fixture scope',path)
        rows = [
            ('vendor','shared','Other Brand','Shared-1',[('Oracle','regulatory_applicant'),('Celestica','manufacturer')]),
            ('vendor','shared-brand','Other Brand','Shared-1',[('Oracle','brand')]),
            ('vendor','switch','Edge-Core','Switch-1',[('Accton','declared_oem'),('Edge-Core','registry_manufacturer')]),
            ('vendor','factory','Cisco','NCS-1',[('Celestica','listed_factory')]),
            ('vendor','legal','Oracle','WH1CL',[('Oracle America Inc.','manufacturer')]),
            ('vendor','unknown','Accton','Future-1',[('Accton','future_unknown_role')]),
            ('seller','sale','Oracle','Seller-1',[('Oracle','manufacturer')]),
            ('unhashed','unproven','Accton','Unproven-1',[('Accton','manufacturer')]),
        ]
        for sid,key,brand,model,aff in rows:
            self.c.add(sid,key,'https://example.org/'+key,brand,model,model,'device',{},aff)
        self.c.finish('vendor',6);self.c.finish('seller',1);self.c.finish('unhashed',1)

    def tearDown(self):
        self.c.db.close();self.temp.cleanup()

    def test_manufacturer_target_and_role_must_share_record_and_factory_stays_separate(self):
        result=v.run(self.c.db,limit=-1)
        self.assertEqual({(r['target'],r['model']) for r in result['rows']},
                         {('Oracle','WH1CL'),('Celestica','Shared-1'),('Accton','Switch-1'),('Edge-Core','Switch-1')})
        self.assertEqual(result['identity_target_pairs'],4)
        self.assertEqual(result['unique_identities'],3)
        self.assertEqual(v.run(self.c.db,'Oracle')['rows'][0]['evidence'][0]['company'],'Oracle America Inc.')
        factory=v.run(self.c.db,'Celestica','factory')
        self.assertEqual([r['model'] for r in factory['rows']],['NCS-1'])
        self.assertEqual(factory['rows'][0]['brand'],'Cisco')
        self.assertEqual(factory['rows'][0]['evidence'][0]['evidence_class'],'factory_listing')

    def test_all_scope_preserves_seller_unhashed_unknown_roles_and_metadata(self):
        result=v.run(self.c.db,scope='all',limit=-1)
        flat=v.flat_rows(result)
        self.assertTrue(any(e['role']=='future_unknown_role' and e['evidence_class']=='other_association' for e in flat))
        for model in ['Seller-1','Unproven-1']:
            row=next(e for e in flat if e['model']==model)
            self.assertFalse(row['source_snapshot_eligible'])
            self.assertEqual(row['inventory_scope'],'all')
        self.assertTrue(all('coverage' in e and 'source_url' in e and 'source_sha256' in e for e in flat))
        shared=next(r for r in result['rows'] if r['target']=='Oracle' and r['model']=='Shared-1')
        self.assertEqual({e['role'] for e in shared['evidence']},{'brand','regulatory_applicant'})
        self.assertEqual({e['evidence_class'] for e in shared['evidence']},{'other_association'})

    def test_limits_counts_aliases_and_invalid_input(self):
        for limit in [0,1,3]:
            result=v.run(self.c.db,limit=limit)
            self.assertEqual(result['identity_target_pairs'],4)
            self.assertEqual(result['shown'],limit)
            self.assertTrue(result['truncated'])
            self.assertEqual(sum(r['identities'] for r in result['counts_by_target']),4)
        self.assertEqual(v.run(self.c.db,'edgecore')['vendor'],'Edge-Core')
        self.assertEqual(v.run(self.c.db,'Amazon Data Services')['counts_by_target'][0]['identities'],0)
        for args in [('unknown','all',1),(None,'unknown',1),(None,'all',-2),(None,'all',True)]:
            with self.assertRaises(ValueError):v.run(self.c.db,*args)

    def test_cli_read_only_json_csv_and_missing_database(self):
        before=self.path.read_bytes()
        out=io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(main(['--db',str(self.path),'vendor-inventory','Oracle','--format','json']),0)
        self.assertEqual(json.loads(out.getvalue())['identity_target_pairs'],1)
        out=io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(main(['--db',str(self.path),'vendor-inventory','--scope','all','--limit','-1','--format','csv']),0)
        rows=list(csv.DictReader(io.StringIO(out.getvalue())))
        self.assertTrue(any(r['model']=='NCS-1' and r['role']=='listed_factory' for r in rows))
        self.assertEqual(before,self.path.read_bytes())
        missing=self.root/'missing.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db',str(missing),'vendor-inventory']),1)
        self.assertFalse(missing.exists())

    def test_existing_caller_transaction_is_preserved(self):
        self.c.db.execute("UPDATE devices SET model='Pending' WHERE model='WH1CL'")
        self.assertEqual(v.run(self.c.db,'Oracle')['rows'][0]['model'],'Pending')
        self.assertTrue(self.c.db.in_transaction)
        self.c.db.rollback()
        self.assertEqual(v.run(self.c.db,'Oracle')['rows'][0]['model'],'WH1CL')

    def test_report_scope_metadata_matches_inventory_and_does_not_change_database(self):
        before = self.path.read_bytes()
        out = self.root/'catalog.html'
        reader = Catalog(self.path, read_only=True)
        try:
            report(reader, out)
        finally:
            reader.db.close()
        payload = out.read_text().split('<script id="data" type="application/json">',1)[1].split('</script>',1)[0]
        devices = json.loads(payload)
        for scope,category in [('manufacturer','manufacturer_claim'),('factory','factory_listing'),('all',None)]:
            actual = {(a['target'],d['id'],a['record_id'],a['company'],a['role'])
                      for d in devices for a in d['affiliations'] if a['target'] and
                      (category is None or a['source_snapshot_eligible'] and a['evidence_class']==category)}
            expected = {(r['target'],r['device_id'],e['record_id'],e['company'],e['role'])
                        for r in v.run(self.c.db,scope=scope,limit=-1)['rows'] for e in r['evidence']}
            self.assertEqual(actual,expected)
        seller = next(d for d in devices if d['model']=='Seller-1')['affiliations'][0]
        self.assertEqual(seller['source_tier'],'marketplace')
        self.assertFalse(seller['source_snapshot_eligible'])
        self.assertEqual(before,self.path.read_bytes())

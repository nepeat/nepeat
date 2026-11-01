import contextlib
import csv
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt import hyperscaler_inventory as h
from devicehunt.__main__ import main


class HyperscalerInventoryTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.root=Path(self.tmp.name)
        self.path=self.root/'catalog.sqlite'; self.c=Catalog(self.path)
        proof=self.root/'source.txt'; proof.write_text('Synthetic source fixture')
        for sid,tier,p in [('vendor','vendor',proof),('seller','marketplace',proof),('unhashed','vendor',None)]:
            self.c.source(sid,'https://example.org/'+sid,tier,'fixture','Fixture scope',p)
        entries=[
            ('vendor','deployed','Shared-1',[('Oracle','brand'),('Celestica','manufacturer')],'documented_platform_deployment',''),
            ('vendor','separate','Shared-1',[('Oracle','manufacturer')],None,''),
            ('vendor','lineage','Shared-1',[('Oracle','brand')],'documented_design_lineage',''),
            ('vendor','revision','Shared-1',[('Celestica','manufacturer')],'announced_platform_deployment','B'),
            ('vendor','product','Product-1',[('AWS','documentation_vendor')],'documented_product_family',''),
            ('vendor','future','Future-1',[('Accton','future_role')],'future_relationship',''),
            ('seller','sale','Sale-1',[('Oracle','manufacturer')],'documented_platform_deployment',''),
            ('unhashed','uncached','Uncached-1',[('Oracle','manufacturer')],'documented_platform_deployment',''),
        ]
        for sid,key,model,aff,relationship,revision in entries:
            self.c.add(sid,key,'https://example.org/'+key,'Other Brand',model,model,'switch',{},aff,
                       facts=[('published_date','2020-01-02')],
                       hyperscaler=[('AWS',relationship,'Fixture historical claim '+key)] if relationship else [],revision=revision)
        self.c.finish('vendor',6);self.c.finish('seller',1);self.c.finish('unhashed',1)

    def tearDown(self):
        self.c.db.close();self.tmp.cleanup()

    def test_default_preserves_relationships_revisions_dates_and_full_provenance(self):
        result=h.run(self.c.db,limit=-1)
        self.assertEqual((result['assertions'],result['unique_identities']),(5,4))
        self.assertEqual(result['counts_by_relationship'],{
            'announced_platform_deployment':1,'documented_design_lineage':1,
            'documented_platform_deployment':1,'documented_product_family':1,'future_relationship':1})
        self.assertEqual(result['counts_by_operator'],{'AWS':5})
        self.assertEqual({r['revision'] for r in result['rows']},{'','B'})
        self.assertTrue(all(r['source_snapshot_eligible'] and r['source_sha256'] for r in result['rows']))
        self.assertTrue(all(r['source_dates']==[{'field':'published_date','value':'2020-01-02'}] for r in result['rows']))
        self.assertTrue(all(r['record_id'] and r['source_url'] and r['coverage'] for r in result['rows']))
        deployed=h.run(self.c.db,relationship='documented_platform_deployment')
        self.assertEqual([r['detail'] for r in deployed['rows']],['Fixture historical claim deployed'])

    def test_vendor_role_and_hyperscaler_claim_must_share_one_record_and_affiliation(self):
        self.assertEqual(h.run(self.c.db,'Oracle',role='manufacturer')['assertions'],0)
        self.assertEqual(h.run(self.c.db,'Oracle',role='brand')['assertions'],2)
        rows=h.run(self.c.db,'Celestica',role='manufacturer')['rows']
        self.assertEqual({r['relationship'] for r in rows},{'documented_platform_deployment','announced_platform_deployment'})
        for r in rows:
            self.assertEqual(r['matched_affiliations'],[{'company':'Celestica','target':'Celestica','role':'manufacturer'}])
        # A recognizable operator does not imply the target legal entity.
        self.assertEqual(h.run(self.c.db,'Amazon Data Services')['rows'],[])
        self.assertEqual(h.run(self.c.db,role='documentation_vendor')['assertions'],1)
        self.assertEqual(h.run(self.c.db,'accton')['vendor'],'Accton')

    def test_unverified_claims_remain_visible_only_when_requested_and_keep_status(self):
        result=h.run(self.c.db,include_unverified=True,limit=-1)
        self.assertEqual(result['assertions'],7)
        weak=[r for r in result['rows'] if not r['source_snapshot_eligible']]
        self.assertEqual({r['source_id'] for r in weak},{'seller','unhashed'})
        self.assertEqual({r['tier'] for r in weak},{'marketplace','vendor'})
        self.assertEqual(h.run(self.c.db,'Oracle',role='manufacturer',include_unverified=True)['assertions'],2)

    def test_exact_filters_limits_and_errors(self):
        all_rows=h.run(self.c.db,limit=-1)['rows']
        for limit in [0,1,5,10]:
            result=h.run(self.c.db,operator=' aws ',limit=limit)
            self.assertEqual(result['assertions'],5)
            self.assertEqual(result['rows'],all_rows[:limit])
            self.assertEqual(result['truncated'],limit<5)
        for query in ['AW','Amazon','AWS OR 1=1']:
            self.assertEqual(h.run(self.c.db,operator=query)['rows'],[])
        self.assertEqual(h.run(self.c.db,relationship='deployment')['rows'],[])
        for kwargs in [dict(vendor='unknown'),dict(role=''),dict(operator=' '),dict(relationship=''),
                       dict(limit=True),dict(limit=-2),dict(include_unverified=1)]:
            with self.assertRaises(ValueError):h.run(self.c.db,**kwargs)

    def test_cli_json_csv_table_are_readonly_and_missing_database_is_not_created(self):
        before=self.path.read_bytes()
        for fmt in ['json','csv','table']:
            output=io.StringIO()
            with contextlib.redirect_stdout(output):
                status=main(['--db',str(self.path),'hyperscaler-inventory','--vendor','Celestica',
                             '--role','manufacturer','--format',fmt])
            self.assertEqual(status,0)
            if fmt=='json':self.assertEqual(json.loads(output.getvalue())['assertions'],2)
            elif fmt=='csv':
                rows=list(csv.DictReader(io.StringIO(output.getvalue())))
                self.assertEqual(len(rows),2)
                self.assertTrue(all(json.loads(r['matched_affiliations_json'])[0]['company']=='Celestica' for r in rows))
                self.assertTrue(all(json.loads(r['source_dates_json'])[0]['value']=='2020-01-02' for r in rows))
            else:
                self.assertIn('announced_platform_deployment',output.getvalue())
                self.assertIn('Celestica (manufacturer)',output.getvalue())
        self.assertEqual(self.path.read_bytes(),before)
        missing=self.root/'missing.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db',str(missing),'hyperscaler-inventory']),1)
        self.assertFalse(missing.exists())

    def test_caller_transaction_and_result_mutation_are_preserved(self):
        self.c.db.execute('BEGIN')
        self.c.db.execute("UPDATE facts SET value='2099-01-01' WHERE name='published_date'")
        result=h.run(self.c.db)
        self.assertTrue(self.c.db.in_transaction)
        self.assertTrue(all(r['source_dates'][0]['value']=='2099-01-01' for r in result['rows']))
        result['rows'][0]['affiliations'].clear()
        self.assertTrue(all(r['affiliations'] for r in h.run(self.c.db)['rows']))
        self.c.db.rollback()
        self.assertTrue(all(r['source_dates'][0]['value']=='2020-01-02' for r in h.run(self.c.db)['rows']))
        self.assertFalse(self.c.db.in_transaction)

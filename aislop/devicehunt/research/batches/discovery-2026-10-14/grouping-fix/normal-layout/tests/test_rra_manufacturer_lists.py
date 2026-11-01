import json
import shutil
import unittest

from devicehunt import rra, rra_manufacturer_lists as lists, vendor_inventory
from devicehunt.catalog import digest, target_for
from tests import test_rra_celestica_historical


class ManufacturerListTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_rra_celestica_historical.HistoricalCompanyTests()
        self.fixture.setUp(); self.c = self.fixture.c; self.root = self.fixture.root
        self.rows = self.fixture.fixture.fixture.rows
        self.rows[0]['manufacturer'] = 'Juniper Networks, Inc.,Celestica (Suzhou) Technology Co.,Ltd.'
        self.rows[0]['applicant'] = 'Oracle America, Inc.'

    def tearDown(self):
        self.fixture.tearDown()

    def save(self, expected=3, proof=True):
        capture = self.fixture.save()
        if not proof:
            data=json.loads(capture.read_text());data.pop('company_evidence');capture.write_text(json.dumps(data))
        bundle=dict(schema_version=1, kind=lists.SOURCE,
                    captures=[dict(path=capture.name,sha256=digest(capture.read_bytes()))],
                    expected=dict(approval_target_pairs=int(bool(expected)),model_assertions=expected,
                                  assertions_by_target={'Celestica':expected} if expected else {}))
        path=self.root/'lists.json';path.write_text(json.dumps(bundle));return path

    def test_reviewed_names_preserve_context_roles_and_original_provenance(self):
        path=self.save();lists.import_bundle(self.c,path)
        self.assertEqual(len(vendor_inventory.run(self.c.db,'Celestica',limit=-1)['rows']),3)
        self.assertEqual(vendor_inventory.run(self.c.db,'Oracle',limit=-1)['rows'],[])
        self.assertEqual(len(self.c.search(target='Oracle',role='regulatory_applicant')),3)
        self.assertEqual(self.c.search(role='listed_factory'),[])
        self.assertEqual(self.c.search(hyperscaler=True),[])
        for record in self.c.db.execute('SELECT raw_json FROM records'):
            raw=json.loads(record[0])
            self.assertEqual(raw['row']['manufacturer'],self.rows[0]['manufacturer'])
            self.assertEqual(raw['names'],['Celestica (Suzhou) Technology Co.,Ltd.'])
            self.assertEqual(raw['context_parts'][0],'Juniper Networks, Inc.')
            proof=raw['observations'][0]
            self.assertTrue(proof['detail_url'].startswith(rra.DETAIL))
            self.assertEqual(len(proof['detail_sha256']),64)
            self.assertEqual(proof['company_evidence'][raw['names'][0]]['exhibit'],'8.1')
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT kind FROM identifiers')},{'rra_certificate_id'})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],0)
        self.assertIsNone(target_for(self.rows[0]['manufacturer']))
        self.assertIsNone(target_for('Celestica (Suzhou) Technology Co.,Ltd.'))

    def test_two_named_members_keep_separate_legal_names_on_one_model_assertion(self):
        self.rows[0]['manufacturer']='CELESTICA (THAILAND) LTD.,Celestica(DongGuan-SSL) Technology Ltd.'
        self.rows[0]['main_model']='QDX 6000';self.rows[0]['derived_models']=''
        lists.import_bundle(self.c,self.save(1))
        result=vendor_inventory.run(self.c.db,'Celestica',limit=-1)
        self.assertEqual(result['identity_target_pairs'],1)
        names=[tuple(r) for r in self.c.db.execute("SELECT company,target FROM affiliations WHERE role='registry_manufacturer'")]
        self.assertEqual(set(names),{('CELESTICA (THAILAND) LTD.','Celestica'),
                                   ('Celestica(DongGuan-SSL) Technology Ltd.','Celestica')})

    def test_evidence_withdrawal_reimport_and_source_isolation(self):
        path=self.save();lists.import_bundle(self.c,path);lists.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)
        self.c.source('other','https://example.org','vendor','fixture','fixture')
        self.c.add('other','one','https://example.org','Accton','KEEP-1','kept','switch',{})
        self.c.finish('other',1)
        lists.import_bundle(self.c,self.save(0,proof=False))
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},{'KEEP-1'})
        self.assertEqual(self.c.search('MAIN-10'),[])

    def test_unreviewed_lists_annotations_and_parenthetical_relationships_are_excluded(self):
        for name in ['Other Company,Celestica Inc.', 'Other (Celestica Inc.)',
                     'Juniper Networks, Inc.,Celestica (Suzhou) Technology Co.,Ltd. (not manufacturer)',
                     '주식회사 트렌토 시스템즈 (Celestica Inc.)',
                     'CELESTICA(DONGGUAN-SSL) TECHNOLOGY LTD, NORTHERN AREA.',
                     'CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO LTD (Celestica (Thailand) Limited)']:
            with self.subTest(name=name):
                self.rows[0]['manufacturer']=name
                self.assertEqual(lists.import_bundle(self.c,self.save(0))['model_assertions'],0)

    def test_invalid_dependency_or_count_fails_before_replacing_existing_claims(self):
        lists.import_bundle(self.c,self.save())
        before=[tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for change in ('hash','count','duplicate','path','detail'):
            path=self.save();data=json.loads(path.read_text())
            if change=='hash':data['captures'][0]['sha256']='0'*64
            if change=='count':data['expected']['model_assertions']=4
            if change=='duplicate':data['captures']*=2
            if change=='path':data['captures'][0]['path']='../bundle.json'
            if change=='detail':(self.root/'0.html').write_text('changed')
            path.write_text(json.dumps(data))
            with self.subTest(change=change):
                with self.assertRaises(ValueError):lists.import_bundle(self.c,path)
                self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')],before)

    def test_duplicate_approval_keeps_observations_but_conflicting_rows_fail(self):
        path=self.save(); data=json.loads(path.read_text())
        other=self.root/'second';other.mkdir()
        for file in self.root.iterdir():
            if file.is_file():shutil.copyfile(file,other/file.name)
        entry=dict(data['captures'][0]);entry['path']='second/bundle.json';data['captures'].append(entry)
        path.write_text(json.dumps(data));lists.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)
        raw=json.loads(self.c.db.execute('SELECT raw_json FROM records LIMIT 1').fetchone()[0])
        self.assertEqual(len(raw['observations']),2)
        # Change a second observation and its hashes: semantic disagreement must
        # still fail even when every individual capture passes integrity checks.
        detail=other/'0.html';detail.write_text(detail.read_text().replace('MAIN-10','OTHER-10'))
        bundle=other/'bundle.json';second=json.loads(bundle.read_text())
        second['details'][0]['sha256']=digest(detail.read_bytes());bundle.write_text(json.dumps(second))
        data['captures'][1]['sha256']=digest(bundle.read_bytes());path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError,'Conflicting'):
            lists.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)

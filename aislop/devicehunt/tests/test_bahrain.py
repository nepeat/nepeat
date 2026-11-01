import copy
import json
import tempfile
import unittest
from pathlib import Path
from urllib.parse import quote

from devicehunt.catalog import Catalog, digest
from devicehunt import bahrain as b


class BahrainTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root/'db.sqlite')
        def row(ref, manufacturer, model):
            return dict(authority_ref=ref,manufacturer=manufacturer,model_number=model,
                        device_description='Server',expiry='18-04-2025',status_code='RNW',
                        marketing_name=None,device_remark=None,authority_remark=None)
        self.rows = [row('A/1','AMAZON DATA SERVICES, INC.','SCOUT01/AWS'),
                     row('A/2','ACCTON TECHNOLOGY CORPORATION','DMC-12-48C-AS0 48V, DMC-12-48R-AS0 48V'),
                     row('A/3','Amazon Technologies Inc.','CONSUMER-OTHER'),
                     row('A/4','CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO., LTD.','CS8200-32X')]

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def save(self, responses=None):
        data={'schema_version':1,'source_url':b.API_URL,'responses':{}}
        bodies={'full_before':self.rows,'full_after':self.rows}
        bodies.update({q:[r for r in self.rows if q in r['manufacturer'].casefold()] for q in b.QUERIES})
        bodies.update(responses or {})
        for key,rows in bodies.items():
            p=self.root/(key.replace(' ','-')+'.json');p.write_text(json.dumps(rows))
            data['responses'][key]={'path':p.name,'sha256':digest(p.read_bytes()),'rows':len(rows),
                                   'url':b.API_URL if key.startswith('full_') else b.API_URL+'?search='+quote(key)}
        p=self.root/'bundle.json';p.write_text(json.dumps(data));return p

    def test_manufacturer_and_model_scope_and_idempotency(self):
        path=self.save()
        result=b.import_bundle(self.c,path)
        self.assertEqual(result,{'registry_rows':4,'target_rows':3,'model_assertions':4})
        self.assertEqual(len(self.c.search(target='Amazon Data Services')),1)
        self.assertEqual(self.c.search('CONSUMER-OTHER'),[])
        self.assertEqual(self.c.search(role='manufacturer'),[])
        self.assertEqual(self.c.search(hyperscaler=True),[])
        models={r[0] for r in self.c.db.execute('SELECT model FROM devices')}
        self.assertIn('SCOUT01/AWS',models)
        self.assertIn('DMC-12-48R-AS0 48V',models)
        self.assertNotIn('DMC-12-48R-AS0',models)
        self.assertIn('CS8200-32X',models)
        self.assertEqual(self.c.db.execute('SELECT expiry_date FROM bahrain_equipment LIMIT 1').fetchone()[0],'2025-04-18')
        b.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],4)
        self.rows=self.rows[:1]
        b.import_bundle(self.c,self.save())
        self.assertEqual(len(self.c.search()),1)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM bahrain_equipment').fetchone()[0],1)

    def test_inconsistent_filtered_or_full_results_preserve_prior_data(self):
        b.import_bundle(self.c,self.save())
        for changes in [{'accton':[]},{'full_after':self.rows[:1]},
                        {'oracle':[dict(self.rows[0],authority_ref='missing')]},
                        {'full_before':self.rows+[self.rows[0]]}]:
            with self.assertRaises(ValueError):
                b.import_bundle(self.c,self.save(changes))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],4)
        path=self.save();(self.root/'full_before.json').write_text('[]')
        with self.assertRaisesRegex(ValueError,'checksum'):
            b.import_bundle(self.c,path)

    def test_unexpected_result_shapes_and_dates_are_rejected(self):
        for value in [{'results':self.rows},[dict(self.rows[0],expiry='2025-04-18')],
                      [dict(self.rows[0],manufacturer=None)],[dict(self.rows[0],extra='new')]]:
            with self.assertRaises(ValueError): b.parse_rows(json.dumps(value))

import contextlib
import copy
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import tested_components as t
from devicehunt.catalog import Catalog, digest
from devicehunt.__main__ import main


class TestedComponentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.root = Path(self.tmp.name)
        self.path = self.root / 'db.sqlite'; self.c = Catalog(self.path)
        proof = self.root / 'extract.json'; proof.write_text('{}')
        self.c.source('test','https://example.org/report','primary_document_mirror','fixture',
                      'Factual extract, original unavailable',proof)
        self.data = dict(schema_version=1,report_number='TEST-1',issued_on='2022-01-19',
            capture_method='factual extract',original_document_status='unavailable_http_403',
            configurations=[dict(configuration_id=str(n),source_page=6+n,components=[
                dict(kind='PSU',manufacturer='Parts Inc.',model='AC-123' if n==1 else 'DC-456',
                     description='Test configuration only',quantity=2),
                dict(kind='SSD',manufacturer='Disk Inc.',model='SAME-SSD',
                     description='1 TB' if n==1 else '2 TB',quantity=n)]) for n in (1,2)])
        self.c.add('test','components','https://example.org/report','Other Brand','SERVER-1','Server','server',{},
                   [('Amazon Data Services','regulatory_applicant')],
                   facts=[('tested_configurations',json.dumps(self.data))])
        # Same identity, different source-record association cannot lend its vendor.
        self.c.add('test','other','https://example.org/other','Other Brand','SERVER-1','Server','server',{},
                   [('Celestica','manufacturer')])
        self.c.finish('test',2)

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def test_configuration_quantities_and_provenance_are_never_merged(self):
        rows=t.run(self.c.db,'SAME-SSD')['rows']
        self.assertEqual([(r['configuration_id'],r['quantity'],r['description'],r['source_page']) for r in rows],
                         [('1',1,'1 TB',7),('2',2,'2 TB',8)])
        self.assertTrue(all(r['source_snapshot_eligible'] and r['source_sha256'] for r in rows))
        self.assertTrue(all(r['original_document_status']=='unavailable_http_403' for r in rows))
        self.assertEqual(t.run(self.c.db,'ac-123')['rows'][0]['component_manufacturer'],'Parts Inc.')
        self.assertEqual(t.run(self.c.db,'%',limit=-1)['rows'],[])
        self.assertEqual(t.run(self.c.db,vendor='Celestica')['matches'],0)
        self.assertEqual(t.run(self.c.db,vendor='Amazon Data Services')['matches'],4)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0],0)
        result=t.run(self.c.db,limit=0)
        self.assertEqual((result['matches'],result['shown'],result['truncated']),(4,0,True))

    def test_malformed_configurations_are_rejected_instead_of_silently_dropped(self):
        for change in [lambda d:d['configurations'].append(d['configurations'][0]),
                       lambda d:d['configurations'][0].update(source_page=True),
                       lambda d:d['configurations'][0]['components'][0].update(quantity=True),
                       lambda d:d['configurations'][0]['components'][0].update(quantity=0),
                       lambda d:d['configurations'][0]['components'][0].pop('model'),
                       lambda d:d.update(issued_on='not a date')]:
            data=copy.deepcopy(self.data);change(data)
            with self.assertRaises(ValueError):t.parse(json.dumps(data))
        for kwargs in [dict(limit=-2),dict(limit=True),dict(query=1),dict(vendor='Unknown')]:
            with self.assertRaises(ValueError):t.run(self.c.db,**kwargs)

    def test_cli_is_read_only_and_caller_transaction_stays_open(self):
        before=digest(self.path.read_bytes())
        for fmt in ('table','json','csv'):
            output=io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(main(['--db',str(self.path),'tested-components','AC-123','--format',fmt]),0)
            self.assertIn('AC-123',output.getvalue())
            self.assertNotIn('DC-456',output.getvalue())
        self.assertEqual(digest(self.path.read_bytes()),before)
        missing=self.root/'missing.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db',str(missing),'tested-components']),1)
        self.assertFalse(missing.exists())
        self.c.db.execute('BEGIN')
        self.c.db.execute("INSERT INTO facts SELECT id,'temporary','marker' FROM records LIMIT 1")
        t.run(self.c.db)
        self.assertTrue(self.c.db.in_transaction)
        self.c.db.rollback()

import contextlib
import hashlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import pci, lookup
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main

FIXTURE = '''# Version: 2026.09.25
1113  Accton Technology Corporation
# Exact PCI name comment
\t0001  Network controller
\t\t18d4 0002  Board from subsystem namespace
\t\t0000 0003  Unresolved subvendor example
18d4  Celestica
\t0001  Other controller
S 18d4
\t0002  Generic board
C 02  Network controller
\t00  Ethernet controller
\t\t00  Generic interface
'''


class PciTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def bundle(self, text=FIXTURE):
        rows, counts, ignored = pci.parse(text.encode())
        sha = 'a' * 40
        data = dict(schema_version=1, source_url=pci.URL, commit=sha, version='2026.09.25',
                    ids_url='https://raw.githubusercontent.com/pciutils/pciids/' + sha + '/pci.ids',
                    expected_counts=counts, ignored_extension_lines=ignored,
                    undefined_subvendor_ids=sorted({r['subvendor_id'] for r in rows if r['subvendor_id'] and not r['subvendor_name']}))
        for key, name, body in [('ids','pci.ids',text), ('commit_metadata','commit.json',json.dumps({'sha':sha})),
                                 ('readme','README.md','pci.ids: 3-clause BSD License')]:
            (self.root/name).write_text(body)
            data[key] = {'path':name, 'sha256':hashlib.sha256(body.encode()).hexdigest()}
        p=self.root/'bundle.json';p.write_text(json.dumps(data));return p

    def test_hierarchy_distinguishes_device_and_subsystem_vendors(self):
        rows, counts, ignored=pci.parse(FIXTURE.encode())
        self.assertEqual(counts, {'vendor':2,'device':2,'subsystem':2,'generic_subsystem':1,'class':1,'subclass':1,'prog_if':1})
        self.assertEqual(ignored, [])
        row=next(r for r in rows if r['id_path']=='1113:0001/18d4:0002')
        self.assertEqual(row['vendor_name'], 'Accton Technology Corporation')
        self.assertEqual(row['subvendor_name'], 'Celestica')
        self.assertEqual(next(r for r in rows if r['id_path']=='1113:0001')['comments'], '# Exact PCI name comment')
        self.assertEqual(next(r for r in rows if r['subvendor_id']=='0000')['subvendor_name'], '')

    def test_malformed_hierarchy_duplicates_and_unknown_extensions(self):
        for text in ['\t1234  Orphan\n', '1113  Accton\n\t\t1113 0001  Orphan\n',
                     '1113  Accton\n\t0001  A\n\t0001  B\n',
                     '1113  Accton\n    0001  Spaces are not a TAB\n',
                     FIXTURE+'\t\t0001  Invalid class ID width\n']:
            with self.assertRaises(ValueError):pci.parse(text.encode())
        text=FIXTURE+'Z extra section\n\t0001  ignored\n1114  New vendor\n\t0001  New device\n'
        rows,_,ignored=pci.parse(text.encode())
        self.assertEqual(len(ignored),2)
        self.assertEqual(rows[-1]['vendor_id'],'1114')
        self.assertNotIn('ignored',[r['name'] for r in rows])

    def test_import_search_no_chassis_assertions_and_idempotency(self):
        p=self.bundle()
        for _ in range(2):pci.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM pci_entries').fetchone()[0],10)
        self.assertEqual(self.c.search(),[])
        sections={r['dataset']:r for r in lookup.run(self.c.db,'1113:0001')['datasets']}
        self.assertEqual(sections['pci']['matches'],3)
        self.assertEqual(sections['catalog']['matches'],0)
        self.assertEqual(pci.identify(self.c.db,'1113:0001')['device'][0]['name'],'Network controller')

    def test_exact_subsystem_precedes_generic_and_class_lookup_is_separate(self):
        pci.import_bundle(self.c,self.bundle())
        r=pci.identify(self.c.db,'1113:0001','18D4:0002','020000')
        self.assertEqual(r['subsystem'][0]['entry_kind'],'subsystem')
        self.assertEqual(r['subsystem_vendor'][0]['name'],'Celestica')
        self.assertEqual([x['entry_kind'] for x in r['class']],['class','subclass','prog_if'])
        r=pci.identify(self.c.db,'18d4:0001','18d4:0002')
        self.assertEqual(r['subsystem'][0]['entry_kind'],'generic_subsystem')
        self.assertEqual(pci.identify(self.c.db,'1113:ffff')['device'],[])
        for value in ['1113','1113:1','1113:0001:0002','1113:gggg']:
            with self.assertRaises(ValueError):pci.identify(self.c.db,value)
        self.assertEqual(len(pci.identify(self.c.db,'1113:0001',class_code='0200')['class']),2)
        with self.assertRaises(ValueError):pci.identify(self.c.db,'1113:0001',class_code='020')

    def test_invalid_hash_metadata_counts_and_unresolved_ids_preserve_import(self):
        p=self.bundle();pci.import_bundle(self.c,p)
        for key,value in [('commit','b'*40),('version','2020.01.01'),('expected_counts',{}),
                          ('undefined_subvendor_ids',[]),('ignored_extension_lines',[1]),
                          ('ids_url','https://example.org/pci.ids')]:
            data=json.loads(self.bundle().read_text());data[key]=value;p.write_text(json.dumps(data))
            with self.assertRaises(ValueError):pci.import_bundle(self.c,p)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM pci_entries').fetchone()[0],10)
        p=self.bundle();(self.root/'pci.ids').write_text('tampered')
        with self.assertRaises(ValueError):pci.import_bundle(self.c,p)

    def test_read_only_cli_old_schema_and_missing_database(self):
        path=self.root/'db.sqlite';before=path.read_bytes()
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(main(['--db',str(path),'pci','1113:0001','--format','json']),0)
        self.assertEqual(json.loads(out.getvalue())['state'],'not_loaded')
        self.assertEqual(path.read_bytes(),before)
        self.c.db.execute('DROP TABLE pci_entries');self.c.db.execute('PRAGMA user_version=12');self.c.db.commit()
        before=path.read_bytes()
        with contextlib.redirect_stdout(io.StringIO()):self.assertEqual(main(['--db',str(path),'pci','1113:0001']),0)
        self.assertEqual(path.read_bytes(),before)
        absent=self.root/'absent.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):self.assertEqual(main(['--db',str(absent),'pci','1113:0001']),1)
        self.assertFalse(absent.exists())

    def test_caller_transaction_and_schema_upgrade(self):
        self.c.db.execute('DROP TABLE pci_entries');self.c.db.execute('PRAGMA user_version=12');self.c.db.commit()
        other=Catalog(self.root/'db.sqlite')
        try:
            self.assertEqual(other.db.execute('PRAGMA user_version').fetchone()[0],29)
            pci.import_bundle(other,self.bundle())
            other.db.execute('BEGIN')
            pci.identify(other.db,'1113:0001')
            self.assertTrue(other.db.in_transaction)
            other.db.rollback()
        finally:other.db.close()

import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import pci, pci_report
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main
from test_pci import FIXTURE

REPORT = b'''Slot:\t00:01.0
Class:\t0200
Vendor:\t1113
Device:\t0001
SVendor:\t18d4
SDevice:\t0002
Rev:\t01
Module:\tone
Module:\ttwo
Driver:\tmodule_name
FutureTag:\tkeep this observation

Slot:\t0000:00:02.0
Device:\tffff
Vendor:\t1113
Class:\t0200
ProgIf:\t00

Slot:\t0001:00:02.0
Class:\t0200
Vendor:\tffff
Device:\t1234
'''


class PciReportTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        self.path=self.root/'db.sqlite';self.c=Catalog(self.path)
        self.c.source(pci.SOURCE,'https://example.org','community','fixture','fixture')
        rows,_,_=pci.parse(FIXTURE.encode())
        cols=['line_number','entry_kind','id_path','name','comments',*pci.IDS,'vendor_name','subvendor_name']
        self.c.db.executemany('INSERT INTO pci_entries (source_id,'+','.join(cols)+') VALUES ('+','.join('?' for _ in range(len(cols)+1))+')',
                             [(pci.SOURCE,*(r[k] for k in cols)) for r in rows])
        self.c.db.commit()

    def tearDown(self):
        self.c.db.close();self.temp.cleanup()

    def test_numeric_records_optional_fields_and_partial_class(self):
        parsed=pci_report.parse(REPORT)
        self.assertEqual(parsed[0]['slot'],'0000:00:01.0')
        self.assertEqual(parsed[0]['input_fields']['Module'],['one','two'])
        self.assertEqual(parsed[0]['uninterpreted_tags'],['FutureTag'])
        result=pci_report.analyze(self.c.db,REPORT)
        self.assertEqual(result['summary'],{'functions':3,'device_name_found':1,'vendor_only':1,'unknown_id':1,'not_loaded':0})
        first=result['results'][0]
        self.assertEqual(first['decoded']['subsystem'][0]['name'],'Board from subsystem namespace')
        self.assertEqual(first['class_code'],'0200')
        self.assertEqual([r['entry_kind'] for r in first['decoded']['class']],['class','subclass'])
        self.assertEqual(len(result['results'][1]['decoded']['class']),3)
        self.assertEqual(self.c.search(),[])

    def test_duplicate_slots_missing_fields_and_malformed_numeric_tags(self):
        first=REPORT.split(b'\n\n')[0]
        bad=[b'',first+b'\n\n'+first.replace(b'00:01.0',b'0000:00:01.0'),
             first.replace(b'SDevice:\t0002\n',b''),first.replace(b'Class:\t0200\n',b''),
             first.replace(b'1113',b'Accton [1113]'),first.replace(b'00:01.0',b'00:20.0'),
             first.replace(b'00:01.0',b'00:01.8'),first.replace(b'Vendor:\t',b'Vendor: '),
             first+b'\nVendor:\t1113',first+b'\nDevice:\t0002',
             first.replace(b'Rev:\t01',b'Rev:\t001')]
        for body in bad:
            with self.subTest(body=body):
                with self.assertRaises(ValueError):pci_report.parse(body)
        with patch('devicehunt.pci_report.pci.identify',side_effect=AssertionError('Lookup happened before input validation')):
            with self.assertRaises(ValueError):pci_report.analyze(self.c.db,REPORT+b'\n\nSlot:\t00:03.0')

    def test_limits_bom_crlf_and_multiple_functions_with_same_ids(self):
        self.assertEqual(len(pci_report.parse(b'\xef\xbb\xbf'+REPORT.replace(b'\n',b'\r\n'))),3)
        first=REPORT.split(b'\n\n')[0]
        repeat=first+b'\n\n'+first.replace(b'00:01.0',b'00:01.1')
        result=pci_report.analyze(self.c.db,repeat)
        self.assertEqual(result['summary']['device_name_found'],2)
        with patch.object(pci_report,'MAX_RECORDS',1):
            with self.assertRaises(ValueError):pci_report.parse(repeat)
        with patch.object(pci_report,'MAX_BYTES',10):
            with self.assertRaises(ValueError):pci_report.parse(REPORT)
        with self.assertRaises(ValueError):pci_report.parse(first+b'\nFutureTag:\t'+b'a'*4097)

    def test_read_only_json_csv_cli_and_no_network_or_execution(self):
        p=self.root/'inventory.txt';p.write_bytes(REPORT)
        before=self.path.read_bytes()
        with patch('urllib.request.urlopen',side_effect=AssertionError('Network used')):
            for fmt in ['json','csv','table']:
                with contextlib.redirect_stdout(io.StringIO()) as out:
                    self.assertEqual(main(['--db',str(self.path),'pci-report',str(p),'--format',fmt]),0)
                if fmt=='json':self.assertEqual(json.loads(out.getvalue())['summary']['functions'],3)
                if fmt=='csv':self.assertTrue(out.getvalue().startswith('slot,pci_id,status,'))
        self.assertEqual(self.path.read_bytes(),before)
        self.assertEqual(p.read_bytes(),REPORT)
        missing=self.root/'missing.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db',str(missing),'pci-report',str(p)]),1)
        self.assertFalse(missing.exists())

    def test_transaction_ownership_and_unloaded_old_database(self):
        self.c.db.execute('BEGIN')
        pci_report.analyze(self.c.db,REPORT)
        self.assertTrue(self.c.db.in_transaction)
        self.c.db.rollback()
        self.c.db.execute('DROP TABLE pci_entries');self.c.db.commit()
        before=self.path.read_bytes()
        self.assertEqual(pci_report.analyze(self.c.db,REPORT)['summary']['not_loaded'],3)
        self.assertEqual(self.path.read_bytes(),before)

import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.__main__ import main


class ReadOnlyCatalogCommandTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        self.path=self.root/'catalog with spaces.sqlite'
        c=Catalog(self.path)
        c.source('vendor','https://example.org','vendor','test','test')
        self.did=c.add('vendor','MODEL-1','https://example.org','Celestica','MODEL-1','Celestica MODEL-1',
                       'switch',{},[('Celestica','brand')],facts=[('physical_description','2U rack mount')])
        c.finish('vendor',1);c.db.close()

    def tearDown(self):
        self.temp.cleanup()

    def call(self, path, args):
        out=io.StringIO();err=io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
            code=main(['--db',str(path),*args])
        return code,out.getvalue(),err.getvalue()

    def commands(self):
        return [['search','MODEL-1','--format','json'],['specs','MODEL-1','--exact-model','--format','json'],
                ['show',self.did],['shopping',self.did],['stats'],['anatel','MODEL-1'],
                ['ntra','MODEL-1'],['grantees','Celestica'],['registry','MODEL-1'],
                ['export',str(self.root/'export.csv')],['report',str(self.root/'report.html')]]

    def test_queries_and_reports_preserve_database_and_return_actual_records(self):
        before=self.path.read_bytes()
        for args in self.commands():
            code,out,err=self.call(self.path,args)
            self.assertEqual((code,err),(0,''),args)
            self.assertEqual(self.path.read_bytes(),before,args)
            if args[0] in ('search','specs'):
                self.assertEqual(json.loads(out)[0]['model'],'MODEL-1')
        self.assertIn('MODEL-1',(self.root/'export.csv').read_text())
        self.assertIn('MODEL-1',(self.root/'report.html').read_text())
        with Catalog(self.path,read_only=True).db as db:
            with self.assertRaises(sqlite3.OperationalError):db.execute('DELETE FROM devices')
        db.close()

    def test_missing_database_fails_without_creating_path_or_reports(self):
        absent=self.root/'missing folder'/'missing.sqlite'
        for args in self.commands():
            code,_,err=self.call(absent,args)
            self.assertNotEqual(code,0,args);self.assertIn('unable to open database',err)
            self.assertFalse(absent.parent.exists())
        self.assertFalse((self.root/'export.csv').exists())
        self.assertFalse((self.root/'report.html').exists())

    def test_reads_do_not_upgrade_old_schema_but_explicit_init_does(self):
        with sqlite3.connect(self.path) as db:
            db.execute('DROP TABLE bsmi_declarations');db.execute('PRAGMA user_version=15')
        before=self.path.read_bytes()
        self.assertEqual(self.call(self.path,['search','MODEL-1'])[0],0)
        self.assertEqual(self.path.read_bytes(),before)
        with sqlite3.connect(self.path) as db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0],15)
            self.assertIsNone(db.execute("SELECT name FROM sqlite_master WHERE name='bsmi_declarations'").fetchone())
        self.assertEqual(self.call(self.path,['init'])[0],0)
        with sqlite3.connect(self.path) as db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0],29)
            self.assertIsNotNone(db.execute("SELECT name FROM sqlite_master WHERE name='bsmi_declarations'").fetchone())

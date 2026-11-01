"""Curated source bundles are atomic while retaining legacy literal/order behavior."""
import hashlib
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from devicehunt.catalog import Catalog
from devicehunt.importers import curated, prepare_curated


def source(sid, **extra):
    return dict(id=sid, url='https://example.test/' + str(sid), tier='vendor',
                license='fixture', coverage='fixture', **extra)


def row(sid, key, model, **extra):
    return dict(source=sid, key=key, brand='Fixture', model=model,
                title=model, kind='fixture', affiliations=[], **extra)


def legacy_curated(c, path):
    """Small reference for successful output, not a rollback implementation."""
    data = json.loads(path.read_text())
    count = 0
    for s in data['sources']:
        snapshot = path.parent / s['snapshot'] if s.get('snapshot') else None
        c.source(s['id'],s['url'],s['tier'],s['license'],s['coverage'],snapshot)
        rows = [r for r in data['devices'] if r['source'] == s['id']]
        for d in rows:
            c.add(s['id'],d['key'],d.get('url',s['url']),d['brand'],d['model'],d['title'],d['kind'],d,
                  d['affiliations'],d.get('identifiers',[]),d.get('facts',[]),d.get('hyperscaler',[]))
            count += 1
        c.finish(s['id'],len(rows))
    return count


class FaultConnection:
    def __init__(self, connection, predicate, error, once=True):
        self.connection, self.predicate, self.error = connection, predicate, error
        self.once, self.fired, self.statements = once, False, []

    def execute(self, sql, *args):
        self.statements.append(sql)
        if self.predicate(sql) and (not self.fired or not self.once):
            self.fired = True
            raise self.error
        return self.connection.execute(sql, *args)

    def __getattr__(self, name):
        return getattr(self.connection, name)


class CuratedImportTests(unittest.TestCase):
    TABLES = ('sources','devices','records','facts','identifiers','affiliations','hyperscaler_evidence','search_index')

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.c = self.catalog('current')
        clock = patch('devicehunt.catalog.now', return_value='2026-10-12T00:00:00+00:00')
        clock.start()
        self.addCleanup(clock.stop)
        for name in ('socket.socket.connect', 'socket.create_connection', 'subprocess.run', 'subprocess.Popen'):
            guard = patch(name, side_effect=AssertionError('network/external process forbidden'))
            guard.start()
            self.addCleanup(guard.stop)
        self.seed(self.c)

    def catalog(self, name):
        c = Catalog(self.root / (name + '.sqlite'))
        self.addCleanup(c.db.close)
        return c

    def save(self, data, name='input'):
        p = self.root / (name + '.json')
        p.write_text(json.dumps(data))
        return p

    def seed(self, c):
        p = self.save({'sources':[source('one'),source('two')], 'devices':[
            row('one','old','OldOne',facts=[['power_description','old power']]),
            row('two','old','OldTwo',identifiers=[['part_number','OLD']])]}, 'seed')
        legacy_curated(c,p)

    def snapshot(self, c=None):
        c = c or self.c
        return {t:[tuple(r) for r in c.db.execute('SELECT * FROM ' + t + ' ORDER BY 1,2')] for t in self.TABLES}

    def incoming(self):
        return {'sources':[source('one'),source('two')], 'devices':[
            row('one','new','NewOne',facts=[['power_description','new power']]),
            row('two','new','NewTwo')]}

    def test_late_missing_model_is_rejected_before_source_mutation(self):
        before = self.snapshot()
        data = self.incoming()
        del data['devices'][1]['model']
        with patch.object(self.c,'source',wraps=self.c.source) as write:
            with self.assertRaisesRegex(ValueError,r'devices\[1\].model'):
                curated(self.c,self.save(data))
            write.assert_not_called()
        self.assertEqual(self.snapshot(),before)
        self.assertFalse(self.c.db.in_transaction)

    def test_late_missing_snapshot_is_rejected_before_source_mutation(self):
        before = self.snapshot()
        data = self.incoming()
        (self.root/'first.bin').write_bytes(b'first snapshot')
        data['sources'][0]['snapshot']='first.bin'
        data['sources'][1]['snapshot']='missing.bin'
        with patch.object(self.c,'source',wraps=self.c.source) as write:
            with self.assertRaises(FileNotFoundError):
                curated(self.c,self.save(data))
            write.assert_not_called()
        self.assertEqual(self.snapshot(),before)

    def test_all_consumed_shapes_fail_before_mutation(self):
        before = self.snapshot()
        mutations = [lambda d:d.update(sources=None), lambda d:d.update(devices={}),
                     lambda d:d['sources'][1].pop('coverage'),
                     lambda d:d['sources'][1].update(snapshot=['bad']),
                     lambda d:d['devices'][1].update(model=1),
                     lambda d:d['devices'][1].update(affiliations=[['only-one']]),
                     lambda d:d['devices'][1].update(identifiers=[['part_number','X','extra']]),
                     lambda d:d['devices'][1].update(facts=[['bad-bindable-name'],1]),
                     lambda d:d['devices'][1].update(hyperscaler=[['operator','relationship']]),
                     lambda d:d['devices'].append({'missing_source':True})]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                data = self.incoming()
                mutate(data)
                with self.assertRaises(ValueError):
                    curated(self.c,self.save(data))
                self.assertEqual(self.snapshot(),before)

    def test_sql_programming_and_interrupt_failures_restore_bundle(self):
        before = self.snapshot()
        original = self.c.add
        for error in (sqlite3.IntegrityError('late SQL'),RuntimeError('adapter bug'),KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__):
                def failing(sid,*args,**kwargs):
                    if sid=='two':
                        raise error
                    return original(sid,*args,**kwargs)
                with patch.object(self.c,'add',side_effect=failing):
                    with self.assertRaises(type(error)) as raised:
                        curated(self.c,self.save(self.incoming()))
                    self.assertIs(raised.exception,error)
                self.assertEqual(self.snapshot(),before)
                self.assertFalse(self.c.db.in_transaction)

    def test_final_reindex_failure_restores_sources_and_search(self):
        before = self.snapshot()
        original = self.c.reindex
        def broken():
            original()
            raise RuntimeError('after FTS replacement')
        with patch.object(self.c,'reindex',side_effect=broken):
            with self.assertRaisesRegex(RuntimeError,'FTS replacement'):
                curated(self.c,self.save(self.incoming()))
        self.assertEqual(self.snapshot(),before)

    def test_release_failure_rolls_back_without_masking_original(self):
        before = self.snapshot()
        real = self.c.db
        for error in (sqlite3.OperationalError('release failure'),KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__):
                proxy = FaultConnection(real,lambda sql:sql.startswith('RELEASE'),error)
                with patch.object(self.c,'db',proxy):
                    with self.assertRaises(type(error)) as raised:
                        curated(self.c,self.save(self.incoming()))
                self.assertIs(raised.exception,error)
                self.assertEqual(self.snapshot(),before)
                self.assertFalse(real.in_transaction)

    def test_failed_rollback_never_releases_or_masks_original(self):
        class LockedError(RuntimeError):
            def __setattr__(self,name,value):
                raise RuntimeError('diagnostic assignment forbidden')
        before = self.snapshot()
        original = LockedError('original')
        real = self.c.db
        proxy = FaultConnection(real,lambda sql:sql.startswith('ROLLBACK TO'),sqlite3.OperationalError('rollback failure'))
        with patch.object(self.c,'db',proxy), patch.object(self.c,'reindex',side_effect=original):
            with self.assertRaises(LockedError) as raised:
                curated(self.c,self.save(self.incoming()))
        self.assertIs(raised.exception,original)
        self.assertFalse(any(s.startswith('RELEASE') for s in proxy.statements))
        self.assertTrue(real.in_transaction)
        real.rollback()  # Connection cannot be reused after a failed cleanup without owner recovery.
        self.assertEqual(self.snapshot(),before)

    def test_standalone_success_commits_and_reindexes_once(self):
        statements=[]
        self.c.db.set_trace_callback(statements.append)
        with patch.object(self.c,'reindex',wraps=self.c.reindex) as index:
            self.assertEqual(curated(self.c,self.save(self.incoming())),2)
            self.assertEqual(index.call_count,1)
        self.c.db.set_trace_callback(None)
        self.assertFalse(self.c.db.in_transaction)
        self.assertEqual(sum(s=='SAVEPOINT devicehunt_curated_import' for s in statements),1)
        self.assertEqual(sum(s=='RELEASE devicehunt_curated_import' for s in statements),1)
        with closing(sqlite3.connect(self.c.path)) as reader:
            self.assertEqual(reader.execute('SELECT COUNT(*) FROM records').fetchone()[0],2)
            self.assertEqual({r[0] for r in reader.execute('SELECT model FROM devices')},{'NewOne','NewTwo'})

    def test_nested_success_remains_caller_owned_and_invisible_until_commit(self):
        before = self.snapshot()
        self.c.db.execute("INSERT INTO sources(id,url,tier,license,coverage) VALUES('caller','u','t','l','c')")
        with closing(sqlite3.connect(self.c.path)) as reader:
            curated(self.c,self.save(self.incoming()))
            self.assertTrue(self.c.db.in_transaction)
            self.assertEqual({r[0] for r in reader.execute('SELECT model FROM devices')},{'OldOne','OldTwo'})
        self.c.db.rollback()
        self.assertEqual(self.snapshot(),before)
        self.c.db.execute("INSERT INTO sources(id,url,tier,license,coverage) VALUES('caller','u','t','l','c')")
        curated(self.c,self.save(self.incoming()))
        self.c.db.commit()
        with closing(sqlite3.connect(self.c.path)) as reader:
            self.assertEqual({r[0] for r in reader.execute('SELECT model FROM devices')},{'NewOne','NewTwo'})

    def test_nested_failure_preserves_prior_pending_caller_work(self):
        self.c.db.execute("INSERT INTO sources(id,url,tier,license,coverage) VALUES('caller','u','t','l','c')")
        before = self.snapshot()
        with patch.object(self.c,'reindex',side_effect=KeyboardInterrupt()):
            with self.assertRaises(KeyboardInterrupt):
                curated(self.c,self.save(self.incoming()))
        self.assertTrue(self.c.db.in_transaction)
        self.assertEqual(self.snapshot(),before)
        self.c.db.rollback()
        self.assertFalse(self.c.db.execute("SELECT 1 FROM sources WHERE id='caller'").fetchone())

    def test_per_source_orphan_cleanup_preserves_metadata_move(self):
        old=row('one','key','Move')
        old.update(title='old metadata',kind='old kind')
        new=row('two','key','Move')
        new.update(title='new metadata',kind='new kind')
        legacy=self.catalog('move-legacy')
        self.seed(legacy)
        for device in (old,new):
            path=self.save({'sources':[source('one'),source('two')],'devices':[device]})
            self.assertEqual(curated(self.c,path),legacy_curated(legacy,path))
            self.assertEqual(self.snapshot(),self.snapshot(legacy))
        self.assertEqual(tuple(self.c.db.execute('SELECT title,kind FROM devices').fetchone()),('new metadata','new kind'))
        self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT id,records_seen FROM sources ORDER BY id')],[('one',0),('two',1)])

    def test_successful_duplicate_order_raw_and_literals_equal_legacy(self):
        values=[None,'',0,False,1,True,[],{},['x'],{'x':1},1.5]
        first=row('one','same','Literal',facts=[['value',v] for v in values],
                  identifiers=[['part_number',v] for v in values])
        first.update(title='first identity title',kind='first kind',affiliations=[[None,{'skipped':1}],['Unknown','future_role']])
        last=json.loads(json.dumps(first))
        last.update(title='last raw record',revision={'ignored':[True,None]},raw_table_row={'number':1,'empty':[]})
        data={'sources':[source('one'),source('one'),source('two')],
              'devices':[first,last,{'source':['ignored'],'malformed_unconsumed':None}],
              'unresolved_rows':[{'arbitrary':True}]}
        legacy=self.catalog('legacy')
        self.seed(legacy)
        path=self.save(data)
        self.assertEqual(curated(self.c,path),legacy_curated(legacy,path))
        self.assertEqual(self.snapshot(),self.snapshot(legacy))
        self.assertEqual(self.c.db.execute("SELECT records_seen FROM sources WHERE id='one'").fetchone()[0],2)
        self.assertEqual(tuple(self.c.db.execute('SELECT title,kind,revision FROM devices').fetchone()),('first identity title','first kind',''))
        self.assertEqual(json.loads(self.c.db.execute('SELECT raw_json FROM records').fetchone()[0]),last)
        self.assertEqual([r['model'] for r in self.c.search('Literal')],['Literal'])

    def test_sql_scalar_coercions_and_json_iterable_entries_equal_legacy(self):
        data={'sources':[source('one')],'devices':[row('one',7,'Scalar')]}
        data['devices'][0].update(brand=123,title=0,kind=False,facts={'ab':{'ignored':1}},
                                  identifiers=['xy'],affiliations=['uv'],hyperscaler=[[1,1.5,True]])
        legacy=self.catalog('scalar-legacy')
        self.seed(legacy)
        path=self.save(data)
        curated(self.c,path)
        legacy_curated(legacy,path)
        self.assertEqual(self.snapshot(),self.snapshot(legacy))

    def test_empty_sources_is_noop_even_without_devices_in_caller_transaction(self):
        self.c.db.execute("INSERT INTO sources(id,url,tier,license,coverage) VALUES('caller','u','t','l','c')")
        before=self.snapshot()
        with patch.object(self.c,'reindex',side_effect=AssertionError('no reindex')):
            self.assertEqual(curated(self.c,self.save({'sources':[]})),0)
        self.assertTrue(self.c.db.in_transaction)
        self.assertEqual(self.snapshot(),before)

    def test_prepared_snapshots_are_read_once_and_payload_hash_is_ignored(self):
        data=self.incoming()
        (self.root/'snapshot.bin').write_bytes(b'immutable snapshot')
        data['sources'][0].update(snapshot='snapshot.bin',sha256='untrusted',original_sha256='untrusted')
        path=self.save(data)
        prepared=prepare_curated(path)
        with patch('devicehunt.importers.prepare_curated',return_value=prepared), patch.object(Catalog,'prepare_snapshot',side_effect=AssertionError('unexpected second read')):
            curated(self.c,path)
        actual=self.c.db.execute("SELECT sha256,cache_path FROM sources WHERE id='one'").fetchone()
        self.assertEqual(actual['sha256'],hashlib.sha256(b'immutable snapshot').hexdigest())
        self.assertEqual(actual['cache_path'],str(self.root/'snapshot.bin'))

    def test_catalog_source_and_finish_defaults_are_unchanged(self):
        snapshot=self.root/'snapshot.bin'
        snapshot.write_bytes(b'default snapshot')
        self.c.source('one','u','t','l','c',snapshot)
        self.c.add('one','new','u','Fixture','Default','Default','fixture',{})
        with patch.object(self.c,'reindex',wraps=self.c.reindex) as index:
            self.c.finish('one',1)
            self.assertEqual(index.call_count,1)
        self.assertFalse(self.c.db.in_transaction)
        self.assertEqual(self.c.db.execute("SELECT sha256 FROM sources WHERE id='one'").fetchone()[0],hashlib.sha256(b'default snapshot').hexdigest())
        before=self.snapshot()
        with self.assertRaises(ValueError):
            self.c.source('one','u','t','l','c',snapshot,prepared_snapshot={'sha256':'untrusted'})
        self.assertEqual(self.snapshot(),before)


if __name__ == '__main__':
    unittest.main()

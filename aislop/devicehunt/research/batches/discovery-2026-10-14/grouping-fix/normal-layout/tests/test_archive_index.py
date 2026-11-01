"""Offline c1b intent recovery, process exclusion and selected indexed queries."""
import json
import multiprocessing
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import time
import unittest
from unittest.mock import patch
from contextlib import closing

from devicehunt.artifacts import Archive, ArchiveRecoveryError, canonical, digest
from devicehunt.campaign_state import LeaseBusy
from devicehunt.derivations import Derivations


def _network_guards():
    for name in ('socket.socket.connect','socket.create_connection','urllib.request.OpenerDirector.open','subprocess.run','subprocess.Popen'):
        patch(name,side_effect=AssertionError('network/external entrypoint forbidden')).start()


def _crash_writer(root, boundary):
    _network_guards()
    def fault(point):
        if point==boundary:
            os._exit(71)
    archive=Archive(root,fault=fault)
    archive.receipt('test',{'value':'frozen'},'stable')
    os._exit(0)


def _competing_writer(root, ready, start, output):
    _network_guards()
    archive=Archive(root)
    ready.put(True)
    start.wait(10)
    for _ in range(1000):
        try:
            archive.receipt('test',{'value':'same'},'stable')
            output.put((archive.get_receipt('stable'),archive.require_ready()['sequence']))
            archive.close()
            return
        except LeaseBusy:
            time.sleep(.005)
    raise RuntimeError('writer could not obtain publication lock')


def _held_rebuild(root, ready, release):
    _network_guards()
    def fault(point):
        if point=='after_generation_index':
            ready.put(True)
            if not release.wait(10):
                raise RuntimeError('rebuild release missing')
    Archive.rebuild(root,quiesced=True,decision='fixture rebuild',fault=fault)


def _legacy_fixture(root, workspace):
    root.mkdir()
    (root/'receipts').mkdir()
    sha=digest(b'legacy bytes')
    obj=root/'objects'/sha[:2]/sha
    obj.parent.mkdir(parents=True)
    obj.write_bytes(b'legacy bytes')
    registration={'id':'legacy-registration','kind':'artifact-registration','created_at':'unknown',
        'payload':dict(sha256=sha,size=12,scope='archive',path=obj.relative_to(root).as_posix(),acquired_at=None,collector_version=None,metadata_status='unknown')}
    denial={'id':'legacy-denial','kind':'legacy-capture','created_at':'registered later',
        'payload':dict(source_url='https://legacy.test/item',requested_url=None,http_status=403)}
    policy={'id':'legacy-policy','kind':'capture_policy','created_at':'unknown',
        'payload':dict(saved_failure={'url':'https://legacy.test/other','status':200,'error':'HTTP Error 403'})}
    wrappers=[registration,denial,policy]
    for wrapper in wrappers:
        # Whitespace/order are historical bytes, not a new canonical wrapper.
        (root/'receipts'/(wrapper['id']+'.json')).write_bytes(json.dumps(wrapper,indent=2).encode()+b'\n')
    with closing(sqlite3.connect(root/'index.sqlite')) as db, db:
        db.executescript('CREATE TABLE receipts(id TEXT PRIMARY KEY,kind TEXT,created_at TEXT,payload TEXT); CREATE TABLE artifacts(sha256 TEXT PRIMARY KEY,size INTEGER); CREATE TABLE locations(sha256 TEXT,scope TEXT,path TEXT,PRIMARY KEY(sha256,scope,path));')
        for w in wrappers:
            db.execute('INSERT INTO receipts VALUES(?,?,?,?)',(w['id'],w['kind'],w['created_at'],json.dumps(w['payload'])))
        db.execute('INSERT INTO artifacts VALUES(?,?)',(sha,12))
        db.execute('INSERT INTO locations VALUES(?,?,?)',(sha,'archive',obj.relative_to(root).as_posix()))
    return sha


class ArchiveIndexTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.workspace=Path(self.tmp.name)
        self.root=self.workspace/'archive'
        self.archive=Archive(self.root,create=True)
        for name in ('socket.socket.connect','socket.create_connection','urllib.request.OpenerDirector.open','subprocess.run','subprocess.Popen'):
            guard=patch(name,side_effect=AssertionError('network/external entrypoint forbidden'))
            guard.start()
            self.addCleanup(guard.stop)

    def tearDown(self):
        self.archive.close()
        self.tmp.cleanup()

    def _join(self, process):
        process.join(20)
        if process.is_alive():
            process.terminate()
            process.join()
            self.fail('worker did not stop')

    def test_I1_real_process_crashes_recover_exact_wrapper_at_every_boundary(self):
        ctx=multiprocessing.get_context('spawn')
        for boundary in ('after_intent','after_file','after_index','after_head','after_ack'):
            with self.subTest(boundary=boundary):
                root=self.workspace/boundary
                fixture=Archive(root,create=True)
                fixture.close()
                process=ctx.Process(target=_crash_writer,args=(str(root),boundary))
                process.start()
                self._join(process)
                self.assertEqual(process.exitcode,71)
                with closing(sqlite3.connect(root/'index.sqlite')) as db, db:
                    row=db.execute('SELECT wrapper FROM publication_intent').fetchone()
                frozen=bytes(row[0]) if row else (root/'receipts'/'stable.json').read_bytes()
                readonly=Archive(root,readonly=True)
                if boundary!='after_ack':
                    with self.assertRaises(ArchiveRecoveryError):
                        readonly.verified_receipt('stable')
                readonly.close()
                result=Archive.recover(root,decision='fixture crash recovery')
                self.assertEqual(result['sequence'],1)
                self.assertEqual((root/'receipts'/'stable.json').read_bytes(),frozen)
                recovered=Archive(root)
                recovered.receipt('test',{'value':'frozen'},'stable')
                self.assertEqual(recovered.require_ready()['sequence'],1)
                self.assertEqual(recovered.verified_receipt('stable')['wrapper_sha256'],digest(frozen))
                recovered.close()

    def test_I1_two_real_processes_share_one_frozen_stable_wrapper(self):
        self.archive.close()
        ctx=multiprocessing.get_context('spawn')
        ready,output,start=ctx.Queue(),ctx.Queue(),ctx.Event()
        workers=[ctx.Process(target=_competing_writer,args=(str(self.root),ready,start,output)) for _ in range(2)]
        for worker in workers:
            worker.start()
        self.assertTrue(ready.get(timeout=10))
        self.assertTrue(ready.get(timeout=10))
        start.set()
        rows=[output.get(timeout=10),output.get(timeout=10)]
        for worker in workers:
            self._join(worker)
            self.assertEqual(worker.exitcode,0)
        self.assertEqual(rows[0],rows[1])
        self.assertEqual(rows[0][1],1)
        self.archive=Archive(self.root)
        self.assertEqual(len(self.archive.receipts()),1)

    def test_I1_real_rebuild_excludes_public_writer_and_stale_handle(self):
        original=self.archive.receipt('test',{'prior':1},'prior')
        ctx=multiprocessing.get_context('spawn')
        ready,release=ctx.Queue(),ctx.Event()
        worker=ctx.Process(target=_held_rebuild,args=(str(self.root),ready,release))
        worker.start()
        try:
            self.assertTrue(ready.get(timeout=10))
            with self.assertRaises(LeaseBusy):
                self.archive.receipt('test',{'new':1},'new')
            with self.assertRaises(ArchiveRecoveryError):
                self.archive.require_ready()
        finally:
            release.set()
            self._join(worker)
        self.assertEqual(worker.exitcode,0)
        with self.assertRaises(ArchiveRecoveryError):
            self.archive.get_receipt(original)
        self.archive.close()
        self.archive=Archive(self.root)
        self.assertEqual(self.archive.get_receipt(original)['payload'],{'prior':1})
        self.archive.receipt('test',{'new':1},'new')
        self.assertEqual(len(self.archive.receipts()),2)

    def test_I1_missing_referenced_object_halts_pending_recovery(self):
        sha=self.archive.store_bytes(b'required')
        def fault(point):
            if point=='after_intent':
                raise RuntimeError('crash')
        self.archive.fault=fault
        with self.assertRaises(RuntimeError):
            self.archive.receipt('capture_result',{'url':'https://test/a','artifact_sha256':sha},'result')
        self.archive.object_path(sha).unlink()
        with self.assertRaises(ValueError):
            Archive.recover(self.root,decision='fixture')
        self.assertEqual(self.archive.readiness()['status'],'needs_recovery')
        self.assertTrue(self.archive.db.execute('SELECT 1 FROM publication_intent').fetchone())

    def test_I1_object_without_registration_remains_reported_orphan(self):
        self.archive.fault=lambda point: (_ for _ in ()).throw(RuntimeError('crash')) if point=='after_object' else None
        with self.assertRaises(RuntimeError):
            self.archive.store_bytes(b'orphan')
        self.assertEqual(len(self.archive.audit()['orphan_objects']),1)
        with self.assertRaises(ValueError):
            self.archive.resolve(digest(b'orphan'))

    def test_I1_intent_phase_sequence_and_indexed_wrapper_contradictions_fail(self):
        for corruption in ('unindexed-next','indexed-previous','indexed-row-sequence','indexed-other-wrapper'):
            with self.subTest(corruption=corruption):
                root=self.workspace/corruption
                fixture=Archive(root,create=True)
                boundary='after_intent' if corruption=='unindexed-next' else 'after_index'
                fixture.fault=lambda point: (_ for _ in ()).throw(RuntimeError('crash')) if point==boundary else None
                with self.assertRaises(RuntimeError):
                    fixture.receipt('test',{'x':1},'stable')
                with fixture.db:
                    if corruption=='unindexed-next':
                        fixture.db.execute('UPDATE index_meta SET sequence=1')
                    elif corruption=='indexed-previous':
                        fixture.db.execute('UPDATE index_meta SET sequence=0')
                    elif corruption=='indexed-row-sequence':
                        fixture.db.execute('UPDATE receipts SET sequence=0')
                    else:
                        wrapper=fixture._read_receipt(root/'receipts'/'stable.json')
                        wrapper['payload']={'x':2}
                        raw=canonical(wrapper)
                        (root/'receipts'/'stable.json').chmod(0o644)
                        (root/'receipts'/'stable.json').write_bytes(raw)
                        fixture.db.execute('UPDATE receipts SET wrapper=?,wrapper_sha256=?',(raw,digest(raw)))
                fixture.close()
                with self.assertRaises(ArchiveRecoveryError):
                    Archive.recover(root,decision='fixture')

    def test_I2_explicit_legacy_upgrade_preserves_exact_bytes_and_safety(self):
        root=self.workspace/'legacy'
        sha=_legacy_fixture(root,self.workspace)
        before={p.relative_to(root).as_posix():p.read_bytes() for p in root.rglob('*') if p.is_file() and p.name!='index.sqlite'}
        legacy=Archive(root,readonly=True)
        self.assertEqual(legacy.readiness()['status'],'legacy_readonly')
        self.assertTrue(legacy.audit()['ok'])
        self.assertEqual(legacy.resolve(sha).read_bytes(),b'legacy bytes')
        legacy.close()
        with self.assertRaises(ArchiveRecoveryError):
            Archive(root)
        result=Archive.upgrade(root,quiesced=True,decision='fixture explicit upgrade')
        upgraded=Archive(root)
        self.assertEqual(upgraded.require_ready()['generation'],result['generation'])
        for name,raw in before.items():
            self.assertEqual((root/name).read_bytes(),raw)
        refs=upgraded.safety_references()
        self.assertEqual(len(refs),2)
        self.assertTrue(all(r['sequence']==0 for r in refs))
        self.assertTrue(all(upgraded.safety_status(r['row'])==403 for r in refs))
        self.assertEqual(upgraded.safety_row(upgraded.get_receipt('legacy-denial'),'payload')['url'],'https://legacy.test/item')
        prior=upgraded.require_ready()['sequence']
        wrapper=upgraded.get_receipt('legacy-registration')
        upgraded.receipt(wrapper['kind'],wrapper['payload'],wrapper['id'])
        self.assertEqual(upgraded.require_ready()['sequence'],prior)
        upgraded.close()

    def test_I2_missing_corrupt_and_rolled_back_index_never_cache_miss(self):
        self.archive.receipt('test',{'x':1},'one')
        previous=(self.root/'index.sqlite').read_bytes()
        self.archive.receipt('test',{'x':2},'two')
        self.archive.close()
        (self.root/'index.sqlite').write_bytes(previous)
        readonly=Archive(self.root,readonly=True)
        with self.assertRaises(ArchiveRecoveryError):
            readonly.capture_candidates('absent')
        readonly.close()
        Archive.rebuild(self.root,quiesced=True,decision='fixture rollback recovery')
        (self.root/'index.sqlite').write_bytes(b'not SQLite')
        readonly=Archive(self.root,readonly=True)
        self.assertEqual(readonly.readiness()['status'],'needs_recovery')
        self.assertTrue(readonly.audit()['ok'])
        readonly.close()
        result=Archive.rebuild(self.root,quiesced=True,decision='fixture corrupt reconstruction')
        self.assertEqual(result['status'],'ready')
        self.assertEqual(len(list(self.root.glob('index.corrupt-*.sqlite'))),1)
        self.assertEqual(next(self.root.glob('index.corrupt-*.sqlite')).read_bytes(),b'not SQLite')
        (self.root/'index.sqlite').unlink()
        readonly=Archive(self.root,readonly=True)
        with self.assertRaises(ArchiveRecoveryError):
            readonly.get_receipt('one')
        readonly.close()
        with self.assertRaises(ArchiveRecoveryError):
            Archive(self.root,create=True)
        Archive.rebuild(self.root,quiesced=True,decision='fixture missing index')
        self.archive=Archive(self.root)
        self.assertEqual(len(self.archive.receipts()),2)

    def test_I2_newer_schema_is_not_downgraded(self):
        with self.archive.db:
            self.archive.db.execute('UPDATE index_meta SET version=99')
        before=(self.root/'index.sqlite').read_bytes()
        with self.assertRaises(ArchiveRecoveryError):
            Archive.rebuild(self.root,quiesced=True,decision='fixture')
        self.assertEqual((self.root/'index.sqlite').read_bytes(),before)

    def test_I2_bad_heads_and_unexplained_index_advance_require_explicit_rebuild(self):
        for problem in ('missing-head','corrupt-head','wrong-generation','index-ahead-without-intent'):
            with self.subTest(problem=problem):
                root=self.workspace/problem
                fixture=Archive(root,create=True)
                fixture.receipt('test',{'preserved':True},'stable')
                original=(root/'receipts'/'stable.json').read_bytes()
                if problem=='missing-head':
                    (root/'index-head.json').unlink()
                elif problem=='corrupt-head':
                    (root/'index-head.json').write_bytes(b'bad head')
                elif problem=='wrong-generation':
                    head=json.loads((root/'index-head.json').read_bytes())
                    head['generation']='wrong'
                    (root/'index-head.json').write_bytes(canonical(head))
                else:
                    with fixture.db:
                        fixture.db.execute('UPDATE index_meta SET sequence=sequence+1')
                with self.assertRaises(ArchiveRecoveryError):
                    fixture.require_ready()
                with self.assertRaises(ArchiveRecoveryError):
                    Archive.recover(root,decision='fixture')
                fixture.close()
                Archive.rebuild(root,quiesced=True,decision='fixture explicit full rebuild')
                self.assertEqual((root/'receipts'/'stable.json').read_bytes(),original)

    def test_readonly_readiness_proof_and_legacy_audit_never_take_writer_lock(self):
        self.archive.receipt('test',{'x':1},'stable')
        readonly=Archive(self.root,readonly=True)
        before={p.relative_to(self.root).as_posix():p.read_bytes() for p in self.root.rglob('*') if p.is_file()}
        with self.archive.publication_guard(),patch('devicehunt.campaign_state.FileLocks.acquire',side_effect=AssertionError('readonly must not lock')):
            readonly.require_ready()
            readonly.verified_receipt('stable')
            readonly.audit()
        readonly.close()
        after={p.relative_to(self.root).as_posix():p.read_bytes() for p in self.root.rglob('*') if p.is_file()}
        self.assertEqual(before,after)

    def test_I2_generation_commit_before_head_is_recoverable(self):
        identifier=self.archive.receipt('test',{'x':1},'stable')
        original=(self.root/'receipts'/'stable.json').read_bytes()
        def fault(point):
            if point=='after_generation_index':
                raise RuntimeError('generation crash')
        with self.assertRaises(RuntimeError):
            Archive.rebuild(self.root,quiesced=True,decision='fixture',fault=fault)
        with self.assertRaises(ArchiveRecoveryError):
            self.archive.require_ready()
        result=Archive.recover(self.root,decision='fixture generation recovery')
        self.assertEqual(result['status'],'ready')
        self.archive.close()
        self.archive=Archive(self.root)
        self.assertEqual(self.archive.get_receipt(identifier)['payload'],{'x':1})
        self.assertEqual((self.root/'receipts'/'stable.json').read_bytes(),original)

    def test_S1_open_and_point_lookup_do_not_enumerate_unrelated_files(self):
        target=self.archive.receipt('capture_result',{'url':'https://test/a','request_id':'target','outcome':'unavailable'},'target')
        self.archive.receipt('test',{'unused':True},'unrelated')
        (self.root/'receipts'/'unrelated.json').chmod(0o644)
        (self.root/'receipts'/'unrelated.json').write_bytes(b'corrupt unrelated wrapper')
        self.archive.close()
        with patch.object(Path,'glob',side_effect=AssertionError('no glob')),patch.object(Path,'rglob',side_effect=AssertionError('no rglob')),patch.object(Path,'iterdir',side_effect=AssertionError('no iterdir')):
            readonly=Archive(self.root,readonly=True)
            self.assertEqual(readonly.get_receipt(target)['id'],target)
            self.assertEqual(readonly.capture_candidates('target')[0]['id'],target)
            readonly.verified_receipt(target)
            readonly.close()
        self.archive=Archive(self.root)

    def test_S1_selected_proof_hashes_only_required_artifact_closure(self):
        import devicehunt.artifacts as module
        selected=self.archive.store_bytes(b'selected')
        unrelated=self.archive.store_bytes(b'unrelated')
        receipt=self.archive.receipt('capture_result',{'artifact_sha256':selected},'result')
        self.archive.object_path(unrelated).unlink()
        real=module.file_digest
        reads=[]
        def record(path):
            reads.append(Path(path).name)
            return real(path)
        with patch.object(module,'file_digest',side_effect=record):
            self.archive.verified_receipt(receipt)
        self.assertEqual(reads,[selected])

    def test_S2_admission_order_context_and_saved_unknown_remain_separate(self):
        for identifier,sequence,wall in [('first',10,100),('second',11,50)]:
            self.archive.receipt('capture_result',dict(request_id='same',url='https://test/a',outcome='success',request_context_known=True,source='s',session_scope='session',observation_scope='slot',admission_sequence=sequence,ended_at=wall),identifier)
        self.archive.receipt('capture_result',dict(request_id='same',url='https://test/a',outcome='success',request_context_known=False),'later-saved')
        self.assertEqual([r['id'] for r in self.archive.capture_candidates('same')],['later-saved','first','second'])
        self.assertEqual([r['id'] for r in self.archive.capture_candidates('same',source='s',session_scope='session',observation_scope='slot',live=True)],['first','second'])
        self.assertEqual(self.archive.capture_candidates('same',source='s',session_scope='other',observation_scope='slot',live=True),[])
        self.assertEqual(self.archive.capture_candidates('same',source='s',session_scope='session',observation_scope='after',live=True),[])

    def test_selected_lookup_projection_corruption_is_not_trusted(self):
        identifier=self.archive.receipt('capture_result',dict(request_id='real',url='https://test/a',status=403,outcome='blocked'),'denied')
        with self.archive.db:
            self.archive.db.execute("UPDATE capture_lookup SET request_id='fake'")
        with self.assertRaises(ArchiveRecoveryError):
            self.archive.capture_candidates('fake')
        with self.archive.db:
            self.archive.db.execute("UPDATE safety_lookup SET row='{}'")
        with self.assertRaises(ArchiveRecoveryError):
            self.archive.safety_references()
        self.assertEqual(self.archive.get_receipt(identifier)['payload']['status'],403)

    def test_indexed_derivation_lookup_does_not_enumerate(self):
        sha=self.archive.store_bytes(canonical({'pages':[{'text':'saved'}]}))
        engine=Derivations(self.archive)
        recipe=engine.recipe('page-text-flatten','1')
        run=engine.run(recipe,[{'role':'pages','sha256':sha}])
        with patch.object(self.archive,'receipts',side_effect=AssertionError('no enumeration')):
            self.assertEqual(engine.run(recipe,[{'role':'pages','sha256':sha}]),run)
        with self.archive.db:
            self.archive.db.execute("UPDATE derivation_lookup SET recipe_id='fake'")
        with self.assertRaises(ArchiveRecoveryError):
            self.archive.derivation_candidates('fake',run['payload']['inputs'])

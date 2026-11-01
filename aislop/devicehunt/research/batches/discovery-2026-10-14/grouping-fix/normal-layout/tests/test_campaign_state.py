"""C1a contract gates. All archives isolated; only explicit mock dispatch occurs."""
from contextlib import closing
import hashlib
import json
import multiprocessing
from pathlib import Path
import shutil
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from devicehunt.artifacts import Archive, ArchiveRecoveryError
from devicehunt.campaign_state import CampaignState, FileLocks, LeaseBusy, StateError, StopCollection
from devicehunt.capture import Collector, Policy, Response, transport


class Clock:
    def __init__(self, wall=100, mono=10):
        self.wall, self.mono = wall, mono
    def advance(self, amount):
        self.wall += amount
        self.mono += amount


def _quota_worker(root, host, ready, go, output):
    """Spawn, so no inherited flock file descriptors authorize anything."""
    archive = Archive(root)
    state = CampaignState(root)
    url = 'https://' + host + '/q'
    def mocked(*args, observe_response):
        output.put(('dispatch',host))
        observe_response(200,url,{})
        return Response(200,url,{},b'x')
    collector = Collector(archive,Policy(frozenset([url]),budget=1,max_bytes=3),mocked,
                          state=state,account_id='a',mock_transport=True)
    ready.put(host)
    go.wait(10)
    try:
        result = collector.capture(url)
        output.put(('result',result['outcome']))
    except (StopCollection,ArchiveRecoveryError):
        output.put(('stopped',host))
    finally:
        archive.close()


def _held_worker(root, ready, release):
    state = CampaignState(root,clock=lambda:10)
    fences = state.acquire(['request:r','host:h'],'worker',120)
    publication = FileLocks(Path(root)/'capture-locks')
    publication.acquire(['publication'])
    ready.put(True)
    release.wait(10)
    publication.release(['publication'])
    state.release(fences,'worker')


def _fallback_boot_worker(output):
    """Spawn: fresh interpreter, /proc unreadable, same (faked) pid as the parent."""
    from devicehunt import campaign_state
    with patch('pathlib.Path.read_text',side_effect=OSError('no proc')), patch('os.getpid',return_value=4242):
        value = campaign_state._boot()
    output.put(value)  # outside the patches: multiprocessing queues consult the real pid


def _real_boot_worker(root, output):
    """Spawn: real /proc boot id and real clocks against an account another process created."""
    state = CampaignState(root)
    try:
        with state.db(True) as db:
            state._eligible(db,'real-window',new=True)
        output.put('admitted')
    except StopCollection as exc:
        output.put('stopped: '+str(exc))


class CampaignStateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.archive = Archive(Path(self.tmp.name)/'archive',create=True)
        self.clock = Clock()
        self.state = CampaignState.initialize(self.archive.root,clock=lambda:self.clock.wall,
            monotonic=lambda:self.clock.mono,boot='fixture-boot')
        self.urls = ['https://one.test/a','https://two.test/a','https://one.test/other']
        self.calls = []
        self.account(request_limit=10,byte_limit=100)
        self.state.synchronize_archive(self.archive,decision='fixture initial safety binding',policy_hash='capture-policy/1')
        for entry in ('socket.socket.connect','socket.create_connection','urllib.request.OpenerDirector.open','subprocess.run','subprocess.Popen'):
            guard = patch(entry,side_effect=AssertionError('forbidden network/external entrypoint'))
            guard.start()
            self.addCleanup(guard.stop)

    def tearDown(self):
        self.archive.close()
        self.tmp.cleanup()

    def account(self, name='a', **kwargs):
        self.state.allocate_account(name,definition_hash='definition',policy_hash='capture-policy/1',**kwargs)

    def collector(self, *, status=200, payload=b'body', headers=None, failure=None, account='a', **policy):
        def mocked(method,url,body,wire_headers,timeout,max_bytes,*,observe_response):
            self.calls.append(url)
            observe_response(status,url,headers or {})
            if failure:
                raise failure
            return Response(status,url,headers or {},payload)
        return Collector(self.archive,Policy(frozenset(self.urls),budget=1,max_bytes=3,**policy),mocked,
            state=self.state,account_id=account,mock_transport=True,clock=lambda:self.clock.wall,
            monotonic=lambda:self.clock.mono)

    def reservation(self, state=None, account='a', host='h', owner='o', url=None, fences=None):
        state = state or self.state
        keys = ['host:'+host,'request:'+host]
        fences = fences or state.acquire(keys,owner)
        row = state.reserve(account_id=account,run_id='run',unit_revision='unit-v1',request_id=host,
            url=url or 'https://'+host+'/a',host=host,owner=owner,fences=fences,max_body=4,archive=self.archive,transport_mode='mock')
        return row,fences

    def test_Q1_shared_quota_multiprocess_restart(self):
        # Separate allocation; workers share durable authority and different hosts.
        self.account('one',request_limit=1,byte_limit=4)
        # worker uses account 'a'; immutable a cannot be changed, use a fresh archive.
        root = Path(self.tmp.name)/'parallel'
        a = Archive(root,create=True)
        s = CampaignState.initialize(root)
        s.allocate_account('a',definition_hash='d',policy_hash='capture-policy/1',request_limit=1,byte_limit=4)
        s.synchronize_archive(a,decision='parallel fixture safety binding',policy_hash='capture-policy/1')
        a.close()
        ctx = multiprocessing.get_context('spawn')
        ready, output, go = ctx.Queue(),ctx.Queue(),ctx.Event()
        workers = []
        try:
            for host in ('alpha.test','beta.test'):
                worker = ctx.Process(target=_quota_worker,args=(str(root),host,ready,go,output))
                worker.start()
                workers.append(worker)
                self.assertEqual(ready.get(timeout=10),host)  # sequential ordinary archive opens
            go.set()
            results = [output.get(timeout=10) for _ in range(3)]
            for worker in workers:
                worker.join(10)
                self.assertEqual(worker.exitcode,0)
            self.assertEqual(sum(kind=='dispatch' for kind,value in results),1)
            restarted = CampaignState(root)
            self.assertEqual(restarted.account('a')['spent_requests'],1)
            self.assertEqual(restarted.account('a')['spent_bytes'],4)
            with self.assertRaises(StateError):
                restarted.allocate_account('a',definition_hash='d',policy_hash='capture-policy/1',request_limit=2,byte_limit=4)
            archive = Archive(root)
            url = 'https://gamma.test/q'
            def forbidden(*args,**kwargs):
                self.fail('restart replenished quota')
            c = Collector(archive,Policy(frozenset([url]),budget=99,max_bytes=3),forbidden,
                          state=restarted,account_id='a',run_id='new-run',mock_transport=True)
            with self.assertRaises(StopCollection):
                c.capture(url)
            archive.close()
        finally:
            go.set()
            for worker in workers:
                worker.join(2)
                if worker.is_alive():
                    worker.terminate()
                    worker.join()

    def test_Q2_bytes_deadline_window_host_and_uncertain(self):
        self.account('bytes',request_limit=5,byte_limit=7)
        c = self.collector(account='bytes',payload=b'x')
        c.capture(self.urls[0])
        with self.assertRaises(StopCollection):
            c.capture(self.urls[1])
        self.assertEqual(self.state.account('bytes')['spent_bytes'],4)  # never actual=1 refund
        self.account('time',request_limit=5,byte_limit=100,deadline=105,window=5)
        c = self.collector(account='time',payload=b'x')
        c.capture(self.urls[0],refresh=True)
        self.clock.advance(5)
        restarted = CampaignState(self.archive.root,clock=lambda:self.clock.wall,monotonic=lambda:self.clock.mono,boot='fixture-boot')
        c.state = restarted
        with self.assertRaises(StopCollection):
            c.capture(self.urls[1],refresh=True)
        self.account('host',request_limit=5,byte_limit=100,host_limits={'one.test':(1,4)})
        c = self.collector(account='host',payload=b'x')
        c.capture(self.urls[0],refresh=True)
        with self.assertRaises(StopCollection):
            c.capture(self.urls[2],refresh=True)
        self.collector(failure=TimeoutError('no refund')).capture(self.urls[1])
        self.assertEqual(self.state.account('a')['spent_requests'],1)
        c = self.collector(payload=b'x')
        with self.assertRaises(StopCollection):
            c.capture(self.urls[0],refresh=True)

    def test_Q2_clock_and_boot_anomaly(self):
        self.account('clock',request_limit=3,byte_limit=100,deadline=200)
        self.account('jump',request_limit=3,byte_limit=100,deadline=200)
        self.clock.wall += 30
        with self.assertRaises(StopCollection):
            self.reservation(account='jump',host='jump')
        # failed admission still holds manually acquired locks; release separately
        self.state.release(dict((k,v[1]) for k,v in self.state.lock_owners.items()),'o')
        self.clock.wall -= 30
        other = CampaignState(self.archive.root,clock=lambda:self.clock.wall,monotonic=lambda:self.clock.mono,boot='new-boot')
        with self.assertRaises(StopCollection):
            self.reservation(state=other,account='jump',host='restart')
        other.release(dict((k,v[1]) for k,v in other.lock_owners.items()),'o')
        row,fences = self.reservation(account='clock')
        self.state.uncertain(row['id'],'o',fences)
        self.state.release(fences,'o')
        self.assertEqual(self.state.account('clock')['spent_requests'],1)


    def test_Q3_fault_boundaries_no_uncommitted_or_unpublished_dispatch(self):
        points = ['before_reservation','after_reservation','before_attempt_publication','after_attempt_publication',
                  'before_dispatch_admission','after_dispatch_admission','before_transport','after_transport',
                  'before_result_publication','after_result_publication']
        for index, point in enumerate(points):
            with self.subTest(point=point):
                if index:
                    self.archive.close()
                    self.archive = Archive(Path(self.tmp.name)/('fault-archive-'+str(index)),create=True)
                    self.state = CampaignState.initialize(self.archive.root)
                    self.state.synchronize_archive(self.archive,decision='fault fixture binding',policy_hash='capture-policy/1')
                name = 'fault-'+str(index)
                self.account(name,request_limit=3,byte_limit=100)
                c = self.collector(account=name,payload=b'x')
                def fail(where):
                    if where == point:
                        raise RuntimeError(point)
                c.fault = fail
                self.state.fault = fail
                previous = len(self.calls)
                with self.assertRaises(RuntimeError):
                    c.capture(self.urls[0],refresh=True)
                self.state.fault = lambda where:None
                account = self.state.account(name)
                self.assertEqual(account['spent_requests'],0 if point=='before_reservation' else 1)
                if point not in ('after_transport','before_result_publication','after_result_publication'):
                    self.assertEqual(len(self.calls),previous)
                with self.state.db() as db:
                    rows = list(db.execute('SELECT * FROM reservations WHERE account_id=?',(name,)))
                if rows:
                    self.assertEqual(rows[0]['state'],'uncertain')
                    if len(self.calls)>previous:
                        attempt = self.archive.get_receipt(rows[0]['attempt_id'])
                        self.assertEqual(attempt['payload']['reservation_id'],rows[0]['id'])
                    fresh = self.collector(account=name,payload=b'x')
                    with self.assertRaises(StopCollection):
                        fresh.capture(self.urls[1],refresh=True)
                    self.assertEqual(self.state.account(name)['spent_requests'],1)

    def test_Q3_no_callback_missing_attempt_cancel_before_dispatch(self):
        c = self.collector(payload=b'x')
        def bad(*args,**kwargs):
            return Response(200,self.urls[0],{},b'x')
        c.transport = bad
        with self.assertRaises(StateError):
            c.capture(self.urls[0])
        self.assertEqual(self.state.account('a')['spent_requests'],1)
        self.archive.close()
        self.archive = Archive(Path(self.tmp.name)/'cancel-archive',create=True)
        self.state = CampaignState.initialize(self.archive.root)
        self.state.synchronize_archive(self.archive,decision='cancel fixture binding',policy_hash='capture-policy/1')
        self.account('cancel',request_limit=3,byte_limit=100)
        c = self.collector(account='cancel',payload=b'x')
        c.fault = lambda where: self.state.cancel('cancel',decision='fixture-review') if where=='before_dispatch_admission' else None
        with self.assertRaises(StopCollection):
            c.capture(self.urls[1])
        self.assertEqual(self.calls,[])  # bad transport above did not call recorded mock

    def test_R1_status_survives_body_storage_publication_interrupt(self):
        for status in (402,403,429,503):
            for failure in ('oversize','timeout','object','result','interrupt'):
                with self.subTest(status=status,failure=failure):
                    # Fresh archive/source/authority for each fault outcome.
                    root = Path(self.tmp.name)/('r-'+str(status)+'-'+failure)
                    archive = Archive(root,create=True)
                    state = CampaignState.initialize(root)
                    state.allocate_account('a',definition_hash='d',policy_hash='capture-policy/1',request_limit=10,byte_limit=100)
                    state.synchronize_archive(archive,decision='status fixture binding',policy_hash='capture-policy/1')
                    url = 'https://fault.test/a'
                    calls = []
                    def mocked(*args,observe_response):
                        calls.append(True)
                        observe_response(status,url,{})
                        if failure=='timeout':
                            raise TimeoutError('unread body')
                        if failure=='interrupt':
                            raise KeyboardInterrupt()
                        return Response(status,url,{},b'oversized')
                    c = Collector(archive,Policy(frozenset([url,url+'/alternate']),budget=1,max_bytes=3,source='source'),mocked,
                                  state=state,account_id='a',mock_transport=True)
                    if failure=='object':
                        c._store_bytes = lambda payload: (_ for _ in ()).throw(OSError('object fault'))
                    if failure=='result':
                        c.fault = lambda p: (_ for _ in ()).throw(RuntimeError('result fault')) if p=='before_result_publication' else None
                    if failure in ('result','interrupt'):
                        with self.assertRaises((RuntimeError,KeyboardInterrupt)):
                            c.capture(url,resource='item')
                    else:
                        row = c.capture(url,resource='item')
                        self.assertEqual(row['outcome'],'blocked' if status in (402,403) else 'throttled')
                    reopened = CampaignState(root)
                    self.assertTrue(reopened.restrictions(url,source='source',resource='item',host='fault.test'))
                    c = Collector(archive,Policy(frozenset([url,url+'/alternate']),budget=1,max_bytes=3,source='source'),mocked,
                                  state=reopened,account_id='a',mock_transport=True)
                    try:
                        row = c.capture(url+'/alternate',resource='item',refresh=True)
                        self.assertEqual(row['outcome'],'blocked')
                    except StopCollection:
                        pass
                    self.assertEqual(len(calls),1)
                    archive.close()

    def test_R1_status_persistence_fault_remains_charged_nonterminal(self):
        c = self.collector(status=403,payload=b'x')
        original = self.state.observe
        def fails(*args,**kwargs):
            raise StateError('disk failure before status transaction')
        self.state.observe = fails
        with self.assertRaises(StateError):
            c.capture(self.urls[0])
        self.state.observe = original
        self.assertEqual(self.state.account('a')['spent_requests'],1)
        with self.assertRaises(StopCollection):
            self.collector().capture(self.urls[1],refresh=True)
        self.assertEqual(len(self.calls),1)

    def test_R2_resume_exact_triggers_policy_time_quota_no_denial_clear(self):
        c = self.collector(status=403,payload=b'x',source='s')
        c.capture(self.urls[0],resource='denied')
        c = self.collector(status=429,payload=b'x',source='s',throttle_seconds=5)
        c.capture(self.urls[2],resource='other')
        events = self.state.restrictions(self.urls[2],source='s',host='one.test')
        triggers = [r['id'] for r in events if r['kind']=='throttle']
        arguments = dict(account_id='a',host='one.test',triggers=triggers,policy_hash='capture-policy/1',decision='reviewer')
        with self.assertRaises(StopCollection):
            self.state.resume_throttle(**arguments)
        self.clock.wall += 10
        with self.assertRaises(StopCollection):
            self.state.resume_throttle(**arguments)
        self.clock.wall -= 10
        self.clock.advance(5)
        with self.assertRaises(StateError):
            self.state.resume_throttle(**dict(arguments,policy_hash='other'))
        self.state.resume_throttle(**arguments)
        self.assertTrue(self.state.restrictions(self.urls[2],source='s',resource='denied',host='one.test'))
        resumed = self.collector(payload=b'x',source='s')
        resumed.capture(self.urls[2],resource='other',refresh=True)
        self.assertEqual(resumed.capture(self.urls[2],resource='denied',refresh=True)['outcome'],'blocked')
        self.collector(status=503,payload=b'x',source='s').capture(self.urls[2],refresh=True)
        with self.assertRaises(StateError):
            self.state.resume_throttle(**arguments)  # new throttle is not cleared
        self.account('exhausted',request_limit=0,byte_limit=0)
        with self.assertRaises(StopCollection):
            self.state.resume_throttle(**dict(arguments,account_id='exhausted'))
        source_stop = self.state.record_restriction(kind='deny',scope='source',target='s',provenance='source-policy')
        self.assertTrue(source_stop)
        self.assertEqual(self.collector(payload=b'x',source='s').capture(self.urls[1])['outcome'],'blocked')

    def test_L1_multiprocess_locks_clock_jumps_and_writer_exclusion(self):
        ctx = multiprocessing.get_context('spawn')
        ready, release = ctx.Queue(),ctx.Event()
        c = self.collector()
        worker = ctx.Process(target=_held_worker,args=(str(self.archive.root),ready,release))
        worker.start()
        try:
            self.assertTrue(ready.get(timeout=10))
            self.state.clock = lambda:100000000
            for keys in (['request:r'],['host:h']):
                with self.assertRaises(LeaseBusy):
                    self.state.acquire(keys,'intruder')
            count = len(self.archive.receipts())
            with self.assertRaises(LeaseBusy):
                c._receipt('test',{'value':1})
            self.assertEqual(len(self.archive.receipts()),count)
        finally:
            release.set()
            worker.join(10)
            if worker.is_alive():
                worker.terminate()
                worker.join()
        self.assertEqual(worker.exitcode,0)

    def test_L1_stale_owner_cannot_release_or_finalize_replacement(self):
        row,old = self.reservation(owner='old')
        self.state.uncertain(row['id'],'old',old)
        self.state.release(old,'old')
        new = self.state.acquire(list(old),'new')
        self.state.release(old,'old')
        other = CampaignState(self.archive.root)
        with self.assertRaises(LeaseBusy):
            other.acquire(list(old),'third')
        with self.assertRaises(StateError):
            self.state.transition(row['id'],'old',old,'uncertain','complete',archive=self.archive)
        with self.assertRaises(StateError):
            self.state.uncertain(row['id'],'old',old)
        self.state.release(new,'new')
        third = other.acquire(list(old),'third')
        self.assertGreater(third['host:h'],old['host:h'])
        other.release(third,'third')

    def test_M1_legacy_migration_lossless_transactional_backup(self):
        root = Path(self.tmp.name)/'legacy'
        root.mkdir()
        path = root/'capture-leases.sqlite'
        values = [('request:Exact CASE','live',123.456),('host:UPPER.example','expired',-1.0)]
        with closing(sqlite3.connect(path)) as db, db:
            db.execute('CREATE TABLE leases(key TEXT PRIMARY KEY,owner TEXT,expires REAL)')
            db.executemany('INSERT INTO leases VALUES (?,?,?)',values)
        with self.assertRaises(StateError):
            CampaignState.migrate(root,quiesced=True)
        def fault(point):
            if point=='migration_before_commit':
                raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            CampaignState.migrate(root,quiesced=True,recovery_decision='quiesced-review',fault=fault)
        with closing(sqlite3.connect(path)) as db, db:
            self.assertEqual(db.execute('SELECT * FROM leases ORDER BY key').fetchall(),sorted(values))
            self.assertEqual(db.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall(),[('leases',)])
        state = CampaignState.migrate(root,quiesced=True,recovery_decision='quiesced-review')
        with state.db() as db:
            self.assertEqual([tuple(r) for r in db.execute('SELECT * FROM leases ORDER BY key')],sorted(values))
        with closing(sqlite3.connect(root/'capture-leases.before-c1a.sqlite')) as db, db:
            self.assertEqual(db.execute('SELECT * FROM leases ORDER BY key').fetchall(),sorted(values))
        with self.assertRaises(LeaseBusy):
            state.acquire(['host:UPPER.example'],'new')  # expiry alone cannot prove dead owner
        state.recover_lease('host:UPPER.example',decision='known-stopped')
        fence = state.acquire(['host:UPPER.example'],'new')
        state.release(fence,'new')

    def test_M1_missing_corrupt_newer_readonly_quarantine_symlink(self):
        missing = Path(self.tmp.name)/'missing'
        missing.mkdir()
        before = list(missing.iterdir())
        with self.assertRaises(StateError):
            CampaignState(missing,readonly=True)
        self.assertEqual(list(missing.iterdir()),before)
        target = Path(self.tmp.name)/'target'
        target.mkdir()
        link = Path(self.tmp.name)/'linked'
        link.symlink_to(target,target_is_directory=True)
        with self.assertRaises(ValueError):
            CampaignState.initialize(link)
        self.assertEqual(list(target.iterdir()),[])
        with self.assertRaises(ValueError):
            CampaignState.initialize(target/'..'/'target')
        self.assertEqual(list(target.iterdir()),[])
        readonly = CampaignState(self.archive.root,readonly=True)
        before = {p.name:p.read_bytes() for p in self.archive.root.glob('*') if p.is_file()}
        readonly.account('a')
        with self.assertRaises(PermissionError):
            readonly.cancel('a',decision='no')
        self.assertEqual(before,{p.name:p.read_bytes() for p in self.archive.root.glob('*') if p.is_file()})
        self.state.quarantine(decision='explicit fixture restore')
        with self.assertRaises(StateError):
            self.collector().capture(self.urls[0])
        with closing(sqlite3.connect(self.state.path)) as db, db:
            db.execute('UPDATE operational_meta SET version=999')
        with self.assertRaises(StateError):
            CampaignState(self.archive.root)
        bad = Path(self.tmp.name)/'bad'
        bad.mkdir()
        (bad/'capture-leases.sqlite').write_bytes(b'corrupt')
        with self.assertRaises(StateError):
            CampaignState(bad,readonly=True)

    def test_P1_redaction_success_error_saved_policy_and_session_reuse(self):
        secrets = {'cOoKiE':'request-cookie-secret','AUTHORIZATION':'bearer-secret','Proxy-Authorization':'proxy-secret','X-Token':'source-secret'}
        response = {'sEt-CoOkIe':'response-cookie-secret','ETag':'safe'}
        c = self.collector(payload=b'x',headers=response,sensitive_headers=('x-token',),session_scope='session-1',reuse_policy='session')
        c.capture(self.urls[0],headers=secrets)
        c.capture(self.urls[0],headers=secrets)
        self.assertEqual(len(self.calls),1)
        self.collector(payload=b'x',headers=response,sensitive_headers=('x-token',),session_scope='session-2',reuse_policy='session').capture(self.urls[0],headers=secrets)
        self.assertEqual(len(self.calls),2)
        c = self.collector(status=403,payload=b'x',headers=response,sensitive_headers=('x-token',))
        c.capture(self.urls[1],headers=secrets)
        saved = Path(self.tmp.name)/'saved'
        saved.write_bytes(b'exact original body')
        row = {'url':self.urls[2],'status':200,'sha256':hashlib.sha256(saved.read_bytes()).hexdigest(),
               'headers':secrets,'nested':{'response_headers':response},'error':'echo request-cookie-secret source-secret'}
        c.register_saved(row,saved)
        c = self.collector(prior_failures=({'url':'https://unused.test','headers':secrets,'error':'echo proxy-secret'},),sensitive_headers=('x-token',))
        metadata = b'\n'.join(p.read_bytes() for p in (self.archive.root/'receipts').glob('*.json'))
        for secret in list(secrets.values())+[response['sEt-CoOkIe']]:
            self.assertNotIn(secret.encode(),metadata)
        self.assertEqual(saved.read_bytes(),b'exact original body')
        self.assertIn(b'safe',metadata)
        self.assertEqual(self.state.account('a')['spent_requests'],3)

    def test_P1_timeout_exception_does_not_echo_credentials(self):
        c = self.collector(failure=TimeoutError('Cookie secret-echo'),sensitive_headers=('X-Key',))
        c.capture(self.urls[0],headers={'Cookie':'secret-echo','X-Key':'key-secret'})
        persisted = b'\n'.join(p.read_bytes() for p in (self.archive.root/'receipts').glob('*.json'))
        self.assertNotIn(b'secret-echo',persisted)
        self.assertNotIn(b'key-secret',persisted)

    def test_admission_requires_real_scopes_fences_and_receipt_graph(self):
        arguments = dict(account_id='a',run_id='r',unit_revision='u',request_id='r',
                         url='https://h/a',host='h',owner='o',fences={},max_body=4,archive=self.archive,transport_mode='mock')
        with self.assertRaises(StateError):
            self.state.reserve(**arguments)
        fences = self.state.acquire(['host:h','request:r'],'o')
        with self.assertRaises(StateError):
            self.state.reserve(**dict(arguments,host='other',fences=fences))
        with self.assertRaises(StateError):
            self.state.reserve(**dict(arguments,owner='other',fences=fences))
        row = self.state.reserve(**dict(arguments,fences=fences))
        with self.assertRaises(StateError):
            self.state.transition(row['id'],'o',fences,'reserved','complete',archive=self.archive,result_id='missing')
        with self.assertRaises(StateError):
            self.state.transition(row['id'],'o',fences,'reserved','dispatch_admitted',archive=self.archive)
        with self.assertRaises(StateError):
            self.state.transition(row['id'],'o',fences,'reserved','attempt_published',archive=self.archive)
        with self.assertRaises(StateError):
            self.state.observe(row['id'],'o',fences,200,policy_hash='capture-policy/1')
        self.state.uncertain(row['id'],'o',fences)
        self.state.release(fences,'o')
        self.account('fresh',request_limit=100,byte_limit=10000)
        with self.assertRaises(StopCollection):
            self.collector(account='fresh',payload=b'x').capture(self.urls[1],refresh=True)
        self.assertEqual(self.calls,[])

    def test_legacy_resource_source_ingestion_precedes_publication(self):
        original = Collector._receipt
        def fail(*args,**kwargs):
            raise OSError('publication failure')
        row = {'url':self.urls[0],'status':403,'source':'legacy-source','resource':'exact-resource',
               'restriction_scope':'source'}
        with patch.object(Collector,'_receipt',fail):
            with self.assertRaises(OSError):
                self.collector(prior_failures=(row,))
        aliases = self.state.restrictions(self.urls[1],source='legacy-source',resource='exact-resource',host='two.test')
        self.assertEqual({r['scope'] for r in aliases},{'source','resource'})
        self.assertEqual(self.calls,[])

    def test_P1_validator_token_fragment_and_saved_diagnostic_redaction(self):
        c = self.collector(payload=b'x',sensitive_headers=('X-Token',))
        def validator(payload):
            raise ValueError('token abc123 invalid sid-value')
        c.capture(self.urls[0],headers={'Authorization':'Bearer abc123','Cookie':'sid=sid-value'},validator=validator)
        saved = {'url':self.urls[1],'status':404,'headers':{'Authorization':'Bearer xyz789','Cookie':'sid=cookie-value'},
                 'error':'token xyz789 invalid cookie-value'}
        c.register_saved(saved,locator={'note':'echo xyz789 cookie-value'})
        persisted = b'\n'.join(p.read_bytes() for p in (self.archive.root/'receipts').glob('*.json'))
        for value in (b'abc123',b'sid-value',b'xyz789',b'cookie-value'):
            self.assertNotIn(value,persisted)

    def test_P1_short_cookie_does_not_rewrite_structural_identity(self):
        payload = b'precise bytes'
        c = self.collector(payload=payload,headers={'ETag':'v1'})
        # The collector fixture max_bytes is three; use a legal response bound.
        c.policy.max_bytes = 20
        result = c.capture(self.urls[0],headers={'Cookie':'sid=1','Authorization':'Bearer x'})
        expected = hashlib.sha256(payload).hexdigest()
        self.assertEqual(result['artifact_sha256'],expected)
        self.assertEqual(self.archive.resolve(expected).read_bytes(),payload)
        row = self.state.reservation(result['reservation_id'])
        self.assertEqual(row['state'],'complete')
        self.assertEqual(row['result_id'],result['id'])
        attempt = self.archive.get_receipt(row['attempt_id'])['payload']
        self.assertEqual(attempt['reservation_id'],row['id'])
        self.assertEqual(attempt['request_id'],row['request_id'])
        self.assertEqual(attempt['url'],self.urls[0])
        self.assertEqual(result['headers'],{'ETag':'v1'})
        self.assertEqual(attempt['headers'],{})
        self.assertNotIn('Cookie',json.dumps(attempt))
        self.assertNotIn('Authorization',json.dumps(attempt))

    def test_P1_native_HTTPMessage_repeated_credentials_status_before_body(self):
        from http.client import HTTPMessage
        for index,status in enumerate((200,403)):
            with self.subTest(status=status):
                url = self.urls[index]
                native_headers = HTTPMessage()
                native_headers.add_header('Set-Cookie','sid=first-cookie-fragment; HttpOnly')
                native_headers.add_header('sEt-CoOkIe','other=second-cookie-fragment; Secure')
                native_headers.add_header('Authorization','Bearer native-bearer-fragment')
                native_headers.add_header('X-Token','native-source-secret')
                native_headers.add_header('ETag','safe-native-etag')
                test = self
                class NativeResponse:
                    code = status
                    headers = native_headers
                    closed = False
                    def __init__(self):
                        self.url = url
                        self.sent = False
                    def __enter__(self):
                        return self
                    def __exit__(self,*args):
                        pass
                    def read1(self,size):
                        with test.state.db() as db:
                            row = db.execute('SELECT * FROM reservations ORDER BY sequence DESC LIMIT 1').fetchone()
                        test.assertEqual(row['state'],'response_observed')
                        test.assertEqual(row['status'],status)
                        if status==403:
                            test.assertTrue(test.state.restrictions(url,host='two.test'))
                        if self.sent:
                            return b''
                        self.sent = True
                        return b'body'
                def native_fixture(*args,observe_response):
                    return transport(*args,observe_response=observe_response)
                collector = Collector(self.archive,Policy(frozenset([url]),budget=1,max_bytes=10,sensitive_headers=('x-token',)),
                    native_fixture,state=self.state,account_id='a',mock_transport=True)
                with patch('urllib.request.build_opener') as opener:
                    opener.return_value.open.return_value = NativeResponse()
                    result = collector.capture(url)
                self.assertEqual(result['outcome'],'success' if status==200 else 'blocked')
                self.assertEqual(result['headers'],{'ETag':'safe-native-etag'})
                for fragment in ('first-cookie-fragment','second-cookie-fragment','native-bearer-fragment','native-source-secret'):
                    self.assertIn(fragment,collector.secret_values)
                persisted = b'\n'.join(p.read_bytes() for p in (self.archive.root/'receipts').glob('*.json'))
                for fragment in (b'first-cookie-fragment',b'second-cookie-fragment',b'native-bearer-fragment',b'native-source-secret'):
                    self.assertNotIn(fragment,persisted)

    def test_transport_observes_HTTP_error_before_first_body_read(self):
        url = self.urls[0]
        events = []
        class HTTPResponse:
            code,url,headers = 403,self.urls[0],{'Set-Cookie':'private'}
            closed = False
            def __enter__(self):
                return self
            def __exit__(self,*args):
                pass
            def close(self):
                pass
            def read1(self,size):
                events.append('read')
                raise TimeoutError('body timeout')
        response = HTTPResponse()
        def observe(status,final_url,headers):
            events.append('status')
            self.assertEqual(status,403)
        with patch('urllib.request.build_opener') as opener:
            opener.return_value.open.return_value = response
            with self.assertRaises(TimeoutError):
                transport('GET',url,b'',{},1,3,observe_response=observe)
        self.assertEqual(events,['status','read'])
        events.clear()
        import urllib.error
        with patch('urllib.request.build_opener') as opener:
            opener.return_value.open.side_effect = urllib.error.HTTPError(url,403,'forbidden',response.headers,response)
            with self.assertRaises(TimeoutError):
                transport('GET',url,b'',{},1,3,observe_response=observe)
        self.assertEqual(events,['status','read'])
        events.clear()
        def fail_observation(*args):
            events.append('status')
            raise StateError('state storage fault')
        with patch('urllib.request.build_opener') as opener:
            opener.return_value.open.return_value = response
            with self.assertRaises(StateError):
                transport('GET',url,b'',{},1,3,observe_response=fail_observation)
        self.assertEqual(events,['status'])

    def test_missing_authority_cannot_reinitialize_beside_existing_receipts(self):
        self.collector(payload=b'x').capture(self.urls[0])
        self.state.path.unlink()  # isolated loss fixture, never a live data deletion
        with self.assertRaises(StateError):
            CampaignState(self.archive.root)
        with self.assertRaises(StateError):
            CampaignState.initialize(self.archive.root)
        self.assertFalse(self.state.path.exists())

    def test_unobserved_timeout_settles_only_by_explicit_decision_without_refund(self):
        def silent(method, url, body, wire_headers, timeout, max_bytes, *, observe_response):
            self.calls.append(url)
            raise TimeoutError('no response headers before the time bound')
        def collector(account):
            return Collector(self.archive, Policy(frozenset(self.urls), budget=1, max_bytes=3), silent,
                             state=self.state, account_id=account, mock_transport=True,
                             clock=lambda: self.clock.wall, monotonic=lambda: self.clock.mono)
        result = collector('a').capture(self.urls[0])
        self.assertNotEqual(result['outcome'], 'success')
        row = next(r for r in [self.state.reservation(result['reservation_id'])])
        self.assertEqual((row['state'], row['status']), ('uncertain', None))
        self.assertTrue(self.state.account('a')['stopped'])
        # Fail closed: an unrelated new account cannot be admitted while the attempt is open.
        self.account('b', request_limit=5, byte_limit=100)
        with self.assertRaisesRegex(StopCollection, 'unreconciled charged reservation'):
            self.collector(account='b').capture(self.urls[1])
        with self.assertRaises(StateError):  # no guessed result
            self.state.reconcile_result(row['id'], archive=self.archive, result_id=result['id'], decision='guess')
        with self.assertRaises(ValueError):
            self.state.settle_unobserved(row['id'], archive=self.archive, result_id=result['id'], decision='')
        spent = self.state.account('a')['spent_requests']
        self.state.settle_unobserved(row['id'], archive=self.archive, result_id=result['id'],
                                     decision='fixture: timed out before headers')
        self.state.settle_unobserved(row['id'], archive=self.archive, result_id=result['id'],
                                     decision='fixture: repeated')  # idempotent
        settled = self.state.reservation(row['id'])
        self.assertEqual((settled['state'], settled['status'], settled['result_id']), ('uncertain', None, None))
        self.assertEqual(self.state.account('a')['spent_requests'], spent)  # charge kept, no refund
        self.assertTrue(self.state.account('a')['stopped'])
        with self.assertRaises(StateError):
            self.state.resume_account(self.archive, 'a', decision='not a reconciled interruption')
        with closing(sqlite3.connect(self.state.path)) as db:
            self.assertEqual(db.execute("SELECT COUNT(*) FROM events WHERE provenance='unobserved-settlement'").fetchone()[0], 1)
        # A distinct, separately allocated attempt may now proceed.
        self.assertEqual(self.collector(account='b', payload=b'x').capture(self.urls[1])['outcome'], 'success')

    def test_settlement_refuses_attempts_with_an_observed_status(self):
        result = self.collector(failure=TimeoutError('body stalled')).capture(self.urls[0])
        row = self.state.reservation(result['reservation_id'])
        self.assertIsNotNone(row['status'])
        with self.assertRaisesRegex(StateError, 'without an observed status'):
            self.state.settle_unobserved(row['id'], archive=self.archive, result_id=result['id'], decision='x')

    def test_failures_after_headers_or_outside_transport_stay_fail_closed(self):
        # Status persistence fails after headers arrived: generic interruption, no result receipt.
        def headers_then_crash(method, url, body, wire_headers, timeout, max_bytes, *, observe_response):
            raise RuntimeError('persistence failed after headers')
        collector = Collector(self.archive, Policy(frozenset(self.urls), budget=1, max_bytes=3), headers_then_crash,
                              state=self.state, account_id='a', mock_transport=True,
                              clock=lambda: self.clock.wall, monotonic=lambda: self.clock.mono)
        with self.assertRaises(RuntimeError):
            collector.capture(self.urls[0])
        with closing(sqlite3.connect(self.state.path)) as db:
            rid = db.execute("SELECT id FROM reservations WHERE state='uncertain'").fetchone()[0]
        self.assertEqual(self.state.account('a')['stopped'], 'interrupted attempt')
        with self.assertRaisesRegex(StateError, 'pre-header transport failure'):
            self.state.settle_unobserved(rid, archive=self.archive, result_id='none', decision='x')
        with self.assertRaises(StateError):
            self.state.resume_account(self.archive, 'a', decision='no verified result')

    def test_relocation_moves_identity_keeps_authority_and_refuses_copies(self):
        self.collector(payload=b'x').capture(self.urls[0])
        self.archive.close()
        old = self.archive.root
        moved = old.parent/'moved'/'archive'
        marker = json.loads((old/'capture-authority.json').read_bytes())
        with closing(sqlite3.connect(self.state.path)) as db:
            accounts = db.execute('SELECT * FROM accounts').fetchall()
            events = db.execute('SELECT COUNT(*) FROM events').fetchone()[0]
        moved.parent.mkdir()
        old.rename(moved)
        with self.assertRaises(StateError):
            CampaignState(moved)  # path-bound identity: an unexplained move fails closed
        with self.assertRaises(StateError):
            CampaignState.relocate(moved,old_root=old,quiesced=True)
        shutil.copytree(moved,old)
        with self.assertRaisesRegex(StateError,'never a copy'):
            CampaignState.relocate(moved,old_root=old,quiesced=True,decision='fixture move')
        shutil.rmtree(old)
        with closing(sqlite3.connect(moved/'capture-leases.sqlite')) as db, db:
            db.execute("INSERT INTO leases VALUES ('host:h','o',1e12)")
        with self.assertRaisesRegex(StateError,'active leases'):
            CampaignState.relocate(moved,old_root=old,quiesced=True,decision='fixture move')
        with closing(sqlite3.connect(moved/'capture-leases.sqlite')) as db, db:
            db.execute('DELETE FROM leases')
        blockers = [("INSERT INTO campaign_runs VALUES ('r','c','d','{}','offline','running',0,0,1,'b',NULL,NULL)",
                     'DELETE FROM campaign_runs','running campaign'),
                    ("UPDATE reservations SET state='uncertain' WHERE rowid=(SELECT MIN(rowid) FROM reservations)",
                     "UPDATE reservations SET state='complete'",'unfinished reservations')]
        for block, unblock, message in blockers:
            with closing(sqlite3.connect(moved/'capture-leases.sqlite')) as db, db:
                self.assertEqual(db.execute(block).rowcount,1)
            with self.assertRaisesRegex(StateError,message):
                CampaignState.relocate(moved,old_root=old,quiesced=True,decision='fixture move')
            with closing(sqlite3.connect(moved/'capture-leases.sqlite')) as db, db:
                db.execute(unblock)
        def crash(at):
            def hook(point):
                if point == at:
                    raise KeyboardInterrupt(point)
            return hook
        # Rolled back before commit, then interrupted between marker and seal: each retry finishes.
        for point in ('relocate_before_commit','relocate_meta','relocate_marker'):
            with self.assertRaises(KeyboardInterrupt):
                CampaignState.relocate(moved,old_root=old,quiesced=True,decision='fixture move',fault=crash(point))
            with self.assertRaises(StateError):
                CampaignState(moved)
        state = CampaignState.relocate(moved,old_root=old,quiesced=True,decision='fixture move')
        again = CampaignState.relocate(moved,old_root=old,quiesced=True,decision='fixture move')
        relocated = json.loads((moved/'capture-authority.json').read_bytes())
        self.assertEqual(relocated,dict(marker,archive_id=CampaignState.identity(moved)))
        self.assertEqual(json.loads((moved/'capture-authority-ready.json').read_bytes()),
                         dict(version=1,archive_id=CampaignState.identity(moved),authority_id=marker['authority_id']))
        self.assertEqual((moved/'capture-authority.json').stat().st_mode & 0o777,0o444)
        with closing(sqlite3.connect(state.path)) as db:
            self.assertEqual(db.execute('SELECT * FROM accounts').fetchall(),accounts)
            rows = db.execute("SELECT provenance,decision FROM events WHERE sequence>?",(events,)).fetchall()
            self.assertEqual(rows,[('relocation','fixture move')])  # one event despite retries
            self.assertEqual(db.execute('SELECT archive_id FROM operational_meta').fetchone()[0],CampaignState.identity(moved))
        self.assertEqual(len(list(moved.glob('capture-leases.before-relocate-*.sqlite'))),1)
        archive = Archive(moved,Path(self.tmp.name))
        try:
            archive.require_ready()
            self.assertEqual(again.campaign_runs('none'),[])
        finally:
            archive.close()
        # Moving back and forth again gets fresh backups instead of conflicting with old ones.
        for source, target in ((moved,old),(old,moved)):
            source.rename(target)
            CampaignState.relocate(target,old_root=source,quiesced=True,decision='fixture move back and forth')
        self.assertEqual(len(list(moved.glob('capture-leases.before-relocate-*.sqlite'))),3)
        self.assertEqual(json.loads((moved/'capture-authority.json').read_bytes()),relocated)

    def test_lock_directory_symlink_fails_before_writes(self):
        target = Path(self.tmp.name)/'lock-target'
        target.mkdir()
        root = Path(self.tmp.name)/'lock-archive'
        root.mkdir()
        (root/'capture-locks').symlink_to(target,target_is_directory=True)
        with self.assertRaises(ValueError):
            FileLocks(root/'capture-locks')
        with self.assertRaises(ValueError):
            CampaignState.initialize(root)
        self.assertFalse((root/'capture-authority.json').exists())
        self.assertFalse((root/'capture-leases.sqlite').exists())
        with closing(sqlite3.connect(root/'capture-leases.sqlite')) as db, db:
            db.execute('CREATE TABLE leases(key TEXT PRIMARY KEY,owner TEXT,expires REAL)')
        with self.assertRaises(ValueError):
            CampaignState.migrate(root,quiesced=True)
        self.assertEqual(list(target.iterdir()),[])
        clean = root/'clean-locks'
        locks = FileLocks(clean)
        clean.symlink_to(target,target_is_directory=True)
        with self.assertRaises(ValueError):
            locks.acquire(['publication'])
        self.assertEqual(list(target.iterdir()),[])

    def test_result_reconciliation_preserves_charge_and_frozen_attempt(self):
        c = self.collector(payload=b'x')
        c.fault = lambda where: (_ for _ in ()).throw(RuntimeError('crash after result')) if where=='after_result_publication' else None
        with self.assertRaises(RuntimeError):
            c.capture(self.urls[0])
        result = self.archive.receipts('capture_result')[0]
        row = self.state.reservation(result['payload']['reservation_id'])
        self.assertEqual(row['state'],'uncertain')
        attempt_before = (self.archive.root/'receipts'/(row['attempt_id']+'.json')).read_bytes()
        completed = self.state.reconcile_result(row['id'],archive=self.archive,result_id=result['id'],decision='fixture-review')
        self.assertEqual(completed['state'],'complete')
        self.assertEqual(completed['owner'],row['owner'])
        self.assertEqual(completed['fences'],row['fences'])
        self.assertEqual((self.archive.root/'receipts'/(row['attempt_id']+'.json')).read_bytes(),attempt_before)
        self.assertEqual(self.state.account('a')['stopped'],'interrupted attempt')
        self.state.resume_account(self.archive,'a',decision='reviewed remaining finite work')
        self.assertIsNone(self.state.account('a')['stopped'])
        self.assertEqual(self.state.account('a')['spent_requests'],1)
        self.assertEqual(self.calls,[self.urls[0]])

    def test_host_canonicalization_default_ports_and_invalid_urls(self):
        from devicehunt.campaign_state import host_for_url
        self.assertEqual(host_for_url('https://HOST.test:443/a'),'host.test')
        self.assertEqual(host_for_url('http://host.test:80/a'),'host.test')
        self.assertEqual(host_for_url('https://host.test:8443/a'),'host.test')
        for url in ('ftp://host.test/a','https://user:password@host.test/a','https://host.test:bad/a'):
            with self.assertRaises(ValueError):
                host_for_url(url)
        first = 'https://alias.test/a'
        alias = 'https://ALIAS.test:443/a'
        calls = []
        def mocked(*args,observe_response):
            calls.append(True)
            observe_response(429,first,{})
            return Response(429,first,{},b'x')
        c = Collector(self.archive,Policy(frozenset([first,alias]),budget=1,max_bytes=3),mocked,
                      state=self.state,account_id='a',mock_transport=True)
        c.capture(first)
        with self.assertRaises(StopCollection):
            c.capture(alias)
        self.assertEqual(calls,[True])
        self.account('host-alias',request_limit=4,byte_limit=100,host_limits={'TWO.test:443':(0,0)})
        with self.assertRaises(StopCollection):
            self.collector(account='host-alias').capture(self.urls[1],refresh=True)
        self.assertEqual(self.state.account('host-alias')['spent_requests'],0)

    def test_lost_authority_after_reserved_before_receipt_cannot_reset_quota(self):
        row,fences = self.reservation(host='unpublished')
        self.assertEqual(self.archive.capture_candidates('unpublished'),[])
        self.assertEqual(self.archive.require_ready()['sequence'],0)
        marker_before = (self.archive.root/'capture-authority.json').read_bytes()
        # Simulate abrupt DB loss while all dispatch evidence is still absent.
        self.state.path.unlink()
        self.state.locks.release(fences)
        with self.assertRaises(StateError):
            CampaignState.initialize(self.archive.root)
        with self.assertRaises(StateError):
            CampaignState(self.archive.root,readonly=True)
        self.assertFalse(self.state.path.exists())
        self.assertEqual((self.archive.root/'capture-authority.json').read_bytes(),marker_before)

    def test_completed_legacy_migration_cannot_replay_old_backup_after_spend(self):
        root = Path(self.tmp.name)/'legacy-reset'
        archive = Archive(root,create=True)
        path = root/'capture-leases.sqlite'
        with closing(sqlite3.connect(path)) as db, db:
            db.execute('CREATE TABLE leases(key TEXT PRIMARY KEY,owner TEXT,expires REAL)')
        state = CampaignState.migrate(root,quiesced=True,recovery_decision='synthetic stopped legacy collectors')
        state.allocate_account('a',definition_hash='d',policy_hash='capture-policy/1',request_limit=2,byte_limit=10)
        state.synchronize_archive(archive,decision='synthetic safety binding',policy_hash='capture-policy/1')
        fences = state.acquire(['host:h','request:r'],'owner')
        state.reserve(account_id='a',run_id='run',unit_revision='u',request_id='r',url='https://h/a',host='h',
            owner='owner',fences=fences,max_body=4,archive=archive,transport_mode='mock')
        state.release(fences,'owner')
        # Original legacy snapshot has no account/reservation rows. Its exact
        # hash still matches marker, but completed authority must forbid replay.
        path.write_bytes((root/'capture-leases.before-c1a.sqlite').read_bytes())
        with self.assertRaises(StateError):
            CampaignState.migrate(root,quiesced=True,recovery_decision='unsafe old backup replay')
        self.assertTrue((root/'capture-authority-ready.json').is_file())
        archive.close()

    def test_known_denial_cannot_auto_ack_extra_broader_source_scope(self):
        result = self.collector(status=403,payload=b'x',source='source').capture(self.urls[0],resource='item')
        extra = dict(result)
        extra.pop('id')
        extra['restriction_scope']='source'
        self.archive.receipt('capture_result',extra)
        other = self.collector(payload=b'x',source='source')
        with self.assertRaises(StateError):
            other.capture(self.urls[1],refresh=True)
        self.assertEqual(len(self.calls),1)
        self.state.synchronize_archive(self.archive,decision='preserve broader saved restriction',policy_hash='capture-policy/1')
        self.assertEqual(other.capture(self.urls[1])['outcome'],'blocked')
        self.assertEqual(len(self.calls),1)

    def test_indexed_safety_generic_refs_block_and_explicit_sync_preserves_conflicts(self):
        legacy_id = self.archive.receipt('legacy-capture',dict(source_url=self.urls[0],http_status=403,
            source='legacy-source',resource='same-item'))
        with self.assertRaises(StateError):
            self.collector(payload=b'x').capture(self.urls[1])
        self.assertEqual(self.calls,[])
        self.assertEqual(self.state.account('a')['spent_requests'],0)
        synchronized = self.state.synchronize_archive(self.archive,decision='review legacy safety',policy_hash='capture-policy/1')
        self.assertTrue(synchronized['imported_reference_ids'])
        denied = self.state.restrictions(self.urls[1],source='legacy-source',resource='same-item',host='two.test')
        self.assertEqual({r['scope'] for r in denied},{'resource'})
        original = self.collector(payload=b'x').capture(self.urls[1])
        # A generic wrapper claiming a known observed 200 but carrying conflicting
        # saved error403 cannot be auto-acknowledged as known status handling.
        conflict = dict(original)
        conflict.pop('id')
        conflict['error'] = 'HTTP Error 403 in conflicting saved metadata'
        conflict_id = self.archive.receipt('capture_result',conflict)
        with self.assertRaises(StateError):
            self.collector(payload=b'x').capture(self.urls[2],refresh=True)
        self.assertEqual(len(self.calls),1)
        refs = self.archive.safety_references()
        self.assertEqual({r['receipt_id'] for r in refs},{legacy_id,conflict_id})
        self.state.synchronize_archive(self.archive,decision='retain contradictory denial',policy_hash='capture-policy/1')
        self.assertTrue(self.state.restrictions(self.urls[1],host='two.test'))
        row = self.state.reservation(original['reservation_id'])
        self.assertEqual(row['status'],200)  # preserve contradictory statements
        with self.state.db() as db:
            imported = {r['reference_id']:r['wrapper_sha256'] for r in db.execute('SELECT * FROM safety_imports')}
        self.assertEqual(imported,{r['reference_id']:r['wrapper_sha256'] for r in refs})

    def test_acknowledged_observed_result_does_not_resurrect_resumed_throttle(self):
        result = self.collector(status=429,payload=b'x').capture(self.urls[0])
        refs = self.archive.safety_references()
        self.assertEqual(len(refs),1)
        with self.state.db() as db:
            meta = db.execute('SELECT archive_generation,safety_sequence FROM operational_meta').fetchone()
            self.assertEqual(meta['safety_sequence'],self.archive.require_ready()['sequence'])
            imported = db.execute('SELECT * FROM safety_imports').fetchone()
            self.assertEqual(imported['reference_id'],refs[0]['reference_id'])
        triggers = [r['id'] for r in self.state.restrictions(self.urls[0],host='one.test') if r['kind']=='throttle']
        self.state.resume_throttle(account_id='a',host='one.test',triggers=triggers,
            policy_hash='capture-policy/1',decision='eligible fixture throttle resume')
        self.state.synchronize_archive(self.archive,decision='same generation repeat sync',policy_hash='capture-policy/1')
        self.assertEqual(self.state.restrictions(self.urls[0],host='one.test'),[])
        self.assertEqual(self.collector(payload=b'x').capture(self.urls[2],refresh=True)['outcome'],'success')
        self.assertEqual(len(self.calls),2)

    def test_S2_backward_wall_times_late_saved_and_observation_slots(self):
        first = self.collector(payload=b'old').capture(self.urls[0])
        self.clock.wall -= 10
        later = self.collector(payload=b'new').capture(self.urls[0],refresh=True)
        self.assertLess(later['ended_at'],first['ended_at'])
        self.assertGreater(later['admission_sequence'],first['admission_sequence'])
        saved = Path(self.tmp.name)/'late-saved'
        saved.write_bytes(b'old')
        historical = self.collector().register_saved({'url':self.urls[0],'status':200,
            'sha256':hashlib.sha256(b'old').hexdigest()},saved)
        reused = self.collector().capture(self.urls[0])
        self.assertEqual(reused['prior_capture_id'],later['id'])
        self.assertNotEqual(reused['prior_capture_id'],historical['id'])
        saved_url = 'https://saved.test/a'
        self.urls.append(saved_url)
        historical = self.collector().register_saved({'url':saved_url,'status':200,
            'sha256':hashlib.sha256(b'old').hexdigest()},saved)
        offline = Collector(self.archive,Policy(frozenset([saved_url])))
        self.assertEqual(offline.capture(saved_url)['prior_capture_id'],historical['id'])
        fresh = self.collector(payload=b'xyz').capture(saved_url)
        self.assertEqual(fresh['outcome'],'success')
        self.assertEqual(len(self.calls),3)  # saved unknown context cannot satisfy live
        for slot in ('before','after'):
            result = self.collector(payload=b'x',observation_scope=slot).capture(self.urls[2],method='POST',body=b'exact-export')
            self.assertEqual(result['observation_scope'],slot)
        self.assertEqual(len(self.calls),5)

    def test_point_capture_paths_and_reopen_do_not_scan_unrelated_receipts(self):
        first = self.collector(payload=b'x').capture(self.urls[0])
        noise = self.archive.receipt('unrelated',{'purpose':'unrelated immutable fixture'})
        path = self.archive.root/'receipts'/(noise+'.json')
        path.chmod(0o644)
        path.write_bytes(b'corrupt unrelated fixture')
        original_read = Path.read_bytes
        def read(path):
            if path.name == noise+'.json':
                self.fail('point path read unrelated receipt')
            return original_read(path)
        with patch.object(Path,'glob',side_effect=AssertionError('directory scan')), \
             patch.object(Path,'rglob',side_effect=AssertionError('directory scan')), \
             patch.object(Path,'iterdir',side_effect=AssertionError('directory scan')), \
             patch.object(Path,'read_bytes',read):
            reopened = Archive(self.archive.root)
            self.assertEqual(reopened.require_ready()['status'],'ready')
            c = self.collector(payload=b'x')
            self.assertEqual(c.capture(self.urls[0])['prior_capture_id'],first['id'])
            self.assertEqual(c.capture(self.urls[1])['outcome'],'success')
            reopened.close()

    def test_explicit_c1a_schema_upgrade_preserves_quota_rows_and_rollback(self):
        self.collector(payload=b'x').capture(self.urls[0])
        row,fences = self.reservation(host='old-uncertain')
        self.state.uncertain(row['id'],'o',fences)
        self.state.release(fences,'o')
        # Construct exact schema1 topology from the isolated schema2 fixture;
        # schema1 rows remain real captured/charged values, not fabricated claims.
        with closing(sqlite3.connect(self.state.path)) as db, db:
            for table in ('campaign_executions','campaign_jobs','campaign_runs',
                          'activations','safety_imports','event_triggers'):
                db.execute('DROP TABLE '+table)
            db.execute('DROP INDEX event_scope_lookup')
            db.execute('ALTER TABLE operational_meta DROP COLUMN authority_id')
            db.execute('ALTER TABLE operational_meta DROP COLUMN archive_generation')
            db.execute('ALTER TABLE operational_meta DROP COLUMN safety_sequence')
            db.execute('ALTER TABLE reservations DROP COLUMN transport_mode')
            db.execute('UPDATE operational_meta SET version=1')
            preserved = {name:db.execute('SELECT * FROM '+name).fetchall() for name in ('accounts','reservations','leases','lease_fences','events')}
        (self.archive.root/'capture-authority.json').unlink()  # schema1 predates marker
        (self.archive.root/'capture-authority-ready.json').unlink()
        with self.assertRaises(StateError):
            CampaignState(self.archive.root)
        def fault(where):
            if where=='upgrade_before_commit':
                raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            CampaignState.upgrade(self.archive.root,quiesced=True,decision='fixture state migration',fault=fault)
        with closing(sqlite3.connect(self.state.path)) as db:
            self.assertEqual(db.execute('SELECT version FROM operational_meta').fetchone()[0],1)
            for name,rows in preserved.items():
                self.assertEqual(db.execute('SELECT * FROM '+name).fetchall(),rows)
        self.state = CampaignState.upgrade(self.archive.root,quiesced=True,decision='fixture state migration')
        self.state.synchronize_archive(self.archive,decision='bind upgraded fixture',policy_hash='capture-policy/1')
        self.assertEqual(self.state.account('a')['spent_requests'],2)
        self.assertEqual(self.state.account('a')['spent_bytes'],8)
        self.assertEqual(self.state.reservation(row['id'])['state'],'uncertain')
        with self.state.db() as db:
            self.assertEqual([tuple(r) for r in db.execute('SELECT * FROM accounts')],preserved['accounts'])
            upgraded = [tuple(r)[:-1] for r in db.execute('SELECT * FROM reservations')]
            self.assertEqual(upgraded,preserved['reservations'])
            events = [tuple(r) for r in db.execute('SELECT * FROM events')]
            self.assertEqual(events[:len(preserved['events'])],preserved['events'])
        with self.assertRaises(StopCollection):
            self.collector().capture(self.urls[1],refresh=True)

    def test_fixture_production_activation_and_generation_invalidation_no_network(self):
        self.state.activate(self.archive,'a',policy_hash='capture-policy/1',decision='synthetic finite test authorization')
        c = Collector(self.archive,Policy(frozenset(self.urls),budget=1,max_bytes=3),state=self.state,account_id='a')
        class NativeResponse:
            code,url,headers = 200,self.urls[0],{}
            def __init__(self):
                self.sent = False
            def __enter__(self):
                return self
            def __exit__(self,*args):
                pass
            def read1(self,size):
                if self.sent:
                    return b''
                self.sent=True
                return b'x'
        with patch('urllib.request.build_opener') as opener:
            opener.return_value.open.return_value = NativeResponse()
            result = c.capture(self.urls[0])
            self.assertEqual(opener.return_value.open.call_count,1)
        self.assertEqual(self.state.reservation(result['reservation_id'])['transport_mode'],'production')
        prior = self.archive.require_ready()['generation']
        self.archive.reindex(quiesced=True,decision='synthetic new generation')
        self.assertNotEqual(self.archive.require_ready()['generation'],prior)
        with self.assertRaises(StateError):
            self.state.require_production_ready(self.archive,'a')
        with self.assertRaises(StateError):
            self.state.synchronize_archive(self.archive,decision='generation change lacks explicit flag')
        self.state.synchronize_archive(self.archive,decision='reviewed generation replacement',allow_generation_change=True,policy_hash='capture-policy/1')
        with self.assertRaises(StateError):
            self.state.require_production_ready(self.archive,'a')  # activation is generation-bound
        self.state.activate(self.archive,'a',policy_hash='capture-policy/1',decision='synthetic renewed finite test authorization')
        self.state.require_production_ready(self.archive,'a')
        self.assertEqual(self.state.account('a')['spent_requests'],1)

    def test_nonzero_production_and_implicit_mock_initialization_refused(self):
        with self.assertRaises(StateError):
            Collector(self.archive,Policy(frozenset(self.urls),budget=1))
        with self.assertRaises(StateError):
            Collector(self.archive,Policy(frozenset(self.urls),budget=1),lambda *args:None,mock_transport=True)
        with self.assertRaises(StateError):
            Collector(self.archive,Policy(frozenset(self.urls),budget=1),transport,state=self.state,account_id='a',mock_transport=True)
        changed = self.collector(payload=b'x')
        changed.transport = transport
        with self.assertRaises(StateError):
            changed.capture(self.urls[0])
        self.assertEqual(self.state.account('a')['spent_requests'],0)
        offline = Collector(self.archive,Policy(frozenset(self.urls),budget=0))
        with patch('socket.socket',side_effect=AssertionError('network')):
            self.assertEqual(offline.capture(self.urls[0])['outcome'],'offline_unavailable')


    # ---- S4: explicit clock/boot reconciliation -------------------------------------------
    def restarted(self, boot='new-boot'):
        return CampaignState(self.archive.root,clock=lambda:self.clock.wall,monotonic=lambda:self.clock.mono,boot=boot)

    def admits(self, state, account):
        with state.db(True) as db:
            state._eligible(db,account,new=True)

    def events(self, **match):
        with self.state.db() as db:
            rows = [dict(r) for r in db.execute('SELECT * FROM events ORDER BY sequence')]
        return [r for r in rows if all(r[k]==v for k,v in match.items())]

    def test_S4_reconcile_after_boot_change_preserves_remaining_window(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.advance(30)
        other = self.restarted()
        with self.assertRaises(StopCollection):
            self.admits(other,'w')
        other.reconcile_clock('w',archive=self.archive,decision='reviewed reboot')
        self.admits(other,'w')
        row = other.account('w')
        self.assertEqual((row['boot'],row['start_wall'],row['window'],row['deadline']),('new-boot',100,100,None))
        self.assertEqual(row['start_mono'],self.clock.mono-30)
        self.clock.advance(69)  # 99 consumed in total: still admitted
        self.admits(other,'w')
        self.clock.advance(1)   # 100 consumed: window shrinks, never extends
        with self.assertRaises(StopCollection) as raised:
            self.admits(other,'w')
        self.assertIn('window exhausted',str(raised.exception))
        # a later restart needs its own reconcile and still counts the full consumption
        third = self.restarted('third-boot')
        third.reconcile_clock('w',archive=self.archive,decision='second reboot')
        with self.assertRaises(StopCollection):
            self.admits(third,'w')

    def test_S4_reconcile_after_drift_same_boot_and_cross_check_consistent(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.wall += 30  # NTP step or suspend: wall runs ahead of monotonic
        with self.assertRaises(StopCollection):
            self.admits(self.state,'w')
        self.state.reconcile_clock('w',archive=self.archive,decision='reviewed step')
        self.admits(self.state,'w')
        self.assertEqual(self.state.account('w')['start_mono'],self.clock.mono-30)
        self.clock.advance(69)
        self.admits(self.state,'w')
        self.clock.advance(1)
        with self.assertRaises(StopCollection):
            self.admits(self.state,'w')

    def test_S4_negative_monotonic_elapsed_is_reconciled(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.wall += 10
        self.clock.mono = 5  # monotonic restarted lower than the account start (10)
        with self.assertRaises(StopCollection):
            self.admits(self.state,'w')
        self.state.reconcile_clock('w',archive=self.archive,decision='monotonic reset')
        self.admits(self.state,'w')
        self.assertEqual(self.state.account('w')['start_mono'],5-10)

    def test_S4_deadline_account_keeps_absolute_wall_deadline(self):
        self.account('d',request_limit=5,byte_limit=100,deadline=200)
        self.clock.advance(30)
        other = self.restarted()
        other.reconcile_clock('d',archive=self.archive,decision='reviewed reboot')
        self.admits(other,'d')
        self.assertEqual(other.account('d')['deadline'],200)
        self.clock.advance(69)
        self.admits(other,'d')
        self.clock.advance(1)  # wall 200: absolute deadline reached
        with self.assertRaises(StopCollection) as raised:
            self.admits(other,'d')
        self.assertIn('deadline exhausted',str(raised.exception))

    def test_S4_reconcile_refusals_leave_state_unchanged(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.account('untimed',request_limit=5,byte_limit=100)
        def snapshot():
            return self.state.account('w'),len(self.events())
        before = snapshot()
        with self.assertRaises(ValueError):
            self.state.reconcile_clock('w',archive=self.archive,decision='')
        with self.assertRaises(StateError):  # healthy timing: no tampering
            self.state.reconcile_clock('w',archive=self.archive,decision='nothing wrong')
        self.clock.advance(5)
        with self.assertRaises(StateError):
            self.state.reconcile_clock('w',archive=self.archive,decision='still healthy')
        with self.assertRaises(StateError):
            self.restarted().reconcile_clock('missing',archive=self.archive,decision='unknown account')
        other = self.restarted()
        with self.assertRaises(StateError):  # untimed accounts have nothing to re-baseline
            other.reconcile_clock('untimed',archive=self.archive,decision='untimed')
        with self.assertRaises(ValueError):
            other.reconcile_clock('w',archive=self.archive,decision='')
        self.assertEqual(snapshot(),(before[0],before[1]))
        # wall clock before the account start: human call, even with a boot change
        self.clock.wall = 90
        with self.assertRaises(StateError):
            other.reconcile_clock('w',archive=self.archive,decision='wall went backward')
        self.assertEqual(snapshot(),(before[0],before[1]))

    def test_S4_reconcile_refuses_stopped_account(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.state.cancel('w',decision='operator stop')
        other = self.restarted()
        with self.assertRaises(StateError):
            other.reconcile_clock('w',archive=self.archive,decision='stopped')
        self.assertTrue(other.account('w')['stopped'])
        self.assertEqual(other.account('w')['boot'],'fixture-boot')

    def test_S4_reconcile_refuses_while_reservation_open(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        row,fences = self.reservation(account='a',host='open')
        other = self.restarted()
        with self.assertRaises(StateError):
            other.reconcile_clock('w',archive=self.archive,decision='open reservation')
        self.assertEqual(other.account('w')['boot'],'fixture-boot')
        self.assertEqual(self.events(provenance='explicit-clock-reconcile'),[])

    def test_S4_reconcile_records_recovery_event_and_does_not_touch_stopped(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.advance(30)
        other = self.restarted()
        other.reconcile_clock('w',archive=self.archive,decision='reviewed reboot 1')
        [event] = self.events(provenance='explicit-clock-reconcile')
        self.assertEqual((event['kind'],event['scope'],event['target'],event['decision'],event['boot']),
                         ('recovery','account','w','reviewed reboot 1','new-boot'))
        self.assertEqual(event['policy_hash'],'capture-policy/1')
        for fragment in ('fixture-boot','new-boot','drift','no HTTP'):
            self.assertIn(fragment,event['reason'])
        self.assertIsNone(other.account('w')['stopped'])
        # _eligible stops are raised, never persisted; reconcile therefore leaves `stopped` alone
        with self.assertRaises(StopCollection):
            self.admits(self.state,'w')  # old boot now mismatches the re-baselined row
        self.assertIsNone(self.state.account('w')['stopped'])

    def test_S4_throttle_reconcile_after_boot_change(self):
        url,host = 'https://t.test/a','t.test'
        with self.state.db(True) as db:
            self.state._record_status(db,429,url,None,None,host,'prov-1','capture-policy/1',delay=60)
        [throttle] = self.state.restrictions(url)
        other = self.restarted()
        with self.assertRaises(ValueError):
            other.reconcile_throttle(throttle['id'],decision='')
        with self.assertRaises(StopCollection):
            other.resume_throttle(account_id='a',host=host,triggers=[throttle['id']],policy_hash='capture-policy/1',decision='d')
        self.clock.advance(20)
        other.reconcile_throttle(throttle['id'],decision='reviewed reboot')
        [recovery] = self.events(provenance='explicit-throttle-clock-reconcile')
        self.assertEqual((recovery['kind'],recovery['scope'],recovery['target'],recovery['decision'],recovery['boot']),
                         ('recovery','throttle',throttle['id'],'reviewed reboot','new-boot'))
        self.assertEqual(recovery['eligible_wall'],throttle['eligible_wall'])  # wall delay never shortened
        self.assertEqual(recovery['eligible_mono'],self.clock.mono+40)
        self.assertIn(throttle['id'],recovery['reason'])
        self.assertEqual(self.events(id=throttle['id'])[0]['boot'],'fixture-boot')  # original row immutable
        with self.assertRaises(StateError):  # now consistent: second reconcile is refused
            other.reconcile_throttle(throttle['id'],decision='again')
        with self.assertRaises(StopCollection) as raised:
            other.resume_throttle(account_id='a',host=host,triggers=[throttle['id']],policy_hash='capture-policy/1',decision='d')
        self.assertIn('not yet eligible',str(raised.exception))
        self.clock.advance(39)
        with self.assertRaises(StopCollection):
            other.resume_throttle(account_id='a',host=host,triggers=[throttle['id']],policy_hash='capture-policy/1',decision='d')
        self.clock.advance(1)
        other.resume_throttle(account_id='a',host=host,triggers=[throttle['id']],policy_hash='capture-policy/1',decision='reviewed resume')
        self.assertEqual(other.restrictions(url),[])
        with self.assertRaises(StateError):
            other.reconcile_throttle(throttle['id'],decision='already resumed')

    def test_S4_throttle_reconcile_when_wall_delay_already_elapsed_and_drift(self):
        url,host = 'https://t.test/a','t.test'
        with self.state.db(True) as db:
            self.state._record_status(db,503,url,None,None,host,'prov-2','capture-policy/1',delay=10)
        [throttle] = self.state.restrictions(url)
        with self.assertRaises(StateError):  # healthy timing
            self.state.reconcile_throttle(throttle['id'],decision='nothing wrong')
        with self.assertRaises(StateError):
            self.state.reconcile_throttle('missing',decision='unknown event')
        self.clock.wall += 500  # same boot, wall jumped far past the delay
        with self.assertRaises(StopCollection):
            self.state.resume_throttle(account_id='a',host=host,triggers=[throttle['id']],policy_hash='capture-policy/1',decision='d')
        self.state.reconcile_throttle(throttle['id'],decision='reviewed step')
        [recovery] = self.events(provenance='explicit-throttle-clock-reconcile')
        self.assertEqual((recovery['eligible_wall'],recovery['eligible_mono']),(self.clock.wall,self.clock.mono))
        self.state.resume_throttle(account_id='a',host=host,triggers=[throttle['id']],policy_hash='capture-policy/1',decision='reviewed resume')

    def remaining_window(self, state, account):
        row = state.account(account)
        return row['window']-(self.clock.mono-row['start_mono'])

    def test_S4_same_boot_backward_wall_step_never_gains_time(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.advance(80)  # 20 s left
        self.clock.wall -= 50   # NTP step back: wall elapsed 30 < monotonic 80
        with self.assertRaises(StopCollection):
            self.admits(self.state,'w')
        before = self.remaining_window(self.state,'w')
        self.state.reconcile_clock('w',archive=self.archive,decision='reviewed backward step')
        self.assertEqual(self.remaining_window(self.state,'w'),20)
        self.assertLessEqual(self.remaining_window(self.state,'w'),before)
        self.admits(self.state,'w')
        self.clock.advance(19)
        self.admits(self.state,'w')
        self.clock.advance(1)
        with self.assertRaises(StopCollection):
            self.admits(self.state,'w')

    def test_S4_repeated_reconciles_never_gain_time(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.advance(40)
        previous = self.remaining_window(self.state,'w')
        for step,kind in ((-30,'same-boot backward'),(25,'same-boot forward'),(-20,'same-boot backward')):
            with self.subTest(kind=kind):
                state = self.state
                if step is None:
                    state = self.restarted('repeat-boot')
                else:
                    self.clock.wall += step
                with self.assertRaises(StopCollection):
                    self.admits(state,'w')
                state.reconcile_clock('w',archive=self.archive,decision='reviewed '+kind)
                remaining = self.remaining_window(state,'w')
                self.assertLessEqual(remaining,previous)
                previous = remaining
                self.clock.advance(1)
                previous -= 1

    def test_S4_forward_step_then_reboot_reconciles_never_gain_time(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.advance(40)
        self.clock.wall += 15
        self.state.reconcile_clock('w',archive=self.archive,decision='forward step')
        before = self.remaining_window(self.state,'w')
        self.assertEqual(before,45)
        self.clock.advance(5)
        other = self.restarted('after-reboot')
        other.reconcile_clock('w',archive=self.archive,decision='reboot')
        self.assertLessEqual(self.remaining_window(other,'w'),before-5)

    def test_S4_boot_change_uses_wall_elapsed_even_when_monotonic_differs(self):
        self.account('w',request_limit=5,byte_limit=100,window=100)
        self.clock.wall += 30
        self.clock.mono = 3  # new boot: monotonic is unrelated to the old epoch
        other = self.restarted()
        other.reconcile_clock('w',archive=self.archive,decision='reviewed reboot')
        self.assertEqual(self.remaining_window(other,'w'),70)  # 30 s of wall time consumed
        self.admits(other,'w')

    def test_S4_boot_fallback_is_unique_per_process_even_with_same_pid(self):
        from devicehunt import campaign_state
        with patch('pathlib.Path.read_text',side_effect=OSError('no proc')), patch('os.getpid',return_value=4242):
            mine = campaign_state._boot()
            self.assertEqual(mine,campaign_state._boot())
        self.assertTrue(mine.startswith('process-4242-'))
        self.assertIn(campaign_state._PROCESS_UUID,mine)
        ctx = multiprocessing.get_context('spawn')
        output = ctx.Queue()
        worker = ctx.Process(target=_fallback_boot_worker,args=(output,))
        worker.start()
        theirs = output.get(timeout=30)
        worker.join(10)
        self.assertEqual(worker.exitcode,0)
        self.assertTrue(theirs.startswith('process-4242-'))
        self.assertNotEqual(mine,theirs)

    def test_S4_real_boot_and_clocks_across_processes(self):
        try:
            Path('/proc/sys/kernel/random/boot_id').read_text()
        except OSError:
            self.skipTest('no /proc boot id')
        root = Path(self.tmp.name)/'real'
        archive = Archive(root,create=True)
        try:
            state = CampaignState.initialize(root)
            state.allocate_account('real-window',definition_hash='d',policy_hash='capture-policy/1',
                                   request_limit=1,byte_limit=4,window=1800)
            state.synchronize_archive(archive,decision='real boot fixture safety binding',policy_hash='capture-policy/1')
            ctx = multiprocessing.get_context('spawn')
            output = ctx.Queue()
            worker = ctx.Process(target=_real_boot_worker,args=(str(root),output))
            worker.start()
            self.assertEqual(output.get(timeout=30),'admitted')
            worker.join(10)
            self.assertEqual(worker.exitcode,0)
            # healthy real timing is not reconcilable
            with self.assertRaises(StateError):
                state.reconcile_clock('real-window',archive=archive,decision='healthy')
            # simulate a reboot by rewriting the stored boot id, then reconcile with the real _boot()
            with closing(sqlite3.connect(state.path)) as db:
                db.execute("UPDATE accounts SET boot='previous-boot' WHERE id='real-window'")
                db.commit()
            with self.assertRaises(StopCollection):
                with state.db(True) as db:
                    state._eligible(db,'real-window')
            state.reconcile_clock('real-window',archive=archive,decision='real reboot simulation')
            worker = ctx.Process(target=_real_boot_worker,args=(str(root),output))
            worker.start()
            self.assertEqual(output.get(timeout=30),'admitted')
            worker.join(10)
        finally:
            archive.close()


if __name__ == '__main__':
    unittest.main()

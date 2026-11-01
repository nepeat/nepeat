"""Durable admission with indexed publication proof and explicit activation.

This store is safety authority, not a rebuildable cache. Production needs a ready
archive, exact safety synchronization and a separately authorized finite account.
"""
from contextlib import contextmanager, closing
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import tempfile
import time
import uuid
from urllib.parse import urlsplit

SCHEMA = 3


class StopCollection(RuntimeError):
    pass


class LeaseBusy(StopCollection):
    pass


class StateError(StopCollection):
    pass


# pid alone repeats across container restarts; this is fixed once per interpreter.
_PROCESS_UUID = uuid.uuid4().hex


def _boot():
    try:
        return Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    except OSError:
        # On hosts without a shared monotonic epoch restart is conservative.
        return 'process-%d-%s' % (os.getpid(), _PROCESS_UUID)


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)


def host_for_url(url):
    """Safety host is the DNS name across ports; exact URL remains separate.

    No DNS/alias lookup. Grouping all ports is conservative for throttles/quotas.
    """
    parsed = urlsplit(url)
    if parsed.scheme.lower() not in ('http','https') or not parsed.hostname or parsed.username is not None or parsed.password is not None:
        raise ValueError('public HTTP(S) URL without userinfo required')
    parsed.port  # validate even though host safety scopes span all ports
    host = parsed.hostname.lower()
    return '[' + host + ']' if ':' in host else host


def normalize_host(host):
    if not isinstance(host,str) or not host or any(c in host for c in '/?#@'):
        raise ValueError('host authority required')
    return host_for_url('https://' + host)


def _authority_marker(root, *, create=False, origin=None, backup=None):
    from .artifacts import _safe, _publish, file_digest
    path = _safe(root,Path(root)/'capture-authority.json')
    archive_id = CampaignState.identity(root)
    backup_hash = file_digest(backup) if backup is not None else None
    if path.exists():
        try:
            row = json.loads(path.read_bytes())
            if set(row)!={'version','archive_id','authority_id','origin','legacy_backup_sha256'} or row['version']!=1 or row['archive_id']!=archive_id:
                raise StateError('invalid independent operational authority marker')
            uuid.UUID(row['authority_id'])
            if create and (row['origin']!=origin or row['legacy_backup_sha256']!=backup_hash):
                raise StateError('authority marker conflicts with explicit migration backup')
            return row
        except (OSError,ValueError,KeyError,TypeError) as exc:
            raise StateError('invalid independent operational authority marker') from exc
    if not create:
        raise StateError('missing independent operational authority marker; explicit recovery required')
    row = dict(version=1,archive_id=archive_id,authority_id=str(uuid.uuid4()),origin=origin,legacy_backup_sha256=backup_hash)
    _publish(path,_canonical(row).encode())
    return row


def _authority_seal(root, marker, *, create=False):
    from .artifacts import _safe, _publish
    path = _safe(root,Path(root)/'capture-authority-ready.json')
    expected = dict(version=1,archive_id=marker['archive_id'],authority_id=marker['authority_id'])
    if path.exists():
        try:
            if json.loads(path.read_bytes())!=expected:
                raise StateError('operational authority completion fence mismatch')
        except (OSError,ValueError,TypeError) as exc:
            raise StateError('invalid operational completion fence') from exc
    elif create:
        _publish(path,_canonical(expected).encode())
    else:
        raise StateError('unfinished authority allocation; explicit recovery required')


def _replace_immutable(path, data):
    """Atomically swap one read-only authority file; only explicit relocation does this."""
    from .artifacts import _fsync_directory
    fd, staging = tempfile.mkstemp(prefix='.pending-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(staging, 0o444)
        os.replace(staging, path)
        _fsync_directory(path.parent)
    finally:
        if os.path.exists(staging):
            os.unlink(staging)


def _refuse_completed_migration(root):
    from .artifacts import _safe
    if _safe(root,Path(root)/'capture-authority-ready.json').exists():
        raise StateError('completed authority cannot be remigrated from an old backup; explicit coherent restore required')


class FileLocks:
    """Never unlink held lock files. SQL leases complement, not replace flock."""
    def __init__(self, root):
        from .artifacts import _safe
        self.root = _safe(Path(root).absolute(),Path(root).absolute())
        self.handles = {}

    def acquire(self, keys):
        opened = []
        try:
            from .artifacts import _safe
            _safe(self.root,self.root)
            self.root.mkdir(exist_ok=True)
            for key in sorted(set(keys)):
                if key in self.handles:
                    raise LeaseBusy(key)
                path = self.root / (hashlib.sha256(key.encode()).hexdigest() + '.lock')
                fd = os.open(path, os.O_RDWR | os.O_CREAT | getattr(os, 'O_NOFOLLOW', 0), 0o600)
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BaseException:
                    os.close(fd)
                    raise LeaseBusy(key) from None
                self.handles[key] = fd
                opened.append(key)
        except BaseException:
            self.release(opened)
            raise

    def release(self, keys):
        for key in keys:
            fd = self.handles.pop(key, None)
            if fd is not None:
                fcntl.flock(fd, fcntl.LOCK_UN)
                os.close(fd)

    @contextmanager
    def held(self, keys):
        self.acquire(keys)
        try:
            yield
        finally:
            self.release(keys)


# An uncertain attempt that never observed a status stays uncertain and charged forever;
# only an explicit settle_unobserved decision stops it from blocking unrelated admissions.
_OPEN_RESERVATIONS = ("SELECT 1 FROM reservations WHERE state!='complete' AND id NOT IN "
                      "(SELECT target FROM events WHERE kind='recovery' AND scope='reservation' "
                      "AND provenance='unobserved-settlement') LIMIT 1")


_SQL = '''
CREATE TABLE IF NOT EXISTS leases(key TEXT PRIMARY KEY, owner TEXT, expires REAL);
CREATE TABLE operational_meta(version INTEGER NOT NULL, archive_id TEXT NOT NULL,
    recovery TEXT NOT NULL CHECK(recovery IN ('ready','quarantined')),
    archive_generation TEXT, safety_sequence INTEGER NOT NULL DEFAULT 0 CHECK(safety_sequence>=0),
    authority_id TEXT NOT NULL);
CREATE TABLE lease_fences(key TEXT PRIMARY KEY, generation INTEGER NOT NULL,
    owner TEXT, legacy_recovered INTEGER NOT NULL DEFAULT 0);
CREATE TABLE accounts(id TEXT PRIMARY KEY, definition_hash TEXT NOT NULL,
    policy_hash TEXT NOT NULL, request_limit INTEGER NOT NULL CHECK(request_limit>=0),
    byte_limit INTEGER NOT NULL CHECK(byte_limit>=0), deadline REAL, window REAL,
    start_wall REAL NOT NULL, start_mono REAL NOT NULL, boot TEXT NOT NULL,
    stopped TEXT);
CREATE TABLE host_limits(account_id TEXT NOT NULL REFERENCES accounts(id), host TEXT NOT NULL,
    request_limit INTEGER NOT NULL CHECK(request_limit>=0), byte_limit INTEGER NOT NULL CHECK(byte_limit>=0),
    PRIMARY KEY(account_id,host));
CREATE TABLE reservations(sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id), run_id TEXT NOT NULL,
    unit_revision TEXT NOT NULL, request_id TEXT NOT NULL, source TEXT, resource TEXT,
    url TEXT NOT NULL, host TEXT NOT NULL, attempt_id TEXT UNIQUE NOT NULL,
    max_body INTEGER NOT NULL CHECK(max_body>0), owner TEXT NOT NULL, fences TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('reserved','attempt_published','dispatch_admitted',
        'response_observed','complete','uncertain')),
    start_wall REAL NOT NULL, start_mono REAL NOT NULL, boot TEXT NOT NULL,
    status INTEGER, actual_bytes INTEGER, stored_bytes INTEGER, result_id TEXT,
    transport_mode TEXT NOT NULL DEFAULT 'unknown' CHECK(transport_mode IN ('mock','production','unknown')));
CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('deny','throttle','resume','cancel','recovery')),
    scope TEXT NOT NULL, target TEXT NOT NULL, status INTEGER, provenance TEXT NOT NULL,
    eligible_wall REAL, eligible_mono REAL, boot TEXT NOT NULL, policy_hash TEXT,
    decision TEXT, triggers TEXT NOT NULL, reason TEXT NOT NULL);
CREATE INDEX event_scope_lookup ON events(scope,target,kind,sequence);
CREATE TABLE event_triggers(resume_id TEXT NOT NULL REFERENCES events(id),
    event_id TEXT NOT NULL REFERENCES events(id), PRIMARY KEY(resume_id,event_id));
CREATE INDEX event_trigger_lookup ON event_triggers(event_id);
CREATE TABLE safety_imports(generation TEXT NOT NULL,reference_id TEXT NOT NULL,
    receipt_id TEXT NOT NULL,wrapper_sha256 TEXT NOT NULL,publication_sequence INTEGER NOT NULL,
    PRIMARY KEY(generation,reference_id));
CREATE TABLE activations(account_id TEXT PRIMARY KEY REFERENCES accounts(id),
    generation TEXT NOT NULL,policy_hash TEXT NOT NULL,decision TEXT NOT NULL,start_wall REAL NOT NULL);
CREATE TABLE campaign_runs(id TEXT PRIMARY KEY, campaign_id TEXT NOT NULL, definition_sha256 TEXT NOT NULL,
    identities TEXT NOT NULL, mode TEXT NOT NULL CHECK(mode IN ('offline')),
    state TEXT NOT NULL CHECK(state IN ('running','complete','stopped')),
    start_sequence INTEGER NOT NULL CHECK(start_sequence>=0), start_index_bytes INTEGER NOT NULL CHECK(start_index_bytes>=0),
    start_wall REAL NOT NULL, boot TEXT NOT NULL, end_wall REAL, reason TEXT);
CREATE TABLE campaign_jobs(id TEXT PRIMARY KEY, campaign_id TEXT NOT NULL, unit_id TEXT NOT NULL,
    unit_revision TEXT NOT NULL, stage TEXT NOT NULL, recipe TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','complete')), outcome TEXT, result_id TEXT,
    planned_run TEXT NOT NULL REFERENCES campaign_runs(id), updated_wall REAL NOT NULL,
    UNIQUE(unit_revision,stage,recipe), CHECK((state='complete')=(result_id IS NOT NULL)));
CREATE INDEX campaign_job_unit ON campaign_jobs(campaign_id,unit_id,stage);
CREATE TABLE campaign_executions(sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES campaign_jobs(id), run_id TEXT NOT NULL REFERENCES campaign_runs(id),
    action TEXT NOT NULL CHECK(action IN ('executed','reused','recovered')), result_id TEXT NOT NULL,
    new_bytes INTEGER NOT NULL CHECK(new_bytes>=0), wall REAL NOT NULL);
CREATE INDEX campaign_execution_run ON campaign_executions(run_id,sequence);
'''
# Schema 3 adds only these offline campaign checkpoint tables to schema 2.
_SQL_V3 = _SQL[_SQL.index('CREATE TABLE campaign_runs'):]


class CampaignState:
    def __init__(self, root, *, readonly=False, clock=time.time, monotonic=time.monotonic,
                 boot=None, fault=None):
        from .artifacts import _safe
        self.root = _safe(Path(root).absolute(), Path(root).absolute())
        self.path = _safe(self.root, self.root / 'capture-leases.sqlite')
        self.readonly = readonly
        self.clock, self.monotonic = clock, monotonic
        self.boot = boot or _boot()
        self.fault = fault or (lambda point: None)
        self.locks = FileLocks(_safe(self.root, self.root / 'capture-locks'))
        self.lock_owners = {}
        self._validate()

    @staticmethod
    def identity(root):
        return hashlib.sha256(str(Path(root).absolute()).encode()).hexdigest()

    @classmethod
    def initialize(cls, root, **kwargs):
        """Explicit new-store allocation. Missing state during normal open is fatal."""
        from .artifacts import _safe
        root = _safe(Path(root).absolute(), Path(root).absolute())
        if not root.is_dir():
            raise StateError('archive must already exist')
        # Validate lock paths before allocating any independent authority bytes.
        _safe(root,root/'capture-locks')
        # A lost authority must not be recreated beside existing evidence.
        receipts = _safe(root, root / 'receipts')
        if receipts.exists() and any(receipts.iterdir()):
            raise StateError('existing receipts require explicit migration/recovery, not new authority')
        path = _safe(root, root / 'capture-leases.sqlite')
        marker_path = _safe(root,root/'capture-authority.json')
        if path.exists() or marker_path.exists() or _safe(root,root/'capture-authority-ready.json').exists():
            raise StateError('existing/lost authority requires explicit recovery, never initialization')
        marker = _authority_marker(root,create=True,origin='new')
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_NOFOLLOW', 0), 0o600)
        os.close(fd)
        try:
            with closing(sqlite3.connect(path)) as db, db:
                db.executescript('BEGIN IMMEDIATE;\n' + _SQL)
                db.execute('INSERT INTO operational_meta(version,archive_id,recovery,authority_id) VALUES (?,?,?,?)', (SCHEMA, cls.identity(root), 'ready',marker['authority_id']))
        except BaseException:
            # Keep the incomplete authority file; explicit recovery is required.
            raise
        _authority_seal(root,marker,create=True)
        return cls(root, **kwargs)

    @classmethod
    def migrate(cls, root, *, quiesced=False, recovery_decision=None, fault=None, **kwargs):
        """Quiesce old collectors externally. Preserve every legacy row exactly.

        A recovery decision acknowledges all legacy owners, even expired ones.
        It does not release their rows; recover_lease is a separate explicit step.
        """
        from .artifacts import _safe
        root = _safe(Path(root).absolute(), Path(root).absolute())
        path = _safe(root, root / 'capture-leases.sqlite')
        if not quiesced or not path.is_file():
            raise StateError('explicit quiesced legacy migration required')
        _refuse_completed_migration(root)
        hook = fault or (lambda point: None)
        locks = FileLocks(root / 'capture-locks')
        with locks.held(['publication']):
            with closing(sqlite3.connect(path.as_uri() + '?mode=rw', uri=True)) as db, db:
                names = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
                if names != {'leases'}:
                    raise StateError('not a legacy lease schema')
                if [r[1] for r in db.execute('PRAGMA table_info(leases)')] != ['key', 'owner', 'expires']:
                    raise StateError('unsupported legacy leases')
                if db.execute('SELECT 1 FROM leases WHERE owner IS NOT NULL LIMIT 1').fetchone() and not recovery_decision:
                    raise StateError('legacy owners require a recorded recovery decision')
                backup = _safe(root, root / 'capture-leases.before-c1a.sqlite')
                expected = db.execute('SELECT key,owner,expires FROM leases ORDER BY key').fetchall()
                if backup.exists():
                    with closing(sqlite3.connect(backup.as_uri() + '?mode=ro',uri=True)) as prior:
                        if prior.execute('SELECT key,owner,expires FROM leases ORDER BY key').fetchall() != expected:
                            raise StateError('pre-migration backup conflicts')
                else:
                    fd = os.open(backup,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,'O_NOFOLLOW',0),0o600)
                    os.close(fd)
                    with closing(sqlite3.connect(backup)) as prior:
                        db.backup(prior)
                        if prior.execute('SELECT key,owner,expires FROM leases ORDER BY key').fetchall() != expected:
                            raise StateError('pre-migration backup verification failed')
                marker = _authority_marker(root,create=True,origin='legacy-migration',backup=backup)
                db.execute('BEGIN IMMEDIATE')
                # executescript implicitly commits: execute each statement instead.
                for statement in _SQL.split(';'):
                    if statement.strip():
                        db.execute(statement)
                hook('migration_schema')
                db.execute('INSERT INTO operational_meta(version,archive_id,recovery,authority_id) VALUES (?,?,?,?)', (SCHEMA, cls.identity(root), 'ready',marker['authority_id']))
                db.execute('INSERT INTO events(id,kind,scope,target,provenance,boot,decision,triggers,reason) '
                           'VALUES (?,?,?,?,?,?,?,?,?)', (str(uuid.uuid4()), 'recovery', 'archive', cls.identity(root),
                           'legacy-migration', _boot(), recovery_decision, '[]', 'quiesced explicit migration'))
                hook('migration_before_commit')
        _authority_seal(root,marker,create=True)
        return cls(root, fault=fault, **kwargs)

    @classmethod
    def upgrade(cls, root, *, quiesced=False, decision=None, fault=None, **kwargs):
        """Explicit schema1 -> current; original quota/evidence rows are retained."""
        from .artifacts import _safe
        root = _safe(Path(root).absolute(),Path(root).absolute())
        path = _safe(root,root/'capture-leases.sqlite')
        if not quiesced or not decision or not path.is_file():
            raise StateError('quiesced, recorded c1a state upgrade required')
        _refuse_completed_migration(root)
        locks = FileLocks(root/'capture-locks')
        hook = fault or (lambda point:None)
        with locks.held(['publication']), closing(sqlite3.connect(path.as_uri()+'?mode=rw',uri=True)) as db, db:
            meta = db.execute('SELECT version,archive_id,recovery FROM operational_meta').fetchall()
            if len(meta)!=1 or meta[0][0]!=1 or meta[0][1]!=cls.identity(root):
                raise StateError('not a matching c1a authority')
            keys = [row[0] for row in db.execute('SELECT key FROM leases')]
            with locks.held(keys):
                backup = _safe(root,root/'capture-leases.before-c1b.sqlite')
                def snapshot(connection):
                    names = [r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
                    return {name:sorted(connection.execute('SELECT * FROM "'+name.replace('"','""')+'"').fetchall(),key=repr) for name in names}
                expected = snapshot(db)
                if backup.exists():
                    with closing(sqlite3.connect(backup.as_uri()+'?mode=ro',uri=True)) as prior:
                        if snapshot(prior)!=expected:
                            raise StateError('c1a upgrade backup conflicts')
                else:
                    fd = os.open(backup,os.O_CREAT|os.O_EXCL|os.O_WRONLY|getattr(os,'O_NOFOLLOW',0),0o600)
                    os.close(fd)
                    with closing(sqlite3.connect(backup)) as prior:
                        db.backup(prior)
                        if snapshot(prior)!=expected:
                            raise StateError('c1a backup verification failed')
                marker = _authority_marker(root,create=True,origin='c1a-upgrade',backup=backup)
                db.execute('BEGIN IMMEDIATE')
                db.execute('ALTER TABLE operational_meta ADD COLUMN authority_id TEXT')
                db.execute('ALTER TABLE operational_meta ADD COLUMN archive_generation TEXT')
                db.execute('ALTER TABLE operational_meta ADD COLUMN safety_sequence INTEGER NOT NULL DEFAULT 0 CHECK(safety_sequence>=0)')
                db.execute("ALTER TABLE reservations ADD COLUMN transport_mode TEXT NOT NULL DEFAULT 'unknown' CHECK(transport_mode IN ('mock','production','unknown'))")
                additions = _SQL[_SQL.index('CREATE INDEX event_scope_lookup'):]
                for statement in additions.split(';'):
                    if statement.strip():
                        db.execute(statement)
                hook('upgrade_schema')
                db.execute('UPDATE operational_meta SET version=?,authority_id=?',(SCHEMA,marker['authority_id']))
                db.execute('INSERT INTO events(id,kind,scope,target,provenance,boot,decision,triggers,reason) VALUES (?,?,?,?,?,?,?,?,?)',
                           (str(uuid.uuid4()),'recovery','archive',cls.identity(root),'c1b-upgrade',_boot(),decision,'[]','explicit quota-preserving schema upgrade'))
                # Legacy resume JSON has exactly the same append-only meaning.
                for resume_id,trigger_json in db.execute("SELECT id,triggers FROM events WHERE kind='resume'").fetchall():
                    for trigger in json.loads(trigger_json):
                        db.execute('INSERT INTO event_triggers VALUES (?,?)',(resume_id,trigger))
                hook('upgrade_before_commit')
        _authority_seal(root,marker,create=True)
        return cls(root,fault=fault,**kwargs)

    @classmethod
    def upgrade_campaign_jobs(cls, root, *, quiesced=False, decision=None, fault=None, **kwargs):
        """Explicit schema2 -> schema3; adds only offline campaign checkpoint tables.

        The completed authority marker stays; quota, lease and event rows are untouched.
        """
        from .artifacts import _safe
        root = _safe(Path(root).absolute(),Path(root).absolute())
        path = _safe(root,root/'capture-leases.sqlite')
        if not quiesced or not decision or not path.is_file():
            raise StateError('quiesced, recorded schema 3 upgrade required')
        marker = _authority_marker(root)
        _authority_seal(root,marker)
        hook = fault or (lambda point:None)
        locks = FileLocks(root/'capture-locks')
        with locks.held(['publication']), closing(sqlite3.connect(path.as_uri()+'?mode=rw',uri=True)) as db, db:
            meta = db.execute('SELECT version,archive_id,authority_id FROM operational_meta').fetchall()
            if len(meta)!=1 or tuple(meta[0])!=(2,cls.identity(root),marker['authority_id']):
                raise StateError('not a matching schema 2 authority')
            if db.execute('SELECT 1 FROM leases LIMIT 1').fetchone():
                raise StateError('active leases block quiesced upgrade')
            backup = _safe(root,root/'capture-leases.before-c2.sqlite')
            def snapshot(connection):
                names = [r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
                return {name:sorted(connection.execute('SELECT * FROM "'+name.replace('"','""')+'"').fetchall(),key=repr) for name in names}
            expected = snapshot(db)
            if backup.exists() and backup.stat().st_size:
                # A retry after an interrupted upgrade reuses only an exactly matching backup.
                try:
                    with closing(sqlite3.connect(backup.as_uri()+'?mode=ro',uri=True)) as prior:
                        if snapshot(prior)!=expected:
                            raise StateError('schema 2 upgrade backup conflicts with current state')
                except sqlite3.DatabaseError as exc:
                    raise StateError('unreadable schema 2 upgrade backup; inspect before retrying') from exc
            else:
                # Absent, or an empty file left by a crash before any byte was copied.
                fd = os.open(backup,os.O_CREAT|os.O_WRONLY|getattr(os,'O_NOFOLLOW',0),0o600)
                os.close(fd)
                with closing(sqlite3.connect(backup)) as prior:
                    db.backup(prior)
                    if snapshot(prior)!=expected:
                        raise StateError('schema 2 backup verification failed')
            db.execute('BEGIN IMMEDIATE')
            for statement in _SQL_V3.split(';'):
                if statement.strip():
                    db.execute(statement)
            hook('campaign_jobs_schema')
            db.execute('UPDATE operational_meta SET version=?',(SCHEMA,))
            db.execute('INSERT INTO events(id,kind,scope,target,provenance,boot,decision,triggers,reason) VALUES (?,?,?,?,?,?,?,?,?)',
                       (str(uuid.uuid4()),'recovery','archive',cls.identity(root),'c2-upgrade',_boot(),decision,'[]','explicit campaign checkpoint schema upgrade'))
            hook('campaign_jobs_before_commit')
        return cls(root,fault=fault,**kwargs)

    @classmethod
    def relocate(cls, root, *, old_root, quiesced=False, decision=None, fault=None, **kwargs):
        """Explicit whole-archive move to a new absolute root.

        Identity stays path-bound so a copied archive can never share quota
        authority: a move is accepted only once the old root no longer exists.
        Only archive_id changes, in order: operational_meta, marker, seal.
        authority_id, origin and every quota/lease/event row are kept; repeating
        the same command finishes an interrupted relocation.
        """
        from .artifacts import _safe
        root = _safe(Path(root).absolute(),Path(root).absolute())
        old = Path(old_root).absolute()
        path = _safe(root,root/'capture-leases.sqlite')
        if not quiesced or not decision or not path.is_file():
            raise StateError('quiesced, recorded relocation required')
        old_id, new_id = cls.identity(old), cls.identity(root)
        if old_id == new_id:
            raise StateError('relocation requires a different archive root')
        if os.path.lexists(old):
            raise StateError('old archive root still exists; relocation is a move, never a copy')
        marker_path = _safe(root,root/'capture-authority.json')
        seal_path = _safe(root,root/'capture-authority-ready.json')
        def read(file):
            try:
                return json.loads(file.read_bytes())
            except (OSError,ValueError) as exc:
                raise StateError('unreadable authority file: ' + file.name) from exc
        marker, seal = read(marker_path), read(seal_path)
        try:
            if (not isinstance(marker,dict) or set(marker)!={'version','archive_id','authority_id','origin','legacy_backup_sha256'}
                    or marker['version']!=1 or marker['archive_id'] not in (old_id,new_id) or not isinstance(marker['origin'],str)
                    or not isinstance(marker['legacy_backup_sha256'],(str,type(None)))):
                raise ValueError
            uuid.UUID(marker['authority_id'])
        except (ValueError,TypeError,AttributeError) as exc:
            raise StateError('authority marker belongs to neither the old nor the new root') from exc
        if not isinstance(seal,dict) or seal!=dict(version=1,archive_id=seal.get('archive_id'),authority_id=marker['authority_id']) \
                or seal['archive_id'] not in (old_id,new_id) or (seal['archive_id']==new_id and marker['archive_id']!=new_id):
            raise StateError('authority completion fence does not match a relocation in progress')
        hook = fault or (lambda point:None)
        locks = FileLocks(root/'capture-locks')
        def backup_path(events_before):
            # Binds both roots and the append-only event count, so moving back and
            # forth never reuses an old backup, while a retry finds the same name.
            return _safe(root,root/('capture-leases.before-relocate-' + old_id[:16] + '-' + new_id[:16]
                                    + '-' + str(events_before) + '.sqlite'))
        def snapshot(connection):
            names = [r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
            return {name:sorted(connection.execute('SELECT * FROM "'+name.replace('"','""')+'"').fetchall(),key=repr) for name in names}
        with locks.held(['publication']), closing(sqlite3.connect(path.as_uri()+'?mode=rw',uri=True)) as db, db:
            # Write lock first: checks, backup and update see one state.
            db.execute('BEGIN IMMEDIATE')
            meta = db.execute('SELECT version,archive_id,authority_id,recovery FROM operational_meta').fetchall()
            if len(meta)!=1 or meta[0][0]!=SCHEMA or meta[0][2]!=marker['authority_id'] or meta[0][1] not in (old_id,new_id) \
                    or (marker['archive_id']==new_id and meta[0][1]!=new_id):
                raise StateError('operational authority does not match a relocation in progress')
            if meta[0][3]!='ready':
                raise StateError('quarantined authority must be recovered before relocation')
            if db.execute('SELECT 1 FROM leases LIMIT 1').fetchone():
                raise StateError('active leases block relocation')
            if db.execute(_OPEN_RESERVATIONS).fetchone():
                raise StateError('unfinished reservations block relocation')
            if db.execute("SELECT 1 FROM campaign_runs WHERE state='running' LIMIT 1").fetchone():
                raise StateError('a running campaign blocks relocation; resume or stop it first')
            if meta[0][1]==old_id:
                backup = backup_path(db.execute('SELECT COUNT(*) FROM events').fetchone()[0])
                expected = snapshot(db)
                if backup.exists() and backup.stat().st_size:
                    try:
                        with closing(sqlite3.connect(backup.as_uri()+'?mode=ro',uri=True)) as prior:
                            if snapshot(prior)!=expected:
                                raise StateError('relocation backup conflicts with current state')
                    except sqlite3.DatabaseError as exc:
                        raise StateError('unreadable relocation backup; inspect before retrying') from exc
                else:
                    fd = os.open(backup,os.O_CREAT|os.O_WRONLY|getattr(os,'O_NOFOLLOW',0),0o600)
                    os.close(fd)
                    # A separate reader copies the committed bytes; our write lock keeps them fixed.
                    with closing(sqlite3.connect(path.as_uri()+'?mode=ro',uri=True)) as reader, \
                            closing(sqlite3.connect(backup)) as prior:
                        reader.backup(prior)
                        if snapshot(prior)!=expected:
                            raise StateError('relocation backup verification failed')
                db.execute('UPDATE operational_meta SET archive_id=?',(new_id,))
                db.execute('INSERT INTO events(id,kind,scope,target,provenance,boot,decision,triggers,reason) VALUES (?,?,?,?,?,?,?,?,?)',
                           (str(uuid.uuid4()),'recovery','archive',new_id,'relocation',_boot(),decision,'[]',
                            'explicit archive move from ' + str(old) + ' (identity ' + old_id + ')'))
                hook('relocate_before_commit')
            else:
                done = db.execute("SELECT sequence FROM events WHERE kind='recovery' AND provenance='relocation' AND target=? "
                                  'ORDER BY sequence DESC LIMIT 1',(new_id,)).fetchone()
                if done is None:
                    raise StateError('relocated authority has no relocation event')
                backup = backup_path(db.execute('SELECT COUNT(*) FROM events WHERE sequence<?',(done[0],)).fetchone()[0])
                try:
                    with closing(sqlite3.connect(backup.as_uri()+'?mode=ro',uri=True)) as prior:
                        saved = prior.execute('SELECT archive_id,authority_id FROM operational_meta').fetchall()
                except sqlite3.DatabaseError as exc:
                    raise StateError('relocated authority is missing its relocation backup') from exc
                if saved!=[(old_id,marker['authority_id'])]:
                    raise StateError('relocation backup does not record the old authority')
        hook('relocate_meta')
        if marker['archive_id']==old_id:
            _replace_immutable(marker_path,_canonical(dict(marker,archive_id=new_id)).encode())
        hook('relocate_marker')
        if seal['archive_id']==old_id:
            _replace_immutable(seal_path,_canonical(dict(seal,archive_id=new_id)).encode())
        return cls(root,fault=fault,**kwargs)

    @contextmanager
    def db(self, write=False):
        if write and self.readonly:
            raise PermissionError('read-only operational state')
        if not self.path.is_file():
            raise StateError('missing operational authority; restore or quarantine explicitly')
        db = sqlite3.connect(self.path.as_uri() + ('?mode=rw' if write else '?mode=ro'), uri=True, timeout=5)
        db.row_factory = sqlite3.Row
        db.execute('PRAGMA foreign_keys=ON')
        try:
            if write:
                db.execute('BEGIN IMMEDIATE')
            meta = db.execute('SELECT * FROM operational_meta').fetchall()
            if len(meta) != 1 or meta[0]['version'] != SCHEMA or meta[0]['archive_id'] != self.identity(self.root):
                raise StateError('unsupported/mismatched operational schema or archive identity')
            marker = _authority_marker(self.root)
            _authority_seal(self.root,marker)
            if meta[0]['authority_id'] != marker['authority_id']:
                raise StateError('operational authority does not match independent marker')
            if write and meta[0]['recovery'] != 'ready':
                raise StateError('operational authority quarantined')
            yield db
            if write:
                db.commit()
        except sqlite3.DatabaseError as exc:
            db.rollback()
            raise StateError('invalid operational authority') from exc
        except BaseException:
            db.rollback()
            raise
        finally:
            db.close()

    def _validate(self):
        with self.db():
            pass

    def _archive_snapshot(self, archive):
        if archive.root != self.root:
            raise StateError('archive does not match operational authority')
        return archive.require_ready()

    def _checked_reference(self, archive, reference, snapshot):
        proof = archive.verified_receipt(reference['receipt_id'],require_ack=True)
        if proof['generation'] != snapshot['generation'] or proof['wrapper_sha256'] != reference['wrapper_sha256']:
            raise StateError('safety reference does not match acknowledged wrapper')
        wrapper = proof['wrapper']
        row = archive.safety_row(wrapper,reference['selector'])
        if row != reference['row']:
            raise StateError('safety reference selector/payload mismatch')
        if reference['sequence'] > snapshot['sequence']:
            raise StateError('safety reference beyond acknowledged checkpoint')
        return wrapper,row

    @staticmethod
    def _status(row):
        from .artifacts import Archive
        return Archive.safety_status(row)

    def _known_observation(self, db, wrapper, row):
        if wrapper['kind']!='capture_result' or not row.get('reservation_id') or row.get('restriction_scope') is not None:
            return False
        reservation = db.execute('SELECT * FROM reservations WHERE id=?',(row['reservation_id'],)).fetchone()
        if not reservation or reservation['status'] is None or self._status(row)!=reservation['status']:
            return False
        expected = dict(url=reservation['url'],request_id=reservation['request_id'],
                        status=reservation['status'],account_id=reservation['account_id'],
                        source=reservation['source'],resource=reservation['resource'],
                        attempt_id=reservation['attempt_id'],admission_sequence=reservation['sequence'])
        return all(row.get(key)==value for key,value in expected.items())

    def _ack_reference(self, db, reference, generation):
        previous = db.execute('SELECT receipt_id,wrapper_sha256,publication_sequence FROM safety_imports WHERE generation=? AND reference_id=?',
                              (generation,reference['reference_id'])).fetchone()
        exact = (reference['receipt_id'],reference['wrapper_sha256'],reference['sequence'])
        if previous and tuple(previous)!=exact:
            raise StateError('immutable safety reference conflict')
        db.execute('INSERT OR IGNORE INTO safety_imports VALUES (?,?,?,?,?)',
                   (generation,reference['reference_id'],*exact))

    def _require_archive(self, db, archive):
        """Caller holds publication guard before SQL; proof paths never take locks."""
        snapshot = self._archive_snapshot(archive)
        meta = db.execute('SELECT archive_generation,safety_sequence FROM operational_meta').fetchone()
        if meta['archive_generation']!=snapshot['generation']:
            raise StateError('explicit archive safety synchronization required for generation')
        if meta['safety_sequence']>snapshot['sequence']:
            raise StateError('archive safety checkpoint rolled back')
        refs = archive.safety_references(after_sequence=meta['safety_sequence'],generation=snapshot['generation'])
        for reference in refs:
            wrapper,row = self._checked_reference(archive,reference,snapshot)
            if not self._known_observation(db,wrapper,row):
                raise StateError('unresolved indexed safety reference; explicit synchronization required')
            self._ack_reference(db,reference,snapshot['generation'])
        db.execute('UPDATE operational_meta SET safety_sequence=?',(snapshot['sequence'],))
        return snapshot

    def synchronize_archive(self, archive, *, decision, policy_hash=None, allow_generation_change=False):
        """Explicit index safety import. No quota reset, unquarantine or HTTP."""
        if not decision:
            raise ValueError('recorded safety synchronization decision required')
        with archive.publication_guard(), self.db(True) as db:
            snapshot = self._archive_snapshot(archive)
            meta = db.execute('SELECT archive_generation,safety_sequence FROM operational_meta').fetchone()
            changing = meta['archive_generation'] not in (None,snapshot['generation'])
            if changing and not allow_generation_change:
                raise StateError('generation change requires explicit synchronization decision')
            if not changing and meta['archive_generation'] and meta['safety_sequence']>snapshot['sequence']:
                raise StateError('rolled-back archive checkpoint requires recovery')
            after = None if meta['archive_generation']!=snapshot['generation'] else meta['safety_sequence']
            refs = archive.safety_references(after_sequence=after,generation=snapshot['generation'])
            for reference in refs:
                wrapper,row = self._checked_reference(archive,reference,snapshot)
                if not self._known_observation(db,wrapper,row):
                    status = self._status(row)
                    provenance = 'index-reference:'+reference['reference_id']
                    if status in (402,403,429,503) and row.get('url'):
                        exists = db.execute('SELECT 1 FROM events WHERE provenance=?',(provenance,)).fetchone()
                        if not exists:
                            self._record_status(db,status,row['url'],row.get('source'),row.get('resource'),
                                                host_for_url(row['url']),provenance,policy_hash)
                            if status in (402,403) and row.get('restriction_scope')=='source' and row.get('source'):
                                self._event(db,'deny','source',row['source'],status=status,provenance=provenance,policy_hash=policy_hash)
                self._ack_reference(db,reference,snapshot['generation'])
            db.execute('UPDATE operational_meta SET archive_generation=?,safety_sequence=?',(snapshot['generation'],snapshot['sequence']))
            self._event(db,'recovery','archive',snapshot['generation'],provenance='safety-synchronization',decision=decision,
                        reason='acknowledged exact indexed safety references')
            return dict(snapshot,imported_reference_ids=[r['reference_id'] for r in refs])

    def _production_authorized(self, db, account_id):
        activation = db.execute('SELECT * FROM activations WHERE account_id=?',(account_id,)).fetchone()
        meta = db.execute('SELECT archive_generation FROM operational_meta').fetchone()
        account = db.execute('SELECT policy_hash FROM accounts WHERE id=?',(account_id,)).fetchone()
        if not activation or not account or activation['generation']!=meta[0] or activation['policy_hash']!=account[0]:
            raise StateError('production account is not explicitly activated for this ready archive/policy')

    def require_production_ready(self, archive, account_id):
        with archive.publication_guard(), self.db(True) as db:
            self._require_archive(db,archive)
            self._eligible(db,account_id)
            self._production_authorized(db,account_id)

    def activate(self, archive, account_id, *, policy_hash, decision):
        """Coordinator-only local decision for a separately authorized live scope.

        This method makes no request. C1 fixtures and integration do not activate.
        Generation replacement invalidates activation until a new decision.
        """
        if not decision:
            raise ValueError('recorded finite campaign activation decision required')
        with archive.publication_guard(), self.db(True) as db:
            snapshot = self._require_archive(db,archive)
            account = self._eligible(db,account_id,new=True)
            if account['policy_hash']!=policy_hash:
                raise StateError('activation policy mismatch')
            db.execute('INSERT INTO activations VALUES (?,?,?,?,?) ON CONFLICT(account_id) DO UPDATE SET generation=excluded.generation,policy_hash=excluded.policy_hash,decision=excluded.decision,start_wall=excluded.start_wall',
                       (account_id,snapshot['generation'],policy_hash,decision,self.clock()))
            self._event(db,'recovery','account',account_id,provenance='explicit-production-activation',policy_hash=policy_hash,
                        decision=decision,reason='finite account activation; no HTTP executed')

    def allocate_account(self, account_id, *, definition_hash, policy_hash, request_limit,
                         byte_limit, deadline=None, window=None, host_limits=None):
        for value in (request_limit, byte_limit):
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                raise ValueError('nonnegative integer quota required')
        for value in (deadline, window):
            if value is not None and (not math.isfinite(value) or value <= 0):
                raise ValueError('finite positive deadline/window required')
        values = (definition_hash, policy_hash, request_limit, byte_limit, deadline, window)
        hosts = {}
        for host, limits in (host_limits or {}).items():
            normalized = normalize_host(host)
            if normalized in hosts and hosts[normalized] != limits:
                raise ValueError('conflicting equivalent host limits')
            hosts[normalized] = limits
        with self.db(True) as db:
            row = db.execute('SELECT * FROM accounts WHERE id=?', (account_id,)).fetchone()
            if row:
                saved = tuple(row[k] for k in ('definition_hash','policy_hash','request_limit','byte_limit','deadline','window'))
                saved_hosts = {r['host']: (r['request_limit'], r['byte_limit']) for r in db.execute('SELECT * FROM host_limits WHERE account_id=?', (account_id,))}
                if saved != values or saved_hosts != hosts:
                    raise StateError('account allocation immutable')
                return
            db.execute('INSERT INTO accounts VALUES (?,?,?,?,?,?,?,?,?,?,NULL)',
                       (account_id, *values, self.clock(), self.monotonic(), self.boot))
            for host, (requests, size) in hosts.items():
                if any(not isinstance(v, int) or isinstance(v, bool) or v < 0 for v in (requests, size)):
                    raise ValueError('invalid host quota')
                db.execute('INSERT INTO host_limits VALUES (?,?,?,?)', (account_id, host, requests, size))

    def account(self, account_id):
        with self.db() as db:
            row = db.execute('SELECT * FROM accounts WHERE id=?', (account_id,)).fetchone()
            if not row:
                raise StateError('explicit durable account required')
            result = dict(row)
            spent = db.execute('SELECT COUNT(*), COALESCE(SUM(max_body),0) FROM reservations WHERE account_id=?', (account_id,)).fetchone()
            result.update(spent_requests=spent[0], spent_bytes=spent[1])
            return result

    def acquire(self, keys, owner, lease_seconds=120):
        if self.readonly:
            raise PermissionError('read-only operational state')
        self.locks.acquire(keys)
        try:
            fences = {}
            with self.db(True) as db:
                for key in sorted(set(keys)):
                    if db.execute('SELECT 1 FROM leases WHERE key=?', (key,)).fetchone():
                        raise LeaseBusy('lease requires explicit recovery: ' + key)
                    row = db.execute('SELECT generation FROM lease_fences WHERE key=?', (key,)).fetchone()
                    generation = (row[0] if row else 0) + 1
                    db.execute('INSERT INTO leases VALUES (?,?,?)', (key, owner, self.clock() + lease_seconds))
                    db.execute('INSERT INTO lease_fences VALUES (?,?,?,0) ON CONFLICT(key) DO UPDATE SET generation=excluded.generation,owner=excluded.owner', (key, generation, owner))
                    fences[key] = generation
            self.lock_owners.update({key: (owner, generation) for key,generation in fences.items()})
            return fences
        except BaseException:
            self.locks.release(keys)
            raise

    def release(self, fences, owner):
        owned = {key: generation for key,generation in fences.items()
                 if self.lock_owners.get(key) == (owner,generation)}
        try:
            with self.db(True) as db:
                for key, generation in owned.items():
                    row = db.execute('SELECT owner,generation FROM lease_fences WHERE key=?', (key,)).fetchone()
                    if row and tuple(row) == (owner, generation):
                        db.execute('DELETE FROM leases WHERE key=? AND owner=?', (key, owner))
                        db.execute('UPDATE lease_fences SET owner=NULL WHERE key=? AND generation=? AND owner=?', (key, generation, owner))
        finally:
            self.locks.release(owned)
            for key in owned:
                self.lock_owners.pop(key,None)

    def recover_lease(self, key, *, decision):
        if not decision:
            raise ValueError('recorded recovery decision required')
        with self.locks.held([key]), self.db(True) as db:
            db.execute('DELETE FROM leases WHERE key=?', (key,))
            db.execute('UPDATE lease_fences SET owner=NULL,generation=generation+1,legacy_recovered=1 WHERE key=?', (key,))
            self._event(db, 'recovery', 'lease', key, provenance='explicit-recovery', decision=decision)

    def _ownership(self, db, owner, fences):
        if not fences or any(not isinstance(v,int) or isinstance(v,bool) or v <= 0 for v in fences.values()):
            raise StateError('nonempty positive integer fence map required')
        for key, generation in fences.items():
            row = db.execute('SELECT owner,generation FROM lease_fences WHERE key=?', (key,)).fetchone()
            if not row or tuple(row) != (owner, generation) or self.lock_owners.get(key) != (owner,generation) or key not in self.locks.handles:
                raise StateError('lost lease ownership/fence')

    def _eligible(self, db, account_id, *, host=None, allocation=0, new=False):
        row = db.execute('SELECT * FROM accounts WHERE id=?', (account_id,)).fetchone()
        if not row:
            raise StateError('explicit durable account required')
        if row['stopped']:
            raise StopCollection(row['stopped'])
        if new and db.execute(_OPEN_RESERVATIONS).fetchone():
            raise StopCollection('unreconciled charged reservation')
        wall, mono = self.clock(), self.monotonic()
        if row['deadline'] is not None or row['window'] is not None:
            elapsed = mono - row['start_mono']
            if self.boot != row['boot'] or elapsed < 0 or abs((wall-row['start_wall']) - elapsed) > 2:
                raise StopCollection('clock/boot anomaly requires explicit reconciliation')
            if row['deadline'] is not None and (wall >= row['deadline'] or row['start_wall'] + elapsed >= row['deadline']):
                raise StopCollection('account deadline exhausted')
            if row['window'] is not None and elapsed >= row['window']:
                raise StopCollection('account elapsed window exhausted')
        if new:
            used = db.execute('SELECT COUNT(*),COALESCE(SUM(max_body),0) FROM reservations WHERE account_id=?', (account_id,)).fetchone()
            if used[0] + 1 > row['request_limit'] or used[1] + allocation > row['byte_limit']:
                raise StopCollection('durable quota exhausted')
            limit = db.execute('SELECT * FROM host_limits WHERE account_id=? AND host=?', (account_id, host)).fetchone()
            if limit:
                used = db.execute('SELECT COUNT(*),COALESCE(SUM(max_body),0) FROM reservations WHERE account_id=? AND host=?', (account_id,host)).fetchone()
                if used[0] + 1 > limit['request_limit'] or used[1] + allocation > limit['byte_limit']:
                    raise StopCollection('durable host quota exhausted')
        return row

    def _restrictions(self, db, url, source, resource, host):
        scopes = [('url',url), ('host',host)]
        if source:
            scopes.append(('source',source))
        if resource:
            # Resources are source-scoped; no guessed cross-source aliases.
            scopes.append(('resource',_canonical([source,resource])))
        query = """WITH wanted(scope,target) AS (VALUES %s)
            SELECT e.* FROM events e JOIN wanted w ON e.scope=w.scope AND e.target=w.target
            WHERE e.kind IN ('deny','throttle') AND (e.kind='deny' OR NOT EXISTS
                (SELECT 1 FROM event_triggers t WHERE t.event_id=e.id))
            ORDER BY e.sequence""" % ','.join('(?,?)' for scope,target in scopes)
        params = [value for scope,target in scopes for value in (scope,target)]
        return [dict(row) for row in db.execute(query,params)]

    def restrictions(self, url, *, source=None, resource=None, host=None):
        derived = host_for_url(url) if url else None
        if host is not None and normalize_host(host) != derived:
            raise StateError('restriction host does not match URL')
        with self.db() as db:
            return self._restrictions(db,url,source,resource,derived)

    def reserve(self, *, account_id, run_id, unit_revision, request_id, url, host,
                owner, fences, max_body, source=None, resource=None, reservation_id=None,
                archive=None, transport_mode=None):
        if not isinstance(max_body,int) or isinstance(max_body,bool) or max_body <= 0:
            raise ValueError('positive worst-case body allocation required')
        reservation_id = reservation_id or str(uuid.uuid4())
        if archive is None or transport_mode not in ('mock','production'):
            raise StateError('modern archive and explicit transport mode required')
        self.fault('before_reservation')
        with archive.publication_guard(), self.db(True) as db:
            self._require_archive(db,archive)
            if transport_mode == 'production':
                self._production_authorized(db,account_id)
            derived_host = host_for_url(url)
            required = {'host:'+derived_host,'request:'+request_id}
            if not derived_host or host != derived_host or set(fences) != required:
                raise StateError('exact URL host and request/host fence set required')
            self._ownership(db,owner,fences)
            old = db.execute('SELECT * FROM reservations WHERE id=?', (reservation_id,)).fetchone()
            if old:
                raise StateError('reservation ID cannot authorize redispatch')
            self._eligible(db,account_id,host=host,allocation=max_body,new=True)
            if self._restrictions(db,url,source,resource,host):
                raise StopCollection('durable source restriction')
            db.execute('INSERT INTO reservations(id,account_id,run_id,unit_revision,request_id,source,resource,url,host,attempt_id,max_body,owner,fences,state,start_wall,start_mono,boot,transport_mode) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)',
                       (reservation_id, account_id, run_id, unit_revision, request_id, source, resource,url,host,
                        'attempt-' + reservation_id, max_body, owner,_canonical(fences),'reserved', self.clock(), self.monotonic(), self.boot,transport_mode))
        self.fault('after_reservation')
        return self.reservation(reservation_id)

    def reservation(self, reservation_id):
        with self.db() as db:
            row = db.execute('SELECT * FROM reservations WHERE id=?', (reservation_id,)).fetchone()
            if not row:
                raise StateError('unknown reservation')
            return dict(row)

    def _verify_receipt(self, row, kind, receipt_id, archive):
        try:
            proof = archive.verified_receipt(receipt_id,require_ack=True)
            wrapper = proof['wrapper']
            payload = wrapper['payload']
            if wrapper['kind'] != kind:
                raise StateError('wrong receipt kind')
            required = dict(reservation_id=row['id'],account_id=row['account_id'],
                            admission_sequence=row['sequence'],request_id=row['request_id'],url=row['url'],
                            source=row['source'],resource=row['resource'])
            if any(payload.get(key) != value for key,value in required.items()):
                raise StateError('receipt does not prove this reservation')
            if kind == 'capture_attempt':
                if payload.get('owner') != row['owner'] or payload.get('lease_fences') != json.loads(row['fences']):
                    raise StateError('attempt fence/owner mismatch')
            else:
                if payload.get('attempt_id') != row['attempt_id'] or payload.get('status') != row['status']:
                    raise StateError('result attempt/status mismatch')
                if row['status'] in (402,403) and payload.get('outcome') != 'blocked':
                    raise StateError('denial cannot be downgraded in result')
                if row['status'] in (429,503) and payload.get('outcome') != 'throttled':
                    raise StateError('throttle cannot be downgraded in result')
                for key in ('artifact_sha256','response_sha256'):
                    if payload.get(key):
                        archive.resolve(payload[key])
            return payload
        except (OSError,ValueError,KeyError) as exc:
            raise StateError('verified immutable receipt required') from exc

    def transition(self, reservation_id, owner, fences, expected, state, *, archive=None, **fields):
        edges = {('reserved','attempt_published'),('attempt_published','dispatch_admitted'),('response_observed','complete')}
        if (expected,state) not in edges:
            raise StateError('transition outside admission state graph')
        allowed = {'actual_bytes','stored_bytes','result_id'}
        if not fields.keys() <= allowed:
            raise ValueError('invalid transition fields')
        if archive is None:
            raise StateError('acknowledged modern archive proof required')
        with archive.publication_guard(), self.db(True) as db:
            self._require_archive(db,archive)
            self._ownership(db,owner,fences)
            row = db.execute('SELECT * FROM reservations WHERE id=?', (reservation_id,)).fetchone()
            if not row or row['owner'] != owner or row['fences'] != _canonical(fences) or row['state'] != expected:
                raise StateError('invalid reservation transition/ownership')
            if state in ('attempt_published','dispatch_admitted'):
                self._verify_receipt(row,'capture_attempt',row['attempt_id'],archive)
            if state == 'complete':
                if not fields.get('result_id'):
                    raise StateError('complete requires verified result receipt')
                result = self._verify_receipt(row,'capture_result',fields['result_id'],archive)
                for key in ('actual_bytes','stored_bytes'):
                    if not isinstance(fields.get(key),int) or isinstance(fields.get(key),bool) or fields[key] < 0 or result.get(key) != fields[key]:
                        raise StateError('completion byte observations must match verified result')
                if fields['stored_bytes'] > fields['actual_bytes']:
                    raise StateError('stored bytes exceed observed bytes')
            if state == 'dispatch_admitted':
                if row['transport_mode'] == 'production':
                    self._production_authorized(db,row['account_id'])
                self._eligible(db,row['account_id'])
                if self._restrictions(db,row['url'],row['source'],row['resource'],row['host']):
                    raise StopCollection('restriction arrived before dispatch')
            assignments = ['state=?'] + [key + '=?' for key in fields]
            db.execute('UPDATE reservations SET ' + ','.join(assignments) + ' WHERE id=?', (state,*fields.values(),reservation_id))

    def uncertain(self, reservation_id, owner, fences, reason='interrupted attempt'):
        # A crash before this update is still blocked by the nonterminal row.
        with self.db(True) as db:
            self._ownership(db,owner,fences)
            row = db.execute('SELECT account_id,state,owner,fences FROM reservations WHERE id=?', (reservation_id,)).fetchone()
            if row and (row['owner'] != owner or row['fences'] != _canonical(fences)):
                raise StateError('uncertain finalization not owned')
            if row and row['state'] != 'complete':
                db.execute("UPDATE reservations SET state='uncertain' WHERE id=?", (reservation_id,))
                db.execute('UPDATE accounts SET stopped=? WHERE id=?', (reason,row['account_id']))

    def _event(self, db, kind, scope, target, *, status=None, provenance, eligible_wall=None,
               eligible_mono=None, policy_hash=None, decision=None, triggers=(), reason='observed restriction'):
        event_id = str(uuid.uuid4())
        db.execute('INSERT INTO events(id,kind,scope,target,status,provenance,eligible_wall,eligible_mono,boot,policy_hash,decision,triggers,reason) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)',
                   (event_id,kind,scope,target,status,provenance,eligible_wall,eligible_mono,self.boot,policy_hash,decision,_canonical(list(triggers)),reason))
        for trigger in triggers:
            db.execute('INSERT INTO event_triggers VALUES (?,?)',(event_id,trigger))
        return event_id

    def observe(self, reservation_id, owner, fences, status, *, policy_hash, throttle_seconds=0):
        """Called before a transport reads even the first body byte."""
        if not isinstance(status,int) or isinstance(status,bool) or not 100 <= status <= 599 or not math.isfinite(throttle_seconds) or throttle_seconds < 0:
            raise ValueError('valid status and nonnegative finite throttle delay required')
        with self.db(True) as db:
            self._ownership(db,owner,fences)
            row = db.execute('SELECT * FROM reservations WHERE id=?', (reservation_id,)).fetchone()
            if not row or row['owner'] != owner or row['state'] != 'dispatch_admitted' or row['fences'] != _canonical(fences):
                raise StateError('response not owned/admitted')
            account = db.execute('SELECT policy_hash FROM accounts WHERE id=?',(row['account_id'],)).fetchone()
            if account[0] != policy_hash:
                raise StateError('response policy mismatches reservation account')
            db.execute("UPDATE reservations SET state='response_observed',status=? WHERE id=?", (status,reservation_id))
            self._record_status(db,status,row['url'],row['source'],row['resource'],row['host'],reservation_id,policy_hash,throttle_seconds)
        self.fault('after_status_observed')

    def _record_status(self, db, status, url, source, resource, host, provenance, policy_hash, delay=0):
        if status in (402,403):
            self._event(db,'deny','url',url,status=status,provenance=provenance,policy_hash=policy_hash)
            if resource:
                self._event(db,'deny','resource',_canonical([source,resource]),status=status,provenance=provenance,policy_hash=policy_hash)
        elif status in (429,503):
            self._event(db,'throttle','host',host,status=status,provenance=provenance,policy_hash=policy_hash,
                        eligible_wall=self.clock()+delay,eligible_mono=self.monotonic()+delay)

    def ingest_legacy_status(self, *, status, url, host, source=None, resource=None,
                             broader_source=False, provenance, policy_hash):
        """One transaction preserves every declared legacy denial scope.

        Provenance makes repeated scans idempotent without resurrecting resumed
        throttles. C1b replaces scanning with explicit safety migration/indexes.
        """
        if host != host_for_url(url):
            raise StateError('legacy restriction host mismatch')
        with self.db(True) as db:
            if db.execute('SELECT 1 FROM events WHERE provenance=?',(provenance,)).fetchone():
                return
            self._record_status(db,status,url,source,resource,host,provenance,policy_hash)
            if status in (402,403) and broader_source and source:
                self._event(db,'deny','source',source,status=status,provenance=provenance,policy_hash=policy_hash)

    def record_restriction(self, *, kind, scope, target, provenance, policy_hash=None,
                           status=None, reason='saved policy restriction'):
        if kind not in ('deny','throttle') or scope not in ('url','source','resource','host'):
            raise ValueError('invalid restriction scope/kind')
        if scope == 'host':
            target = normalize_host(target)
        with self.db(True) as db:
            return self._event(db,kind,scope,target,status=status,provenance=provenance,policy_hash=policy_hash,
                               eligible_wall=self.clock(),eligible_mono=self.monotonic(),reason=reason)

    @staticmethod
    def _throttle_timing(db, event):
        """Effective timing of a throttle event: the newest explicit clock reconcile, else the row.

        Event rows are append-only, so a reconcile is a later `recovery` event (scope
        `throttle`, target = the throttle event id) that carries re-stamped timing.
        """
        later = db.execute("SELECT * FROM events WHERE kind='recovery' AND scope='throttle' AND target=? "
                           "AND provenance='explicit-throttle-clock-reconcile' ORDER BY sequence DESC LIMIT 1",
                           (event['id'],)).fetchone()
        if not later:
            return event
        return dict(event, boot=later['boot'], eligible_wall=later['eligible_wall'],
                    eligible_mono=later['eligible_mono'])

    def reconcile_throttle(self, event_id, *, decision):
        """Explicit decision re-stamping one active host throttle's monotonic timing, no HTTP.

        Allowed only when its timing is anomalous (boot change or wall/monotonic drift >2 s).
        The wall delay is never shortened: `eligible_wall` is kept (or advanced to now when it
        has already passed) and `eligible_mono` becomes now + the remaining wall delay.
        The original event row is untouched; a recovery event names it. Resuming still needs
        resume_throttle with its own decision, quota and policy checks.
        """
        if not decision:
            raise ValueError('throttle clock reconciliation decision required')
        with self.db(True) as db:
            row = db.execute('SELECT * FROM events WHERE id=?',(event_id,)).fetchone()
            if not row or row['kind'] != 'throttle' or row['scope'] != 'host':
                raise StateError('only an existing host throttle event can be clock-reconciled')
            if db.execute('SELECT 1 FROM event_triggers WHERE event_id=?',(event_id,)).fetchone():
                raise StateError('throttle already resumed')
            if row['eligible_wall'] is None or row['eligible_mono'] is None:
                raise StateError('throttle has no timing to reconcile')
            current = self._throttle_timing(db,row)
            wall, mono = self.clock(), self.monotonic()
            drift = (wall-current['eligible_wall'])-(mono-current['eligible_mono'])
            if current['boot'] == self.boot and abs(drift) <= 2:
                raise StateError('throttle timing is not anomalous; nothing to reconcile')
            remaining = max(0.0, current['eligible_wall']-wall)
            self._event(db,'recovery','throttle',event_id,provenance='explicit-throttle-clock-reconcile',
                        policy_hash=row['policy_hash'],decision=decision,
                        eligible_wall=max(current['eligible_wall'],wall),eligible_mono=mono+remaining,
                        reason='re-stamped throttle %s: boot %s -> %s, drift %.3fs, remaining wall delay %.3fs'
                               % (event_id,current['boot'],self.boot,drift,remaining))

    def resume_throttle(self, *, account_id, host, triggers, policy_hash, decision):
        if not decision or not triggers:
            raise ValueError('exact triggers and decision required')
        host = normalize_host(host)
        with self.db(True) as db:
            account = self._eligible(db,account_id,new=False)
            if account['policy_hash'] != policy_hash:
                raise StateError('resume policy mismatch')
            spent = db.execute('SELECT COUNT(*),COALESCE(SUM(max_body),0) FROM reservations WHERE account_id=?', (account_id,)).fetchone()
            if spent[0] >= account['request_limit'] or spent[1] >= account['byte_limit']:
                raise StopCollection('no remaining quota')
            limit = db.execute('SELECT request_limit,byte_limit FROM host_limits WHERE account_id=? AND host=?',(account_id,host)).fetchone()
            if limit:
                host_spent = db.execute('SELECT COUNT(*),COALESCE(SUM(max_body),0) FROM reservations WHERE account_id=? AND host=?',(account_id,host)).fetchone()
                if host_spent[0] >= limit[0] or host_spent[1] >= limit[1]:
                    raise StopCollection('no remaining host quota')
            # Deliberately per-account and unsettled-agnostic: a settled attempt still blocks its own account.
            if db.execute("SELECT 1 FROM reservations WHERE account_id=? AND state != 'complete'", (account_id,)).fetchone():
                raise StopCollection('uncertain attempt cannot resume')
            active = self._restrictions(db,'',None,None,host)
            expected = {r['id'] for r in active if r['kind'] == 'throttle'}
            if set(triggers) != expected:
                raise StateError('resume must name exact active host throttles')
            for event in active:
                if event['kind'] != 'throttle':
                    continue
                event = self._throttle_timing(db,event)
                if event['policy_hash'] != policy_hash or event['boot'] != self.boot or event['eligible_wall'] is None or event['eligible_mono'] is None:
                    raise StopCollection('unreconciled throttle timing/policy')
                if self.clock() < event['eligible_wall'] or self.monotonic() < event['eligible_mono']:
                    raise StopCollection('throttle not yet eligible')
                if abs((self.clock()-event['eligible_wall'])-(self.monotonic()-event['eligible_mono'])) > 2:
                    raise StopCollection('clock anomaly blocks throttle resume')
            return self._event(db,'resume','host',host,provenance=account_id,policy_hash=policy_hash,decision=decision,triggers=triggers,
                               eligible_wall=self.clock(),eligible_mono=self.monotonic(),reason='reviewed throttle resume')

    def reconcile_result(self, reservation_id, *, archive, result_id, decision):
        """Finish acknowledged durable bytes under fresh recovery fences, no HTTP.

        Original attempt ownership, observations and quota allocation are retained.
        Account stop remains until a distinct justified resume_account decision.
        """
        if not decision:
            raise ValueError('result reconciliation decision required')
        original = self.reservation(reservation_id)
        keys = sorted(json.loads(original['fences']))
        with self.locks.held(keys):
            with archive.publication_guard(), self.db(True) as db:
                self._require_archive(db,archive)
                row = db.execute('SELECT * FROM reservations WHERE id=?',(reservation_id,)).fetchone()
                self._verify_receipt(row,'capture_attempt',row['attempt_id'],archive)
                result = self._verify_receipt(row,'capture_result',result_id,archive)
                if row['state']=='complete':
                    if row['result_id']!=result_id:
                        raise StateError('complete reservation result conflict')
                    return dict(row)
                if row['status'] is None or row['state'] not in ('response_observed','uncertain'):
                    raise StateError('ambiguous request cannot finish from guessed result')
                actual,stored = result.get('actual_bytes'),result.get('stored_bytes')
                if any(not isinstance(v,int) or isinstance(v,bool) or v<0 for v in (actual,stored)) or stored>actual:
                    raise StateError('invalid result byte observations')
                for key in keys:
                    current = db.execute('SELECT generation FROM lease_fences WHERE key=?',(key,)).fetchone()
                    generation = (current[0] if current else 0)+1
                    db.execute('DELETE FROM leases WHERE key=?',(key,))
                    db.execute('INSERT INTO lease_fences VALUES (?,?,NULL,1) ON CONFLICT(key) DO UPDATE SET generation=excluded.generation,owner=NULL,legacy_recovered=1',
                               (key,generation))
                db.execute("UPDATE reservations SET state='complete',actual_bytes=?,stored_bytes=?,result_id=? WHERE id=?",
                           (actual,stored,result_id,reservation_id))
                self._event(db,'recovery','reservation',reservation_id,provenance=result_id,decision=decision,
                            reason='fresh recovery fence; original attempt ownership/quota preserved')
                completed = dict(row)
                completed.update(state='complete',actual_bytes=actual,stored_bytes=stored,result_id=result_id)
                return completed

    def settle_unobserved(self, reservation_id, *, archive, result_id, decision):
        """Explicit terminal decision for an uncertain attempt that observed no status.

        No HTTP, no refund and no invented result: the row stays `uncertain` with its
        full charge and its account stays stopped. The recorded decision only lifts the
        global fail-closed block so a distinct, separately allocated attempt may follow
        (state contract: "an explicit decision ... authorizes a distinct bounded attempt").
        Only a pre-header transport failure qualifies: its account stop reason and its
        published result receipt must both prove that no status was ever observed. A failure
        after headers (e.g. persisting the status) stays fail-closed.
        """
        if not decision:
            raise ValueError('settlement decision required')
        original = self.reservation(reservation_id)
        keys = sorted(json.loads(original['fences']))
        with self.locks.held(keys):
            with archive.publication_guard(), self.db(True) as db:
                self._require_archive(db,archive)
                row = db.execute('SELECT * FROM reservations WHERE id=?',(reservation_id,)).fetchone()
                if db.execute("SELECT 1 FROM events WHERE kind='recovery' AND scope='reservation' AND target=? "
                              "AND provenance='unobserved-settlement'",(reservation_id,)).fetchone():
                    return dict(row)
                if row['state'] != 'uncertain' or row['status'] is not None or row['result_id'] is not None:
                    raise StateError('only an uncertain attempt without an observed status can be settled unobserved')
                account = db.execute('SELECT stopped FROM accounts WHERE id=?',(row['account_id'],)).fetchone()
                if not account or account['stopped'] != 'response/transport failure':
                    raise StateError('only a pre-header transport failure can be settled unobserved')
                self._verify_receipt(row,'capture_attempt',row['attempt_id'],archive)
                result = self._verify_receipt(row,'capture_result',result_id,archive)
                if result.get('status') is not None or result.get('outcome') not in ('timeout','unavailable'):
                    raise StateError('result receipt does not prove a status-less transport failure')
                for key in keys:
                    current = db.execute('SELECT generation FROM lease_fences WHERE key=?',(key,)).fetchone()
                    generation = (current[0] if current else 0)+1
                    db.execute('DELETE FROM leases WHERE key=?',(key,))
                    db.execute('INSERT INTO lease_fences VALUES (?,?,NULL,1) ON CONFLICT(key) DO UPDATE SET generation=excluded.generation,owner=NULL,legacy_recovered=1',
                               (key,generation))
                self._event(db,'recovery','reservation',reservation_id,provenance='unobserved-settlement',decision=decision,
                            reason='no status observed (result ' + result_id + '); charge kept; no result invented; '
                                   'account stays stopped')
                return dict(row)

    def resume_account(self, archive, account_id, *, decision):
        """Separate decision after verified result reconciliation, never a refund."""
        if not decision:
            raise ValueError('account resume decision required')
        with archive.publication_guard(), self.db(True) as db:
            self._require_archive(db,archive)
            account = db.execute('SELECT * FROM accounts WHERE id=?',(account_id,)).fetchone()
            if not account or account['stopped']!='interrupted attempt':
                raise StateError('only a reconciled interruption may resume; cancellation/failure remains stopped')
            if db.execute(_OPEN_RESERVATIONS).fetchone():
                raise StateError('unreconciled request remains globally stopped')
            # A settled (still uncertain) attempt never lets its own account resume.
            if db.execute("SELECT 1 FROM reservations WHERE account_id=? AND state!='complete'",(account_id,)).fetchone():
                raise StateError('account holds an unverified attempt; only a distinct new account may proceed')
            # Check immutable time/quota using original account after clearing only
            # in this transaction; any failure rolls the stop removal back.
            db.execute('UPDATE accounts SET stopped=NULL WHERE id=?',(account_id,))
            self._eligible(db,account_id,new=True)
            self._event(db,'recovery','account',account_id,provenance='explicit-account-resume',decision=decision,
                        reason='reviewed reconciled interruption; no quota refund')

    def reconcile_clock(self, account_id, *, archive, decision):
        """Explicit decision re-baselining a timed account after a reboot or clock anomaly, no HTTP.

        Covers the three `_eligible` triggers (boot change, negative monotonic elapsed,
        wall/monotonic drift >2 s) without relaxing `_eligible`. `deadline`/`window` are
        immutable and untouched; `start_mono` and `boot` are
        re-baselined as `now_mono - consumed`, where consumed is `max(wall elapsed, mono elapsed)` on the
        same boot with a valid monotonic reading (a backward wall step never refunds time) and wall
        elapsed otherwise. Limit: after a boot change only wall time is evidence, so a wall step back
        across a reboot can understate consumption and cannot be detected. Remaining time can shrink
        but never grow on the same boot. `start_wall` stays unless monotonic consumption exceeds wall
        elapsed (backward wall step); then it moves earlier to `now_wall - consumed` so `_eligible`'s
        drift cross-check is consistent again. It is never moved later. Refused when wall elapsed is negative,
        the account is stopped or untimed, timing is healthy, or any reservation is open.
        `accounts.stopped` is never touched (see handoff: `_eligible` stops are not persisted).
        """
        if not decision:
            raise ValueError('clock reconciliation decision required')
        with archive.publication_guard(), self.db(True) as db:
            self._require_archive(db,archive)
            row = db.execute('SELECT * FROM accounts WHERE id=?',(account_id,)).fetchone()
            if not row:
                raise StateError('explicit durable account required')
            if row['stopped']:
                raise StateError('stopped account cannot be clock-reconciled')
            if row['deadline'] is None and row['window'] is None:
                raise StateError('account has no deadline/window to reconcile')
            if db.execute(_OPEN_RESERVATIONS).fetchone():
                raise StateError('unreconciled charged reservation blocks clock reconciliation')
            wall, mono = self.clock(), self.monotonic()
            wall_elapsed = wall-row['start_wall']
            if wall_elapsed < 0:
                raise StateError('wall clock is before account start; human review required')
            mono_elapsed = mono-row['start_mono']
            drift = wall_elapsed-mono_elapsed
            if self.boot == row['boot'] and mono_elapsed >= 0 and abs(drift) <= 2:
                raise StateError('account timing is not anomalous; nothing to reconcile')
            # Same boot with a valid monotonic reading: monotonic consumption is real evidence, so a
            # backward wall step must not refund it. Otherwise (boot change, or a monotonic reading
            # below the account start) monotonic is meaningless and wall elapsed is all there is.
            same_clock = self.boot == row['boot'] and mono_elapsed >= 0
            consumed = max(wall_elapsed,mono_elapsed) if same_clock else wall_elapsed
            # start_wall moves earlier only when monotonic consumption exceeds wall elapsed, so the
            # wall/monotonic cross-check in _eligible is consistent again; it never moves later.
            start_wall = wall-consumed if consumed > wall_elapsed else row['start_wall']
            db.execute('UPDATE accounts SET start_mono=?,start_wall=?,boot=? WHERE id=?',
                       (mono-consumed,start_wall,self.boot,account_id))
            self._event(db,'recovery','account',account_id,provenance='explicit-clock-reconcile',
                        policy_hash=row['policy_hash'],decision=decision,
                        reason='re-baselined start_mono/boot: boot %s -> %s, start_wall %r -> %r, drift %.3fs, '
                               'wall elapsed %.3fs, monotonic elapsed %.3fs, consumed %.3fs; no HTTP'
                               % (row['boot'],self.boot,row['start_wall'],start_wall,drift,wall_elapsed,
                                  mono_elapsed,consumed))

    def quarantine(self, *, decision):
        if not decision:
            raise ValueError('restore/recovery decision required')
        with self.db(True) as db:
            db.execute("UPDATE operational_meta SET recovery='quarantined'")
            self._event(db,'recovery','archive',self.identity(self.root),provenance='explicit-quarantine',decision=decision)

    def cancel(self, account_id, *, decision):
        if not decision:
            raise ValueError('decision required')
        with self.db(True) as db:
            db.execute('UPDATE accounts SET stopped=? WHERE id=?', ('cancelled: '+decision,account_id))
            self._event(db,'cancel','account',account_id,provenance=account_id,decision=decision)

    # Offline campaign checkpoints. Job identity binds unit revision + stage +
    # recipe; results are immutable archive receipts published before completion.

    def start_run(self, run_id, *, campaign_id, definition_sha256, identities, start_sequence, start_index_bytes, mode='offline'):
        """Start sequence/index size are the run's measured archive-growth baseline."""
        with self.db(True) as db:
            db.execute('INSERT INTO campaign_runs(id,campaign_id,definition_sha256,identities,mode,state,start_sequence,start_index_bytes,start_wall,boot) VALUES (?,?,?,?,?,?,?,?,?,?)',
                       (run_id,campaign_id,definition_sha256,_canonical(identities),mode,'running',start_sequence,start_index_bytes,self.clock(),self.boot))
        return self.campaign_run(run_id)

    def campaign_run(self, run_id):
        with self.db() as db:
            row = db.execute('SELECT * FROM campaign_runs WHERE id=?',(run_id,)).fetchone()
            if not row:
                raise StateError('unknown campaign run')
            return dict(row,identities=json.loads(row['identities']))

    def campaign_runs(self, campaign_id):
        with self.db() as db:
            return [dict(r,identities=json.loads(r['identities'])) for r in
                    db.execute('SELECT * FROM campaign_runs WHERE campaign_id=? ORDER BY start_wall,id',(campaign_id,))]

    def finish_run(self, run_id, state, reason=None):
        if state not in ('complete','stopped'):
            raise ValueError('terminal run state required')
        with self.db(True) as db:
            row = db.execute('SELECT state FROM campaign_runs WHERE id=?',(run_id,)).fetchone()
            if not row or row['state'] != 'running':
                raise StateError('only a running campaign run can finish')
            db.execute('UPDATE campaign_runs SET state=?,end_wall=?,reason=? WHERE id=?',(state,self.clock(),reason,run_id))

    def reopen_run(self, run_id, *, decision):
        """Explicit resume of a stopped run; the same definition/identities remain bound."""
        if not decision:
            raise ValueError('recorded resume decision required')
        with self.db(True) as db:
            row = db.execute('SELECT state FROM campaign_runs WHERE id=?',(run_id,)).fetchone()
            if not row or row['state'] == 'complete':
                raise StateError('only an unfinished campaign run can resume')
            db.execute("UPDATE campaign_runs SET state='running',end_wall=NULL,reason=NULL WHERE id=?",(run_id,))
            self._event(db,'recovery','campaign-run',run_id,provenance='explicit-campaign-resume',decision=decision,
                        reason='resume bound offline campaign run')

    def plan_job(self, job_id, *, campaign_id, unit_id, unit_revision, stage, recipe, run_id):
        """Idempotent: a repeated plan returns the existing durable job unchanged."""
        with self.db(True) as db:
            row = db.execute('SELECT * FROM campaign_jobs WHERE id=?',(job_id,)).fetchone()
            if row:
                if (row['campaign_id'],row['unit_id'],row['unit_revision'],row['stage'],row['recipe']) != (campaign_id,unit_id,unit_revision,stage,recipe):
                    raise StateError('job identity conflict')
                return dict(row)
            db.execute('INSERT INTO campaign_jobs(id,campaign_id,unit_id,unit_revision,stage,recipe,state,planned_run,updated_wall) VALUES (?,?,?,?,?,?,?,?,?)',
                       (job_id,campaign_id,unit_id,unit_revision,stage,recipe,'pending',run_id,self.clock()))
            return dict(db.execute('SELECT * FROM campaign_jobs WHERE id=?',(job_id,)).fetchone())

    def complete_job(self, job_id, *, run_id, result_id, outcome, action, new_bytes=0):
        """Checkpoint after the result receipt is durable; a repeat must name the same result."""
        if action not in ('executed','reused','recovered'):
            raise ValueError('unknown job action')
        with self.db(True) as db:
            run = db.execute('SELECT state FROM campaign_runs WHERE id=?',(run_id,)).fetchone()
            if not run or run['state'] != 'running':
                raise StateError('job completion requires a running campaign run')
            row = db.execute('SELECT * FROM campaign_jobs WHERE id=?',(job_id,)).fetchone()
            if not row:
                raise StateError('unknown campaign job')
            if row['state'] == 'complete':
                if (row['result_id'],row['outcome']) != (result_id,outcome):
                    raise StateError('completed job cannot change its result')
            else:
                db.execute("UPDATE campaign_jobs SET state='complete',result_id=?,outcome=?,updated_wall=? WHERE id=?",
                           (result_id,outcome,self.clock(),job_id))
            db.execute('INSERT INTO campaign_executions(job_id,run_id,action,result_id,new_bytes,wall) VALUES (?,?,?,?,?,?)',
                       (job_id,run_id,action,result_id,new_bytes,self.clock()))

    def campaign_jobs(self, campaign_id):
        with self.db() as db:
            return [dict(r) for r in db.execute('SELECT * FROM campaign_jobs WHERE campaign_id=? ORDER BY unit_id,stage,id',(campaign_id,))]

    def campaign_executions(self, run_id=None, campaign_id=None):
        with self.db() as db:
            if run_id is not None:
                rows = db.execute('SELECT * FROM campaign_executions WHERE run_id=? ORDER BY sequence',(run_id,))
            else:
                rows = db.execute('SELECT e.* FROM campaign_executions e JOIN campaign_runs r ON r.id=e.run_id WHERE r.campaign_id=? ORDER BY e.sequence',(campaign_id,))
            return [dict(r) for r in rows]

"""Immutable bytes and receipts with explicit, recoverable indexed publication."""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import tempfile
import stat
import uuid
from datetime import datetime, timezone
from contextlib import contextmanager


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode('utf-8')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    h = hashlib.sha256()
    fd = os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0))
    with os.fdopen(fd, 'rb') as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode):
            raise ValueError('regular artifact file required')
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
        after = os.fstat(stream.fileno())
        if (before.st_size,before.st_mtime_ns,before.st_ctime_ns) != (after.st_size,after.st_mtime_ns,after.st_ctime_ns):
            raise ValueError('file changed while hashing')
    return h.hexdigest()


def _hash(value):
    if not isinstance(value, str) or not re.fullmatch('[0-9a-f]{64}', value):
        raise ValueError('invalid SHA-256')
    return value


def _safe(base, path):
    base = Path(base).absolute()
    path = Path(path)
    path = path if path.is_absolute() else base / path
    if '..' in path.parts:
        raise ValueError('parent traversal forbidden')
    try:
        path.relative_to(base)
    except ValueError:
        raise ValueError('path outside root') from None
    current = base
    if current.is_symlink():
        raise ValueError('symlink forbidden')
    for part in path.relative_to(base).parts:
        current = current / part
        if current.is_symlink():
            raise ValueError('symlink forbidden')
    # Also reject symlink ancestors of the root.
    for parent in base.parents:
        if parent.is_symlink():
            raise ValueError('symlink ancestor forbidden')
    return path


def _fsync_directory(path):
    fd = os.open(path, os.O_RDONLY | getattr(os, 'O_DIRECTORY', 0))
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def _mkdir(path):
    """Persist every newly created directory entry, including hash prefixes."""
    if path.is_dir():
        return
    _mkdir(path.parent)
    try:
        path.mkdir()
    except FileExistsError:
        if not path.is_dir():
            raise
    _fsync_directory(path.parent)


def _publish(path, data):
    """Publish complete bytes without replacing an existing immutable pathname."""
    _mkdir(path.parent)
    fd, staging = tempfile.mkstemp(prefix='.pending-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(staging, 0o444)
        try:
            os.link(staging, path)  # link our private staging inode, never a legacy file
        except FileExistsError:
            if path.read_bytes() != data:
                raise ValueError('immutable path has conflicting bytes')
        _fsync_directory(path.parent)
    finally:
        os.unlink(staging)


def _replace(path, data):
    """Durable replacement is reserved for the mutable operational head."""
    fd, staging = tempfile.mkstemp(prefix='.pending-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(staging, path)
        _fsync_directory(path.parent)
    finally:
        if os.path.exists(staging):
            os.unlink(staging)


class ArchiveRecoveryError(ValueError):
    """Archive authority is unavailable or requires explicit reconciliation."""


INDEX_VERSION = 2
_SCHEMA = '''
CREATE TABLE index_meta(version INTEGER NOT NULL, generation TEXT NOT NULL, sequence INTEGER NOT NULL);
CREATE TABLE receipts(id TEXT PRIMARY KEY,kind TEXT NOT NULL,created_at TEXT NOT NULL,payload TEXT NOT NULL,
    wrapper BLOB NOT NULL,wrapper_sha256 TEXT NOT NULL,sequence INTEGER);
CREATE INDEX receipts_kind ON receipts(kind,id);
CREATE TABLE artifacts(sha256 TEXT PRIMARY KEY,size INTEGER NOT NULL);
CREATE TABLE locations(sha256 TEXT NOT NULL,scope TEXT NOT NULL,path TEXT NOT NULL,PRIMARY KEY(sha256,scope,path));
CREATE TABLE publication_intent(slot INTEGER PRIMARY KEY CHECK(slot=1),action TEXT NOT NULL,
    receipt_id TEXT,wrapper BLOB,wrapper_sha256 TEXT,previous_head BLOB,
    generation TEXT NOT NULL,previous_sequence INTEGER NOT NULL,next_sequence INTEGER NOT NULL,indexed INTEGER NOT NULL);
CREATE TABLE capture_lookup(receipt_id TEXT PRIMARY KEY,request_id TEXT,url TEXT,host TEXT,
    source TEXT,resource TEXT,session_scope TEXT,observation_scope TEXT,context_known INTEGER NOT NULL,
    admission_sequence INTEGER,outcome TEXT);
CREATE INDEX capture_request ON capture_lookup(request_id,source,session_scope,observation_scope,admission_sequence);
CREATE INDEX capture_url ON capture_lookup(url);
CREATE INDEX capture_host ON capture_lookup(host);
CREATE TABLE policy_lookup(receipt_id TEXT PRIMARY KEY,url TEXT);
CREATE INDEX policy_url ON policy_lookup(url);
CREATE TABLE derivation_lookup(receipt_id TEXT PRIMARY KEY,recipe_id TEXT,input_fingerprint TEXT,status TEXT);
CREATE INDEX derivation_recipe_inputs ON derivation_lookup(recipe_id,input_fingerprint,status);
CREATE TABLE safety_lookup(reference_id TEXT PRIMARY KEY,receipt_id TEXT NOT NULL,wrapper_sha256 TEXT NOT NULL,
    sequence INTEGER NOT NULL,selector TEXT NOT NULL,row TEXT NOT NULL,url TEXT,host TEXT,
    source TEXT,resource TEXT,status INTEGER,broader_source INTEGER NOT NULL);
CREATE INDEX safety_sequence ON safety_lookup(sequence,reference_id);
CREATE INDEX safety_url ON safety_lookup(url,status);
CREATE INDEX safety_host ON safety_lookup(host,status);
CREATE INDEX safety_resource ON safety_lookup(source,resource,status);
CREATE INDEX safety_source ON safety_lookup(source,broader_source,status);
'''


class Archive:
    def __init__(self, root, workspace=None, create=False, readonly=False, *, fault=None):
        self._setup(root, workspace, readonly, fault)
        if create and readonly:
            raise ValueError('cannot create read-only archive')
        if create:
            if self.root.exists() and any(self.root.iterdir()):
                raise ArchiveRecoveryError('create requires a truly new archive; use explicit upgrade/rebuild')
            _mkdir(self.root)
            for name in ('objects', 'receipts'):
                _mkdir(_safe(self.root, self.root / name))
            with self.publication_guard():
                self._connect(create=True)
                with self.db:
                    self._schema()
                    generation = str(uuid.uuid4())
                    self.db.execute('INSERT INTO index_meta VALUES (?,?,?)', (INDEX_VERSION,generation,0))
                    self._generation_intent(generation, None)
                self._bound_generation=generation
                _fsync_directory(self.root)
                self._recover_locked()
            return
        if not self.root.is_dir():
            raise FileNotFoundError(self.root)
        if not self.database.is_file():
            if readonly:
                self.db = None
                return
            raise ArchiveRecoveryError('missing archive index; explicit rebuild required')
        self._connect()
        if not readonly and self._legacy():
            self.close()
            raise ArchiveRecoveryError('legacy archive requires explicit upgrade')
        # Ordinary opens intentionally do not scan, recover, migrate or write.

    def _setup(self, root, workspace, readonly=False, fault=None):
        self.root = _safe(Path(root).absolute(), Path(root).absolute())
        self.workspace = Path(workspace).absolute() if workspace else self.root.parent
        # In-place registrations are workspace-relative forever; never guess the base.
        self.workspace_explicit = workspace is not None
        _safe(self.workspace,self.workspace)
        self.database = _safe(self.root,self.root/'index.sqlite')
        self.head_path = _safe(self.root,self.root/'index-head.json')
        self.readonly = readonly
        self.fault = fault or (lambda point: None)
        self.db = None
        self._guard_held = False
        self._bound_generation = None

    def _connect(self, create=False):
        mode = 'ro' if self.readonly else 'rwc' if create else 'rw'
        try:
            self.db = sqlite3.connect(self.database.as_uri()+'?mode='+mode,uri=True)
            self.db.row_factory = sqlite3.Row
            if not self.readonly:
                self.db.execute('PRAGMA synchronous=FULL')
            try:
                row=self.db.execute('SELECT generation FROM index_meta').fetchone()
                self._bound_generation=row[0] if row else None
            except sqlite3.DatabaseError:
                self._bound_generation=None
        except sqlite3.DatabaseError as exc:
            self.close()
            raise ArchiveRecoveryError('invalid archive index') from exc

    def _schema(self):
        for statement in _SCHEMA.split(';'):
            if statement.strip():
                self.db.execute(statement)

    def close(self):
        if self.db is not None:
            self.db.close()
            self.db = None

    def _write(self):
        if self.readonly:
            raise PermissionError('read-only archive')

    @contextmanager
    def publication_guard(self):
        """Acquire before operational SQL, release before HTTP; no recovery here."""
        self._write()
        from .campaign_state import FileLocks
        if self._guard_held:
            raise ArchiveRecoveryError('publication guard is not reentrant')
        locks = FileLocks(_safe(self.root,self.root/'capture-locks'))
        with locks.held(['publication']):
            self._guard_held = True
            try:
                yield self
            finally:
                self._guard_held = False

    def _legacy(self):
        if self.db is None:
            return False
        tables = {r[0] for r in self.db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        return tables == {'receipts','artifacts','locations'} and not self.head_path.exists()

    def _head(self):
        raw = self.head_path.read_bytes()
        head = json.loads(raw)
        if (set(head) != {'version','generation','sequence'} or head['version'] != INDEX_VERSION
                or not isinstance(head['generation'],str) or not head['generation']
                or type(head['sequence']) is not int or head['sequence'] < 0):
            raise ArchiveRecoveryError('invalid archive head')
        return raw,head

    def _meta(self):
        rows = self.db.execute('SELECT * FROM index_meta').fetchall()
        if len(rows)!=1 or rows[0]['version']!=INDEX_VERSION or type(rows[0]['sequence']) is not int or rows[0]['sequence']<0:
            raise ArchiveRecoveryError('unsupported/invalid archive index schema')
        return dict(rows[0])

    def readiness(self):
        result = dict(status='needs_recovery',reason=None,generation=None,sequence=None,
                      acknowledged_sequence=None,pending=None)
        try:
            if self.db is None:
                raise ArchiveRecoveryError('missing archive index')
            if self._legacy():
                return dict(result,status='legacy_readonly',reason='explicit upgrade required')
            raw,head = self._head()
            with self.db:
                meta = self._meta()
                pending = self.db.execute('SELECT * FROM publication_intent').fetchall()
            result.update(generation=meta['generation'],sequence=meta['sequence'],
                          acknowledged_sequence=head['sequence'],pending=bool(pending))
            if self._bound_generation is not None and meta['generation']!=self._bound_generation:
                raise ArchiveRecoveryError('archive generation changed; reopen explicitly')
            if self.head_path.read_bytes()!=raw:
                raise ArchiveRecoveryError('archive head changed during read; retry readiness')
            if head['generation']!=meta['generation'] or head['sequence']!=meta['sequence']:
                raise ArchiveRecoveryError('archive index/head generation or sequence mismatch')
            if pending:
                raise ArchiveRecoveryError('pending publication requires recovery')
            return dict(result,status='ready')
        except (OSError,ValueError,KeyError,TypeError,sqlite3.DatabaseError) as exc:
            return dict(result,reason=str(exc))

    def require_ready(self):
        ready = self.readiness()
        if ready['status']!='ready':
            raise ArchiveRecoveryError(ready['reason'] or 'archive requires recovery')
        return ready

    def _head_bytes(self, generation, sequence):
        return canonical(dict(version=INDEX_VERSION,generation=generation,sequence=sequence))

    def _generation_intent(self, generation, previous_head):
        self.db.execute('INSERT INTO publication_intent VALUES (1,?,?,?,?,?,?,?,?,?)',
                        ('generation',None,None,None,previous_head,generation,0,0,1))

    def _recover_locked(self):
        meta = self._meta()
        intents = self.db.execute('SELECT * FROM publication_intent').fetchall()
        if not intents:
            self.require_ready()
            return []
        if len(intents)!=1:
            raise ArchiveRecoveryError('invalid publication intent count')
        intent = dict(intents[0])
        if intent['generation']!=meta['generation']:
            raise ArchiveRecoveryError('publication intent generation mismatch')
        current = self.head_path.read_bytes() if self.head_path.exists() else None
        expected_new = self._head_bytes(intent['generation'],intent['next_sequence'])
        if current not in (intent['previous_head'],expected_new):
            raise ArchiveRecoveryError('head contradicts publication intent')
        if intent['action']=='generation':
            if not intent['indexed'] or meta['sequence']!=0 or intent['next_sequence']!=0:
                raise ArchiveRecoveryError('invalid generation transition')
            # Explicit recovery may scan its validated generation, never normal open.
            for row in self.db.execute('SELECT id FROM receipts ORDER BY id').fetchall():
                wrapper = self._indexed_wrapper(row[0])
                self._dependencies(wrapper)
            identifiers = []
        elif intent['action']=='receipt':
            raw = bytes(intent['wrapper'])
            if digest(raw)!=intent['wrapper_sha256']:
                raise ArchiveRecoveryError('corrupt frozen publication intent')
            wrapper = self._parse_receipt(raw,self._receipt_path(intent['receipt_id']))
            phase_sequence = intent['next_sequence'] if intent['indexed'] else intent['previous_sequence']
            if intent['indexed'] not in (0,1) or meta['sequence'] != phase_sequence:
                raise ArchiveRecoveryError('index sequence contradicts publication intent phase')
            if intent['next_sequence']!=intent['previous_sequence']+1:
                raise ArchiveRecoveryError('invalid publication intent sequence')
            if not intent['indexed'] and current==expected_new:
                raise ArchiveRecoveryError('head advanced without indexed intent')
            previous = json.loads(intent['previous_head'])
            if previous != dict(version=INDEX_VERSION,generation=intent['generation'],sequence=intent['previous_sequence']):
                raise ArchiveRecoveryError('previous head contradicts publication intent')
            self._dependencies(wrapper)
            try:
                _publish(self._receipt_path(wrapper['id']),raw)
            except ValueError as exc:
                raise ArchiveRecoveryError('immutable receipt conflicts with frozen intent') from exc
            self.fault('after_file')
            with self.db:
                self.db.execute('BEGIN IMMEDIATE')
                if not intent['indexed']:
                    self._index_row(wrapper,raw,intent['next_sequence'])
                    self.db.execute('UPDATE index_meta SET sequence=?',(intent['next_sequence'],))
                    self.db.execute('UPDATE publication_intent SET indexed=1 WHERE slot=1')
                else:
                    known = self.db.execute('SELECT wrapper,wrapper_sha256,sequence FROM receipts WHERE id=?',(wrapper['id'],)).fetchone()
                    if (known is None or bytes(known['wrapper']) != raw
                            or known['wrapper_sha256'] != intent['wrapper_sha256']
                            or known['sequence'] != intent['next_sequence']):
                        raise ArchiveRecoveryError('indexed receipt contradicts frozen intent')
                    self._indexed_wrapper(wrapper['id'])
            self.fault('after_index')
            identifiers = [wrapper['id']]
        else:
            raise ArchiveRecoveryError('unknown publication intent operation')
        _replace(self.head_path,expected_new)
        self.fault('after_head')
        with self.db:
            self.db.execute('DELETE FROM publication_intent WHERE slot=1')
        self.fault('after_ack')
        return identifiers

    def _prepare_locked(self):
        self._write()
        if self._legacy():
            raise ArchiveRecoveryError('legacy archive requires explicit upgrade')
        self._recover_locked()

    def object_path(self, sha256):
        sha256 = _hash(sha256)
        return _safe(self.root,self.root/'objects'/sha256[:2]/sha256)

    def _receipt_path(self, receipt_id):
        if not isinstance(receipt_id,str) or not re.fullmatch('[A-Za-z0-9_-]{1,160}',receipt_id):
            raise ValueError('invalid receipt ID')
        return _safe(self.root,self.root/'receipts'/(receipt_id+'.json'))

    def receipt(self, kind, payload, receipt_id=None):
        with self.publication_guard():
            self._prepare_locked()
            return self._receipt_locked(kind,payload,receipt_id)

    def _receipt_locked(self, kind, payload, receipt_id=None):
        if not isinstance(kind,str) or not kind:
            raise ValueError('receipt kind required')
        payload = json.loads(canonical(payload))
        receipt_id = receipt_id or str(uuid.uuid4())
        path = self._receipt_path(receipt_id)
        row = self.db.execute('SELECT id FROM receipts WHERE id=?',(receipt_id,)).fetchone()
        if row:
            existing = self._indexed_wrapper(receipt_id)
            if existing['kind']!=kind or canonical(existing['payload'])!=canonical(payload):
                raise ValueError('receipt ID conflict')
            self._dependencies(existing)
            return receipt_id
        if path.exists():
            raise ArchiveRecoveryError('unindexed receipt requires explicit rebuild')
        wrapper = dict(id=receipt_id,kind=kind,created_at=datetime.now(timezone.utc).isoformat(),payload=payload)
        self._registration(wrapper)
        self._dependencies(wrapper)
        raw = canonical(wrapper)
        meta = self._meta()
        with self.db:
            self.db.execute('BEGIN IMMEDIATE')
            self.db.execute('INSERT INTO publication_intent VALUES (1,?,?,?,?,?,?,?,?,?)',
                            ('receipt',receipt_id,raw,digest(raw),self.head_path.read_bytes(),
                             meta['generation'],meta['sequence'],meta['sequence']+1,0))
        self.fault('after_intent')
        self._recover_locked()
        return receipt_id

    def _parse_receipt(self, raw, path):
        wrapper = json.loads(raw)
        if not isinstance(wrapper,dict) or set(wrapper)!={'id','kind','created_at','payload'} or path.name!=wrapper['id']+'.json':
            raise ValueError('invalid receipt wrapper')
        self._receipt_path(wrapper['id'])
        if not isinstance(wrapper['kind'],str) or not wrapper['kind'] or not isinstance(wrapper['created_at'],str):
            raise ValueError('invalid receipt metadata')
        canonical(wrapper)
        self._registration(wrapper)
        return wrapper

    def _read_receipt(self, path):
        _safe(self.root,path)
        return self._parse_receipt(path.read_bytes(),path)

    def _indexed_wrapper(self, receipt_id):
        row = self.db.execute('SELECT wrapper,wrapper_sha256 FROM receipts WHERE id=?',(receipt_id,)).fetchone()
        if row is None:
            raise KeyError(receipt_id)
        raw = self._receipt_path(receipt_id).read_bytes()
        if digest(raw)!=row['wrapper_sha256'] or raw!=bytes(row['wrapper']):
            raise ArchiveRecoveryError('selected receipt disagrees with index')
        return self._parse_receipt(raw,self._receipt_path(receipt_id))

    def receipts(self, kind=None):
        """Explicit whole-history enumeration; only audit/rebuild use file scans."""
        if self.db is None or self._legacy():
            rows = [self._read_receipt(p) for p in sorted(_safe(self.root,self.root/'receipts').glob('*.json'))]
            return [r for r in rows if kind is None or r['kind']==kind]
        self.require_ready()
        sql = 'SELECT id FROM receipts'+(' WHERE kind=?' if kind is not None else '')+' ORDER BY id'
        return [self._indexed_wrapper(r[0]) for r in self.db.execute(sql,() if kind is None else (kind,)).fetchall()]

    def get_receipt(self, receipt_id):
        self._receipt_path(receipt_id)
        if self._legacy():
            return self._read_receipt(self._receipt_path(receipt_id))
        self.require_ready()
        return self._indexed_wrapper(receipt_id)

    def verified_receipt(self, receipt_id, *, require_ack=True):
        ready = self.require_ready() if require_ack else self.readiness()
        if ready['generation'] is None:
            raise ArchiveRecoveryError('modern indexed proof required')
        wrapper = self._indexed_wrapper(receipt_id)
        self._dependencies(wrapper)
        row = self.db.execute('SELECT wrapper_sha256,sequence FROM receipts WHERE id=?',(receipt_id,)).fetchone()
        after = self.require_ready() if require_ack else self.readiness()
        if (after['generation'],after['sequence'])!=(ready['generation'],ready['sequence']):
            raise ArchiveRecoveryError('archive changed during selected receipt proof')
        return dict(wrapper=wrapper,wrapper_sha256=row['wrapper_sha256'],generation=ready['generation'],
                    sequence=row['sequence'],acknowledged_sequence=ready['acknowledged_sequence'])

    def _registration(self, wrapper):
        if wrapper['kind']!='artifact-registration':
            return None
        p = wrapper['payload']
        _hash(p['sha256'])
        if p['scope'] not in ('archive','workspace') or not isinstance(p['path'],str) or Path(p['path']).is_absolute():
            raise ValueError('invalid location scope/path')
        if type(p['size']) is not int or p['size']<0:
            raise ValueError('invalid artifact size')
        base = self.root if p['scope']=='archive' else self.workspace
        _safe(base,p['path'])
        if p['scope']=='archive' and p['path']!=self.object_path(p['sha256']).relative_to(self.root).as_posix():
            raise ValueError('invalid object location')
        return p

    def _dependencies(self, wrapper, visited=None):
        """Only explicitly required evidence fields; code/config hashes are not bytes."""
        visited = set() if visited is None else visited
        if wrapper['id'] in visited:
            raise ValueError('cyclic receipt dependencies')
        visited = visited | {wrapper['id']}
        p,kind = wrapper['payload'],wrapper['kind']
        registration = self._registration(wrapper)
        if registration:
            base = self.root if registration['scope']=='archive' else self.workspace
            path = _safe(base,registration['path'])
            if not path.is_file() or path.stat().st_size!=registration['size'] or file_digest(path)!=registration['sha256']:
                raise ValueError('missing or corrupt registered artifact')
            return
        hashes,refs = [],[]
        if kind in ('saved-derivation','derivation-run'):
            hashes.extend(i['sha256'] for i in p['inputs']+p['outputs'])
            if p.get('recipe_id'):
                refs.append(p['recipe_id'])
        elif kind=='legacy-capture':
            hashes.extend(p[k] for k in ('artifact_sha256','bundle_sha256') if p.get(k))
        elif kind=='reference-document':
            hashes.extend([p['artifact_sha256'],p['text_sha256'],p['entry']['pdf']['sha256'],p['entry']['text']['sha256']])
            refs.extend(p[k] for k in ('capture_id','derivation_id') if p.get(k))
        elif kind=='reconstruction-lineage':
            hashes.extend(p[k] for k in ('artifact_sha256','original_sha256') if p.get(k))
            refs.extend(p.get('capture_ids',[]))
            if p.get('derivation_id'):
                refs.append(p['derivation_id'])
        elif kind in ('capture_result','capture_reuse'):
            hashes.extend(p[k] for k in ('artifact_sha256','response_sha256') if p.get(k))
            refs.extend(p[k] for k in ('attempt_id','prior_capture_id') if p.get(k))
        elif kind=='derivation-consumer':
            refs.append(p['run_id'])
        elif kind in ('campaign-register','campaign-validate','campaign-bundle'):
            hashes.extend(i['sha256'] for i in p.get('inputs',[])+p.get('outputs',[]))
            refs.extend(p[k] for k in ('registration_id','validation_id') if p.get(k))
        for sha in set(hashes):
            self._resolve(sha)
        for identifier in set(refs):
            self._dependencies(self._indexed_wrapper(identifier),visited)

    def _status_rows(self, wrapper):
        payload,kind=wrapper['payload'],wrapper['kind']
        if kind=='capture_result':
            return [('payload',payload)]
        if kind=='capture_policy':
            return [('payload.saved_failure',payload.get('saved_failure',{}))]
        if kind=='legacy-capture' and payload.get('http_status') in (402,403,429,503):
            return [('payload',dict(payload,url=payload.get('requested_url') or payload.get('source_url'),status=payload['http_status']))]
        return []

    def safety_row(self, wrapper, selector):
        """Canonical pure selector adaptation, shared with operational import."""
        rows=dict(self._status_rows(wrapper))
        if selector not in rows:
            raise ArchiveRecoveryError('unsupported safety row selector')
        return rows[selector]

    @staticmethod
    def safety_status(row):
        status=row.get('status')
        if status not in (402,403,429,503):
            status=next((code for code in (402,403,429,503) if str(code) in str(row.get('error') or '')),None)
        return status

    _status=safety_status

    def _index_row(self, wrapper, raw, sequence):
        p = self._registration(wrapper)
        self.db.execute('INSERT INTO receipts VALUES (?,?,?,?,?,?,?)',
                        (wrapper['id'],wrapper['kind'],wrapper['created_at'],canonical(wrapper['payload']).decode(),raw,digest(raw),sequence))
        if p:
            known = self.db.execute('SELECT size FROM artifacts WHERE sha256=?',(p['sha256'],)).fetchone()
            if known and known['size']!=p['size']:
                raise ValueError('conflicting artifact size')
            self.db.execute('INSERT OR IGNORE INTO artifacts VALUES (?,?)',(p['sha256'],p['size']))
            self.db.execute('INSERT OR IGNORE INTO locations VALUES (?,?,?)',(p['sha256'],p['scope'],p['path']))
        payload,kind = wrapper['payload'],wrapper['kind']
        from .campaign_state import host_for_url
        def host(url):
            try:
                return host_for_url(url) if url else None
            except ValueError:
                return None
        if kind=='capture_result':
            order = payload.get('admission_sequence')
            order = order if type(order) is int and order>0 else None
            self.db.execute('INSERT INTO capture_lookup VALUES (?,?,?,?,?,?,?,?,?,?,?)',
                (wrapper['id'],payload.get('request_id'),payload.get('url'),host(payload.get('url')),
                 payload.get('source'),payload.get('resource'),payload.get('session_scope'),payload.get('observation_scope'),
                 int(payload.get('request_context_known') is True),order,payload.get('outcome')))
        if kind=='capture_policy':
            self.db.execute('INSERT INTO policy_lookup VALUES (?,?)',(wrapper['id'],payload.get('saved_failure',{}).get('url')))
        if kind=='derivation-run':
            self.db.execute('INSERT INTO derivation_lookup VALUES (?,?,?,?)',
                (wrapper['id'],payload.get('recipe_id'),digest(canonical(payload.get('inputs'))),payload.get('status')))
        for selector,row in self._status_rows(wrapper):
            status=self._status(row)
            if status not in (402,403,429,503) or not row.get('url'):
                continue
            reference_id = digest(canonical(dict(receipt_id=wrapper['id'],wrapper_sha256=digest(raw),selector=selector)))
            self.db.execute('INSERT INTO safety_lookup VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
                (reference_id,wrapper['id'],digest(raw),sequence or 0,selector,canonical(row).decode(),row['url'],host(row['url']),
                 row.get('source'),row.get('resource'),status,int(row.get('restriction_scope')=='source')))

    def capture_candidates(self, request_id, *, source=None, session_scope=None, observation_scope=None, live=False, scoped=False):
        """Live reuse needs known compatible context; offline replay honours a declared scope."""
        self.require_ready()
        sql = 'SELECT * FROM capture_lookup WHERE request_id=?'
        args = [request_id]
        if live:
            sql += ' AND context_known=1'
        if live or scoped:
            sql += ' AND source IS ? AND session_scope IS ? AND observation_scope IS ?'
            args += [source,session_scope,observation_scope]
        sql += ' ORDER BY (admission_sequence IS NOT NULL),admission_sequence,receipt_id'
        result=[]
        for r in self.db.execute(sql,args).fetchall():
            wrapper=self._indexed_wrapper(r['receipt_id'])
            p=wrapper['payload']
            order=p.get('admission_sequence')
            order=order if type(order) is int and order>0 else None
            if (wrapper['kind']!='capture_result' or p.get('request_id')!=request_id
                    or order!=r['admission_sequence'] or p.get('outcome')!=r['outcome']
                    or (live and p.get('request_context_known') is not True)
                    or ((live or scoped) and (p.get('source')!=source or p.get('session_scope')!=session_scope
                        or p.get('observation_scope')!=observation_scope))):
                raise ArchiveRecoveryError('capture lookup disagrees with selected wrapper')
            result.append(wrapper)
        return result

    def policy_candidates(self, url):
        self.require_ready()
        result=[]
        for r in self.db.execute('SELECT receipt_id FROM policy_lookup WHERE url=? ORDER BY receipt_id',(url,)).fetchall():
            wrapper=self._indexed_wrapper(r[0])
            if wrapper['kind']!='capture_policy' or wrapper['payload'].get('saved_failure',{}).get('url')!=url:
                raise ArchiveRecoveryError('policy lookup disagrees with selected wrapper')
            result.append(wrapper)
        return result

    def derivation_candidates(self, recipe_id, inputs):
        self.require_ready()
        result=[]
        for r in self.db.execute(
            "SELECT receipt_id FROM derivation_lookup WHERE recipe_id=? AND input_fingerprint=? AND status='success' ORDER BY receipt_id",
            (recipe_id,digest(canonical(inputs)))).fetchall():
            wrapper=self._indexed_wrapper(r[0])
            p=wrapper['payload']
            if wrapper['kind']!='derivation-run' or p.get('recipe_id')!=recipe_id or p.get('inputs')!=inputs or p.get('status')!='success':
                raise ArchiveRecoveryError('derivation lookup disagrees with selected wrapper')
            result.append(wrapper)
        return result

    def safety_references(self, *, after_sequence=None, generation=None, url=None, host=None, source=None, resource=None):
        ready = self.require_ready()
        if generation is not None and generation!=ready['generation']:
            raise ArchiveRecoveryError('safety reference generation mismatch')
        predicates,args = [],[]
        if after_sequence is not None:
            predicates.append('sequence>?')
            args.append(after_sequence)
        scopes=[]
        if url is not None:
            scopes.append('(url=? AND status IN (402,403))')
            args.append(url)
        if host is not None:
            scopes.append('(host=? AND status IN (429,503))')
            args.append(host)
        if source is not None:
            scopes.append('(source=? AND broader_source=1 AND status IN (402,403))')
            args.append(source)
        if resource is not None:
            scopes.append('(source IS ? AND resource=? AND status IN (402,403))')
            args.extend([source,resource])
        if scopes:
            predicates.append('('+' OR '.join(scopes)+')')
        sql='SELECT * FROM safety_lookup'+(' WHERE '+' AND '.join(predicates) if predicates else '')+' ORDER BY sequence,reference_id'
        result=[]
        for r in self.db.execute(sql,args).fetchall():
            wrapper=self._indexed_wrapper(r['receipt_id'])
            rows=dict(self._status_rows(wrapper))
            row=rows.get(r['selector'])
            stored=self.db.execute('SELECT wrapper_sha256,sequence FROM receipts WHERE id=?',(r['receipt_id'],)).fetchone()
            expected_id=digest(canonical(dict(receipt_id=r['receipt_id'],wrapper_sha256=stored['wrapper_sha256'],selector=r['selector'])))
            from .campaign_state import host_for_url
            try:
                expected_host=host_for_url(row['url']) if row and row.get('url') else None
            except ValueError:
                expected_host=None
            if (row is None or canonical(row).decode()!=r['row'] or expected_id!=r['reference_id']
                    or r['wrapper_sha256']!=stored['wrapper_sha256'] or r['sequence']!=(stored['sequence'] or 0)
                    or r['url']!=row.get('url') or r['host']!=expected_host
                    or r['source']!=row.get('source') or r['resource']!=row.get('resource')
                    or r['status']!=self._status(row) or r['broader_source']!=int(row.get('restriction_scope')=='source')):
                raise ArchiveRecoveryError('safety lookup disagrees with selected wrapper')
            result.append(dict(reference_id=r['reference_id'],receipt_id=r['receipt_id'],wrapper_sha256=r['wrapper_sha256'],sequence=r['sequence'],selector=r['selector'],row=row))
        return result

    def register_file(self, path, expected_sha256=None, copy=False):
        if not copy and not self.workspace_explicit:
            raise ValueError('in-place registration requires an explicit workspace root')
        with self.publication_guard():
            self._prepare_locked()
            path = _safe(self.workspace,path)
            if not path.is_file():
                raise FileNotFoundError(path)
            before=path.stat()
            sha=file_digest(path)
            after=path.stat()
            if (before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)!=(after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns):
                raise ValueError('file changed during registration')
            if expected_sha256 is not None and sha!=_hash(expected_sha256):
                raise ValueError('artifact hash mismatch')
            if copy:
                data=path.read_bytes()
                if digest(data)!=sha:
                    raise ValueError('file changed during registration')
                return self._store_bytes_locked(data)
            payload=dict(sha256=sha,size=after.st_size,scope='workspace',path=path.relative_to(self.workspace).as_posix(),acquired_at=None,collector_version=None,metadata_status='unknown')
            self._receipt_locked('artifact-registration',payload,'registration-'+digest(canonical(payload)))
            return sha

    def store_bytes(self, data):
        with self.publication_guard():
            self._prepare_locked()
            return self._store_bytes_locked(data)

    def _store_bytes_locked(self, data):
        if not isinstance(data,bytes):
            raise TypeError('bytes required')
        sha=digest(data)
        path=self.object_path(sha)
        _publish(path,data)
        self.fault('after_object')
        payload=dict(sha256=sha,size=len(data),scope='archive',path=path.relative_to(self.root).as_posix(),acquired_at=None,collector_version=None,metadata_status='unknown')
        self._receipt_locked('artifact-registration',payload,'registration-'+digest(canonical(payload)))
        return sha

    def resolve(self, sha256):
        if not self._legacy():
            self.require_ready()
        return self._resolve(sha256)

    def _resolve(self, sha256):
        sha256=_hash(sha256)
        rows=self.db.execute('SELECT scope,path FROM locations WHERE sha256=? ORDER BY scope,path',(sha256,)).fetchall()
        failures=[]
        for row in rows:
            path=_safe(self.root if row['scope']=='archive' else self.workspace,row['path'])
            if path.is_file() and file_digest(path)==sha256:
                size=self.db.execute('SELECT size FROM artifacts WHERE sha256=?',(sha256,)).fetchone()
                if size is not None and path.stat().st_size==size[0]:
                    return path
            failures.append(str(path))
        raise ValueError('missing or corrupt artifact: '+sha256+' '+', '.join(failures))

    @classmethod
    def _maintenance(cls, root, workspace, fault=None):
        archive=cls.__new__(cls)
        archive._setup(root,workspace,False,fault)
        if not archive.root.is_dir():
            raise FileNotFoundError(archive.root)
        return archive

    @classmethod
    def recover(cls, root, workspace=None, *, decision=None, fault=None):
        if not decision:
            raise ValueError('explicit recovery decision required')
        archive=cls._maintenance(root,workspace,fault)
        try:
            with archive.publication_guard():
                if not archive.database.is_file():
                    raise ArchiveRecoveryError('missing index requires explicit rebuild')
                archive._connect()
                recovered=archive._recover_locked()
                return archive._maintenance_result(recovered)
        finally:
            archive.close()

    @classmethod
    def upgrade(cls, root, workspace=None, *, quiesced=False, decision=None, fault=None):
        return cls._rebuild(root,workspace,quiesced=quiesced,decision=decision,fault=fault,upgrade=True)

    @classmethod
    def rebuild(cls, root, workspace=None, *, quiesced=False, decision=None, fault=None):
        return cls._rebuild(root,workspace,quiesced=quiesced,decision=decision,fault=fault,upgrade=False)

    @classmethod
    def _rebuild(cls, root, workspace, *, quiesced, decision, fault, upgrade):
        if not quiesced or not decision:
            raise ValueError('explicit quiescence and upgrade/rebuild decision required')
        archive=cls._maintenance(root,workspace,fault)
        try:
            with archive.publication_guard():
                previous=archive.head_path.read_bytes() if archive.head_path.exists() else None
                damaged=False
                if archive.database.is_file():
                    try:
                        archive._connect()
                        legacy=archive._legacy()
                        if not legacy:
                            meta=archive._meta()
                    except (sqlite3.DatabaseError,ArchiveRecoveryError) as exc:
                        if isinstance(exc,ArchiveRecoveryError) and not isinstance(exc.__cause__,sqlite3.DatabaseError):
                            raise
                        if upgrade:
                            raise ArchiveRecoveryError('upgrade requires a recognized legacy index') from None
                        damaged=True
                        legacy=False
                        archive.close()
                        raw=archive.database.read_bytes()
                        backup=_safe(archive.root,archive.root/('index.corrupt-'+digest(raw)+'.sqlite'))
                        _publish(backup,raw)
                    # Valid unknown schemas are refused by _meta, never downgraded.
                    if upgrade and not legacy:
                        raise ArchiveRecoveryError('upgrade requires recognized legacy schema')
                    if not legacy and not damaged:
                        pending=archive.db.execute('SELECT 1 FROM publication_intent').fetchone()
                        if pending:
                            archive._recover_locked()
                            previous=archive.head_path.read_bytes()
                elif upgrade:
                    raise ArchiveRecoveryError('missing legacy index; use explicit rebuild')
                # Save exact bytes; validate every file before replacing the old index.
                snapshots=[]
                for path in sorted(_safe(archive.root,archive.root/'receipts').glob('*.json')):
                    raw=path.read_bytes()
                    snapshots.append((archive._parse_receipt(raw,path),raw))
                staging=None
                original_database=archive.database
                if damaged:
                    staging=_safe(archive.root,archive.root/('.pending-index-'+str(uuid.uuid4())+'.sqlite'))
                    archive.database=staging
                    archive._connect(create=True)
                elif archive.db is None:
                    archive._connect(create=True)
                with archive.db:
                    archive.db.execute('BEGIN IMMEDIATE')
                    for name in ('publication_intent','index_meta','capture_lookup','policy_lookup','derivation_lookup','safety_lookup','receipts','locations','artifacts'):
                        archive.db.execute('DROP TABLE IF EXISTS '+name)
                    archive._schema()
                    generation=str(uuid.uuid4())
                    archive.db.execute('INSERT INTO index_meta VALUES (?,?,0)',(INDEX_VERSION,generation))
                    # Registrations precede dependent receipts; dependencies validate
                    # after all rows exist, preserving original enumeration/IDs.
                    for wrapper,raw in snapshots:
                        archive._index_row(wrapper,raw,None)
                    for wrapper,raw in snapshots:
                        archive._dependencies(wrapper)
                    archive._generation_intent(generation,previous)
                archive._bound_generation=generation
                if staging is not None:
                    archive.close()
                    # SQLite's FULL commit persists the staging database; fsync
                    # it explicitly before installing its directory entry.
                    fd=os.open(staging,os.O_RDONLY)
                    try:
                        os.fsync(fd)
                    finally:
                        os.close(fd)
                    os.replace(staging,original_database)
                    archive.database=original_database
                    archive._connect()
                archive.fault('after_generation_index')
                _fsync_directory(archive.root)
                archive._recover_locked()
                return archive._maintenance_result([])
        finally:
            archive.close()

    def _maintenance_result(self, identifiers):
        ready=self.require_ready()
        return dict(ready,recovered_receipt_ids=identifiers,safety_checkpoint=dict(generation=ready['generation'],sequence=ready['sequence']))

    def reindex(self, *, quiesced=False, decision=None, fault=None):
        self._write()
        result=type(self).rebuild(self.root,self.workspace,quiesced=quiesced,decision=decision,fault=fault)
        self.close()
        self._connect()
        return result

    def audit(self):
        """Explicit whole-file audit, also usable on a readonly legacy/missing index."""
        errors=[]
        wrappers=[]
        known=set()
        registrations=[]
        for path in sorted(_safe(self.root,self.root/'receipts').glob('*.json')):
            try:
                wrapper=self._read_receipt(path)
                wrappers.append(wrapper)
                p=self._registration(wrapper)
                if p:
                    registrations.append(p)
                    if p['scope']=='archive':
                        known.add(p['path'])
            except (OSError,ValueError,KeyError,TypeError) as exc:
                errors.append(dict(path=path.relative_to(self.root).as_posix(),error=str(exc)))
        for p in registrations:
            try:
                base=self.root if p['scope']=='archive' else self.workspace
                path=_safe(base,p['path'])
                if not path.is_file() or path.stat().st_size!=p['size'] or file_digest(path)!=p['sha256']:
                    errors.append(dict(p,error='missing or corrupt artifact'))
            except (OSError,ValueError) as exc:
                errors.append(dict(p,error=str(exc)))
        orphan_objects,staging=[],[]
        for path in self.root.rglob('*'):
            relative=path.relative_to(self.root).as_posix()
            if path.is_symlink():
                errors.append(dict(path=relative,error='symlink forbidden'))
            elif path.name.startswith('.pending-'):
                staging.append(relative)
            elif path.is_file() and 'objects' in path.relative_to(self.root).parts and relative not in known:
                orphan_objects.append(relative)
                try:
                    if path!=self.object_path(path.name) or file_digest(path)!=path.name:
                        raise ValueError('invalid orphan object path/hash')
                except (OSError,ValueError) as exc:
                    errors.append(dict(path=relative,error=str(exc)))
        return dict(ok=not errors,errors=errors,orphan_objects=sorted(orphan_objects),staging=sorted(staging),receipts=len(wrappers),readiness=self.readiness())

    verify=audit

"""Bounded capture with indexed evidence and durable guarded admission.

Production needs explicit account activation, synchronized safety authority and
acknowledged archive readiness. Saved-only use never allocates quota authority.
"""
from dataclasses import dataclass
import hashlib
import json
import time
import uuid
import tempfile
import http.client
import urllib.request
import urllib.error

from .campaign_state import LeaseBusy, StateError, StopCollection, host_for_url

_SENSITIVE = frozenset(('cookie','set-cookie','authorization','proxy-authorization','location'))


_DIAGNOSTICS = frozenset(('error','body_error','message','reason','note','diagnostic','diagnostics','warning'))


def sanitize(value, sensitive_headers=(), secrets=(), *, diagnostic=False):
    """Filter credential header keys recursively; redact only diagnostic text.

    Exact URLs, IDs, hashes, timestamps and source report fields are structural
    metadata. A short cookie fragment must never rewrite those identities.
    Original evidence bytes are handled separately and remain unchanged.
    """
    blocked = _SENSITIVE | frozenset(k.lower() for k in sensitive_headers)
    if isinstance(value, dict):
        return {k: sanitize(v,sensitive_headers,secrets,
                            diagnostic=diagnostic or str(k).lower() in _DIAGNOSTICS)
                for k,v in value.items() if str(k).lower() not in blocked}
    if isinstance(value, (tuple,list)):
        return [sanitize(v,sensitive_headers,secrets,diagnostic=diagnostic) for v in value]
    if diagnostic and isinstance(value,str):
        for secret in secrets:
            if secret:
                value = value.replace(str(secret),'[redacted]')
    return value


def _secrets(value, sensitive_headers=()):
    blocked = _SENSITIVE | frozenset(k.lower() for k in sensitive_headers)
    result = []
    if isinstance(value,dict):
        for key, item in value.items():
            if str(key).lower() in blocked and isinstance(item,str):
                result.append(item)
                lower = str(key).lower()
                if lower in ('authorization','proxy-authorization'):
                    parts = item.split(None,1)
                    if len(parts)==2:
                        result.append(parts[1])
                elif lower in ('cookie','set-cookie'):
                    pairs = item.split(';') if lower=='cookie' else item.split(';')[:1]
                    result.extend(pair.split('=',1)[1].strip() for pair in pairs if '=' in pair)
            else:
                result.extend(_secrets(item,sensitive_headers))
    elif isinstance(value,(tuple,list)):
        for item in value:
            result.extend(_secrets(item,sensitive_headers))
    return result


def _response_metadata(headers, sensitive_headers=()):
    """Native HTTPMessage.items() retains duplicate credential observations.

    Called only after the status transaction commits. Normalize permitted fields
    to plain JSON metadata; credentials, including every repeated Set-Cookie,
    remain in memory and never reach the new wrapper.
    """
    entries = list(headers.items())
    secrets = []
    blocked = _SENSITIVE | frozenset(key.lower() for key in sensitive_headers)
    metadata = {}
    for key,value in entries:
        secrets.extend(_secrets({key:value},sensitive_headers))
        if str(key).lower() not in blocked:
            metadata[key] = value
    return metadata,secrets


def fingerprint(method, url, body=b'', headers=None, *, context=None):
    # Credentials affect identity only through this digest, never persisted values.
    value = [method.upper(), url, hashlib.sha256(body).hexdigest(),
             sorted((k.lower(), v) for k, v in (headers or {}).items())]
    if context is not None:
        value.append(context)
    return hashlib.sha256(json.dumps(value, separators=(',', ':')).encode()).hexdigest()


@dataclass
class Policy:
    allowed_urls: frozenset
    budget: int = 0                 # zero selects offline; nonzero is NOT quota authority
    max_bytes: int = 12000000
    timeout: float = 30
    lease_seconds: float = 120
    prior_failures: tuple = ()
    policy_hash: str = 'capture-policy/1'
    source: str | None = None
    sensitive_headers: tuple = ()
    session_scope: str | None = None
    observation_scope: str | None = None
    reuse_policy: str = 'compatible'
    throttle_seconds: float = 0


@dataclass
class Response:
    status: int
    final_url: str
    headers: dict
    body: bytes


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def transport(method, url, body, headers, timeout, max_bytes, *, observe_response):
    """Exactly one request. Persist status before reading any response body byte."""
    opener = urllib.request.build_opener(_NoRedirect())
    request = urllib.request.Request(url, data=body or None, headers=headers, method=method)
    deadline = time.monotonic() + timeout
    try:
        response = opener.open(request, timeout=timeout)
    except urllib.error.HTTPError as exc:
        response = exc
    with response:
        status, final_url = response.code, response.url
        observe_response(status, final_url, response.headers)
        response_headers = dict(response.headers)
        size = 0
        with tempfile.TemporaryFile() as spool:
            while size <= max_bytes:
                if time.monotonic() > deadline:
                    raise TimeoutError('response exceeded time bound')
                chunk = response.read1(min(65536, max_bytes + 1 - size))
                if not chunk:
                    break
                spool.write(chunk)
                size += len(chunk)
            spool.seek(0)
            payload = spool.read()
        return Response(status, final_url, response_headers, payload)


class Collector:
    def __init__(self, archive, policy, request_transport=transport, clock=time.time, *,
                 state=None, account_id=None, run_id='capture', unit_revision='capture/1',
                 mock_transport=False, monotonic=time.monotonic, fault=None):
        if policy.timeout <= 0 or policy.max_bytes <= 0 or policy.lease_seconds <= policy.timeout * 2:
            raise ValueError('positive bounds and lease longer than twice timeout required')
        if policy.budget < 0 or policy.throttle_seconds < 0:
            raise ValueError('negative budget/delay')
        if policy.reuse_policy not in ('compatible','never','session'):
            raise ValueError('unknown reuse policy')
        if policy.reuse_policy == 'session' and not policy.session_scope:
            raise ValueError('session reuse needs explicit ephemeral scope')
        if policy.budget:
            if state is None or account_id is None:
                raise StateError('nonzero collectors require an explicitly allocated durable account')
            if mock_transport:
                if request_transport is transport:
                    raise StateError('explicit fixture mock cannot select production transport')
            else:
                if request_transport is not transport:
                    raise StateError('injected fixture transports require mock_transport=True')
                state.require_production_ready(archive,account_id)
        if state is not None and (state.root != archive.root or state.readonly):
            raise StateError('writable operational state must belong to this archive')
        self.archive, self.policy = archive, policy
        self.transport, self.clock, self.monotonic = request_transport, clock, monotonic
        self.mock_transport = mock_transport
        self.state, self.account_id = state, account_id
        self.run_id, self.unit_revision = run_id, unit_revision
        self.fault = fault or (lambda point: None)
        self.stopped = False
        self.owner = uuid.uuid4().hex
        self.lease_path = archive.root / 'capture-leases.sqlite'
        self.fences = {}
        self.secret_values = []
        if policy.budget:
            account = state.account(account_id)
            if account['policy_hash'] != policy.policy_hash:
                raise StateError('collector policy does not match immutable budget account')
        for row in policy.prior_failures:
            self._ingest_restrictions([row])
            sanitized = sanitize(row,self.policy.sensitive_headers,_secrets(row,self.policy.sensitive_headers))
            policy_id = 'policy-'+hashlib.sha256(json.dumps(sanitized,sort_keys=True,separators=(',',':')).encode()).hexdigest()
            self._receipt('capture_policy', {'saved_failure': row},policy_id)

    def _ingest_restrictions(self, rows):
        if self.state is None:
            return
        for row in rows:
            status = row.get('status')
            error = str(row.get('error') or '')
            if status not in (402,403,429,503):
                status = next((code for code in (402,403,429,503) if str(code) in error),None)
            if not status or not row.get('url'):
                continue
            host = host_for_url(row['url'])
            # New observed records already carry durable authority; don't resurrect
            # an explicitly resumed event by scanning its historical result again.
            if row.get('reservation_id'):
                try:
                    known_reservation = self.state.reservation(row['reservation_id'])
                except StateError:
                    known_reservation = None
                if known_reservation and known_reservation['url'] == row['url'] and known_reservation['status'] == status:
                    continue
            provenance = 'legacy-' + hashlib.sha256(json.dumps(sanitize(row,self.policy.sensitive_headers),sort_keys=True,separators=(',',':')).encode()).hexdigest()
            self.state.ingest_legacy_status(status=status,url=row['url'],host=host,source=row.get('source'),
                resource=row.get('resource'),broader_source=row.get('restriction_scope')=='source',
                provenance=provenance,policy_hash=self.policy.policy_hash)

    def _receipt(self, kind, payload, receipt_id=None):
        secrets = self.secret_values + _secrets(payload,self.policy.sensitive_headers)
        payload = sanitize(payload,self.policy.sensitive_headers,secrets)
        receipt_id = self.archive.receipt(kind,payload,receipt_id=receipt_id)
        return dict(payload,id=receipt_id)

    def _store_bytes(self, payload):
        return self.archive.store_bytes(payload)

    def _lease(self, keys):
        if self.state is None:
            raise StateError('explicit operational authority required')
        self.fences = self.state.acquire(keys,self.owner,self.policy.lease_seconds)

    def _release(self, keys):
        fences = {key:self.fences[key] for key in keys if key in self.fences}
        try:
            self.state.release(fences,self.owner)
        finally:
            for key in keys:
                self.fences.pop(key,None)

    def _blocked(self, url):
        rows = list(self.policy.prior_failures)
        rows.extend(reference['row'] for reference in self.archive.safety_references(url=url))
        return any(row.get('url')==url and self._denial(row) for row in rows)

    @staticmethod
    def _denial(row):
        return row.get('status') in (402,403) or any(str(code) in str(row.get('error') or '') for code in (402,403))

    @staticmethod
    def state_status(row):
        from .artifacts import Archive
        return Archive.safety_status(row)

    def _validate_cache(self, row, expected, validator):
        payload = self.archive.resolve(row['artifact_sha256']).read_bytes()
        if expected == 'pdf' and not payload.startswith(b'%PDF-'):
            raise ValueError('cached representation is not PDF bytes')
        if validator:
            validator(payload)

    def _cached(self, request_id):
        scoped = bool(self.policy.source or self.policy.session_scope or self.policy.observation_scope)
        wrappers = self.archive.capture_candidates(request_id,source=self.policy.source,
            session_scope=self.policy.session_scope,observation_scope=self.policy.observation_scope,
            live=bool(self.policy.budget),scoped=scoped)
        return [dict(wrapper['payload'],id=wrapper['id']) for wrapper in wrappers]

    def capture(self, url, *, method='GET', body=b'', headers=None, refresh=False,
                expected=None, validator=None, resource=None, validator_id=None):
        if self.policy.budget:
            if self.mock_transport and self.transport is transport:
                raise StateError('fixture mock cannot select production transport')
            if not self.mock_transport:
                if self.transport is not transport:
                    raise StateError('injected transport must use explicit fixture mode')
                self.state.require_production_ready(self.archive,self.account_id)
        headers = dict(headers or {})
        self.secret_values = _secrets(headers,self.policy.sensitive_headers)
        context = None
        if self.policy.source or self.policy.session_scope or self.policy.observation_scope:
            context = [self.policy.source,self.policy.session_scope,self.policy.observation_scope]
        request_id = fingerprint(method,url,body,headers,context=context)
        if url not in self.policy.allowed_urls:
            raise ValueError('URL outside explicit source policy')
        host = host_for_url(url)
        if self.stopped:
            raise StopCollection('collection checkpointed')
        # A throttle stops dispatch. Offline (budget zero) replay dispatches nothing,
        # so a saved 429/503 must not halt it; exact-URL denials still apply.
        if self.state:
            restrictions = self.state.restrictions(url,source=self.policy.source,resource=resource,host=host)
            if self.policy.budget and any(r['kind'] == 'throttle' for r in restrictions):
                raise StopCollection('collection checkpointed after throttling')
            blocked = any(r['kind'] == 'deny' for r in restrictions)
        else:
            if self.policy.budget and any(self.state_status(r['row']) in (429,503) for r in self.archive.safety_references(host=host)):
                raise StopCollection('collection checkpointed after throttling')
            blocked = self._blocked(url)
        if blocked:
            return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='blocked'))
        refresh = refresh or self.policy.reuse_policy == 'never'
        cached = self._cached(request_id)
        prior = next((r for r in reversed(cached) if r.get('outcome') in ('success','not_modified')),None)
        if prior:
            self._validate_cache(prior,expected,validator)
        if prior and not refresh:
            return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='reused',prior_capture_id=prior['id'],artifact_sha256=prior['artifact_sha256']))
        if not self.policy.budget:
            return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='offline_unavailable'))
        if not refresh and cached and cached[-1].get('outcome') not in ('success','not_modified'):
            return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='saved_unavailable',prior_capture_id=cached[-1]['id']))
        failures = list(self.policy.prior_failures) + [r['payload']['saved_failure'] for r in self.archive.policy_candidates(url)]
        for failure in failures:
            if failure.get('url') == url:
                return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='saved_unavailable',historical=failure))
        keys = ['request:'+request_id,'host:'+host]
        self._lease(keys)
        reservation_id = str(uuid.uuid4())
        reserved = False
        try:
            # Recheck cache after obtaining the host/request locks.
            cached = self._cached(request_id)
            if not refresh and cached:
                stored = cached[-1]
                if stored.get('outcome') in ('success','not_modified'):
                    self._validate_cache(stored,expected,validator)
                    return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='reused',prior_capture_id=stored['id'],artifact_sha256=stored['artifact_sha256']))
                return self._receipt('capture_reuse',dict(url=url,request_id=request_id,outcome='saved_unavailable',prior_capture_id=stored['id']))
            wire_headers = dict(headers)
            if prior and refresh:
                old = {k.lower():v for k,v in prior.get('headers',{}).items()}
                if old.get('etag'):
                    wire_headers['If-None-Match'] = old['etag']
                if old.get('last-modified'):
                    wire_headers['If-Modified-Since'] = old['last-modified']
            wire_request_id = fingerprint(method,url,body,wire_headers,context=context)
            reservation = self.state.reserve(account_id=self.account_id,run_id=self.run_id,unit_revision=self.unit_revision,
                request_id=request_id,url=url,host=host,owner=self.owner,fences=self.fences,max_body=self.policy.max_bytes+1,
                source=self.policy.source,resource=resource,reservation_id=reservation_id,archive=self.archive,
                transport_mode='mock' if self.mock_transport else 'production')
            reserved = True
            self.fault('before_attempt_publication')
            attempt = self._receipt('capture_attempt',dict(url=url,request_id=request_id,method=method.upper(),body_sha256=hashlib.sha256(body).hexdigest(),
                headers=wire_headers,wire_request_id=wire_request_id,conditional_prior_capture_id=prior['id'] if prior and refresh else None,
                validator_context=dict(expected=expected,validator_id=validator_id),started_at=reservation['start_wall'],collector_version='devicehunt.capture/2',acquisition_method='http',
                reservation_id=reservation_id,account_id=self.account_id,owner=self.owner,lease_fences=dict(self.fences),run_id=self.run_id,unit_revision=self.unit_revision,
                admission_sequence=reservation['sequence'],source=self.policy.source,resource=resource,
                session_scope=self.policy.session_scope,observation_scope=self.policy.observation_scope),reservation['attempt_id'])
            self.fault('after_attempt_publication')
            self.state.transition(reservation_id,self.owner,self.fences,'reserved','attempt_published',archive=self.archive)
            self.fault('before_dispatch_admission')
            self.state.transition(reservation_id,self.owner,self.fences,'attempt_published','dispatch_admitted',archive=self.archive)
            self.fault('after_dispatch_admission')
            row = dict(url=url,request_id=request_id,wire_request_id=wire_request_id,attempt_id=attempt['id'],reservation_id=reservation_id,account_id=self.account_id,
                admission_sequence=reservation['sequence'],request_context_known=True,source=self.policy.source,resource=resource,
                session_scope=self.policy.session_scope,observation_scope=self.policy.observation_scope)
            observed = []
            def observe(status, final_url, response_headers):
                if observed:
                    raise StateError('multiple response observations for one dispatch')
                self.state.observe(reservation_id,self.owner,self.fences,status,policy_hash=self.policy.policy_hash,
                                   throttle_seconds=self.policy.throttle_seconds)
                observed.append((status,final_url))
                response_metadata,response_secrets = _response_metadata(response_headers,self.policy.sensitive_headers)
                self.secret_values.extend(response_secrets)
                row.update(status=status,final_url=final_url,headers=sanitize(response_metadata,self.policy.sensitive_headers,self.secret_values))
            uncertain = False
            actual = stored_size = 0
            try:
                self.fault('before_transport')
                started = self.monotonic()
                response = self.transport(method,url,body,wire_headers,self.policy.timeout,self.policy.max_bytes,observe_response=observe)
                self.fault('after_transport')
                if not observed or observed[0] != (response.status,response.final_url):
                    raise StateError('transport must observe response status before body read')
                if self.monotonic()-started > self.policy.timeout:
                    raise TimeoutError('transport exceeded time bound')
                actual = len(response.body)
                payload = response.body[:self.policy.max_bytes]
                oversized = actual > self.policy.max_bytes
                status = response.status
                if status in (402,403):
                    row['outcome'] = 'blocked'
                elif status in (429,503):
                    row['outcome'] = 'throttled'
                elif oversized:
                    row.update(outcome='invalid',error='response exceeds byte bound')
                elif response.final_url != url:
                    row.update(outcome='invalid',error='unexpected final URL')
                elif status == 304:
                    if prior:
                        row.update(outcome='not_modified',prior_capture_id=prior['id'],artifact_sha256=prior['artifact_sha256'])
                    else:
                        row.update(outcome='invalid',error='304 without verified prior representation')
                elif status != 200:
                    row['outcome'] = 'unavailable'
                elif not payload:
                    row['outcome'] = 'empty'
                elif expected == 'pdf' and not payload.startswith(b'%PDF-'):
                    row.update(outcome='invalid',error='not PDF bytes')
                else:
                    try:
                        if validator:
                            validator(payload)
                        row['outcome'] = 'success'
                    except (ValueError,SyntaxError) as exc:
                        row.update(outcome='invalid',error=type(exc).__name__)
                if oversized:
                    row.update(body_truncated=True,body_error='response exceeds byte bound')
                if payload:
                    row['response_sha256'] = self._store_bytes(payload)
                    stored_size = len(payload)
                    if row['outcome'] == 'success':
                        row['artifact_sha256'] = row['response_sha256']
            except (TimeoutError,urllib.error.URLError,OSError,http.client.HTTPException) as exc:
                uncertain = True
                self.state.uncertain(reservation_id,self.owner,self.fences,'response/transport failure')
                outcome = 'blocked' if row.get('status') in (402,403) else 'throttled' if row.get('status') in (429,503) else 'timeout' if isinstance(exc,TimeoutError) else 'unavailable'
                # Exception repr can contain credentials; retain type, not raw text.
                row.update(outcome=outcome,error=type(exc).__name__,body_error=type(exc).__name__)
            row.update(ended_at=self.clock(),actual_bytes=actual,stored_bytes=stored_size)
            self.fault('before_result_publication')
            result = self._receipt('capture_result',row)
            self.fault('after_result_publication')
            if not uncertain:
                self.state.transition(reservation_id,self.owner,self.fences,'response_observed','complete',archive=self.archive,actual_bytes=actual,stored_bytes=stored_size,result_id=result['id'])
            return result
        except BaseException:
            # Includes committed reservation faults whose reserve() did not return.
            try:
                if reserved or self.state.reservation(reservation_id):
                    self.state.uncertain(reservation_id,self.owner,self.fences)
            except StateError:
                pass  # absent reservation or damaged state already fails closed
            raise
        finally:
            self._release(keys)

    def register_saved(self, row, path=None, locator=None, *, copy=True, method='GET', body=b'',
                       headers=None, source=None, resource=None, session_scope=None, observation_scope=None,
                       slot=None, unit_revision=None):
        """Historical times stay unknown; new wrappers redact legacy metadata.

        copy=False registers the verified existing workspace file in place. The
        saved receipt ID hashes the artifact digest, never its location, so both
        modes yield the same ID for the same saved row.

        Optional request context (method/body/scope/slot) is recorded only when
        supplied, so earlier saved IDs stay stable. A declared context keeps, e.g.,
        an RRA before-export and after-export of the same POST as distinct saved
        observations. Saved context still never satisfies live cache reuse.
        """
        secrets = self.secret_values + _secrets(row,self.policy.sensitive_headers)
        historical = sanitize(row,self.policy.sensitive_headers,secrets)
        if slot is not None and not observation_scope:
            raise ValueError('a saved observation slot needs an explicit observation scope')
        context = [source,session_scope,observation_scope] if (source or session_scope or observation_scope) else None
        declared = context is not None or method.upper() != 'GET' or body or headers
        # Same identity as a live capture() of this request (headers included, values never stored).
        request_id = fingerprint(method,row['url'],body,headers,context=context) if declared else fingerprint('GET',row['url'])
        result = dict(url=row['url'],request_id=request_id,status=row.get('status'),
                      historical=historical,acquisition_method='saved_report',request_context_known=False)
        extra = dict(method=method.upper() if declared else None,
                     body_sha256=hashlib.sha256(body).hexdigest() if body else None,
                     source=source,resource=resource,session_scope=session_scope,
                     observation_scope=observation_scope,slot=slot,unit_revision=unit_revision)
        result.update({k: v for k, v in extra.items() if v is not None})
        if row.get('status') == 200:
            if path is None:
                raise ValueError('saved success needs bytes')
            from .artifacts import _safe
            path = _safe(self.archive.workspace,path)
            if copy:
                payload = path.read_bytes()
                if hashlib.sha256(payload).hexdigest() != row['sha256']:
                    raise ValueError('saved capture checksum mismatch')
                artifact = self._store_bytes(payload)
            else:
                try:
                    artifact = self.archive.register_file(path,expected_sha256=row['sha256'])
                except ValueError as exc:
                    if 'hash mismatch' in str(exc):
                        raise ValueError('saved capture checksum mismatch') from None
                    raise
            result.update(outcome='success',artifact_sha256=artifact)
        else:
            status = row.get('status')
            error = str(row.get('error') or '')
            outcome = ('blocked' if status in (402,403) or any(str(code) in error for code in (402,403)) else
                       'throttled' if status in (429,503) or any(str(code) in error for code in (429,503)) else 'unavailable')
            result.update(outcome=outcome,error=sanitize(row.get('error','unknown historical failure'),self.policy.sensitive_headers,secrets,diagnostic=True))
        result['historical_locator'] = sanitize(locator,self.policy.sensitive_headers,secrets)
        receipt_id = 'saved-' + hashlib.sha256(json.dumps(result,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        self._ingest_restrictions([row])
        return self._receipt('capture_result',result,receipt_id)

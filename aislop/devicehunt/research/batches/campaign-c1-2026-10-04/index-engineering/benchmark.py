"""Bounded offline measurements; real archive is strictly read-only legacy data."""
import json
from pathlib import Path
import socket
import sqlite3
import sys
import tempfile
import time
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[4]))
from devicehunt.artifacts import Archive, canonical, digest
from devicehunt.derivations import Derivations


def measure(root, operation):
    reads=[]
    scans=[]
    hashes=[]
    import devicehunt.artifacts as module
    read=Path.read_bytes
    file_digest=module.file_digest
    def tracked_read(path):
        reads.append(path.relative_to(root).as_posix() if path.is_relative_to(root) else str(path))
        return read(path)
    def forbidden_scan(path,*args,**kwargs):
        scans.append(str(path))
        raise AssertionError('ordinary query enumerated a directory')
    def tracked_hash(path):
        hashes.append(str(path))
        return file_digest(path)
    start=time.perf_counter()
    with patch.object(Path,'read_bytes',tracked_read),patch.object(Path,'glob',forbidden_scan),patch.object(Path,'rglob',forbidden_scan),patch.object(Path,'iterdir',forbidden_scan),patch.object(module,'file_digest',tracked_hash):
        result=operation()
    return dict(elapsed_seconds=time.perf_counter()-start,wrapper_reads=sum('/receipts/' in '/'+p for p in reads),
                head_reads=sum(p=='index-head.json' for p in reads),directory_scans=len(scans),artifact_hashes=len(hashes),result=result)


def main():
    for name in ('socket.socket.connect','socket.create_connection','urllib.request.OpenerDirector.open','subprocess.run','subprocess.Popen'):
        patch(name,side_effect=AssertionError('network/external entrypoint forbidden')).start()
    workspace=Path.cwd()
    real=workspace/'data/archive'
    protected={name:digest((real/name).read_bytes()) for name in ('index.sqlite','capture-leases.sqlite') if (real/name).is_file()}
    def real_query():
        archive=Archive(real,workspace,readonly=True)
        try:
            row=archive.db.execute('SELECT id FROM receipts ORDER BY id LIMIT 1').fetchone()
            wrapper=archive.get_receipt(row[0])
            return dict(readiness=archive.readiness()['status'],selected_receipt=wrapper['id'],receipt_count=archive.db.execute('SELECT COUNT(*) FROM receipts').fetchone()[0],artifact_count=archive.db.execute('SELECT COUNT(*) FROM artifacts').fetchone()[0])
        finally:
            archive.close()
    real_result=measure(real,real_query)
    after={name:digest((real/name).read_bytes()) for name in protected}
    assert after==protected
    with tempfile.TemporaryDirectory(prefix='fcc-c1b-index-') as directory:
        root=Path(directory)/'archive'
        (root/'receipts').mkdir(parents=True)
        (root/'objects').mkdir()
        # Explicit bootstrap fixture: 5,000 immutable small legacy wrappers, then
        # one full explicit rebuild. No bypass is used by ordinary modern writers.
        for number in range(5000):
            wrapper=dict(id='synthetic-'+str(number),kind='test',created_at='unknown',payload={'ordinal':number})
            (root/'receipts'/(wrapper['id']+'.json')).write_bytes(canonical(wrapper))
        Archive.rebuild(root,quiesced=True,decision='bounded synthetic bootstrap')
        archive=Archive(root)
        selected=archive.receipt('capture_result',dict(request_id='selected',url='https://fixture.test/a',outcome='unavailable'),'selected')
        sha=archive.store_bytes(canonical({'pages':[{'text':'fixture'}]}))
        engine=Derivations(archive)
        recipe=engine.recipe('page-text-flatten','1')
        run=engine.run(recipe,[{'role':'pages','sha256':sha}])
        inputs=run['payload']['inputs']
        plans={
            'capture':[dict(r) for r in archive.db.execute('EXPLAIN QUERY PLAN SELECT receipt_id FROM capture_lookup WHERE request_id=?',('selected',))],
            'derivation':[dict(r) for r in archive.db.execute("EXPLAIN QUERY PLAN SELECT receipt_id FROM derivation_lookup WHERE recipe_id=? AND input_fingerprint=? AND status='success'",(recipe,digest(canonical(inputs))))],
            'safety':[dict(r) for r in archive.db.execute('EXPLAIN QUERY PLAN SELECT reference_id FROM safety_lookup WHERE sequence>?',(0,))]}
        count=archive.db.execute('SELECT COUNT(*) FROM receipts').fetchone()[0]
        archive.close()
        def normal_open():
            opened=Archive(root,readonly=True)
            try:
                return opened.require_ready()['status']
            finally:
                opened.close()
        def capture_lookup():
            opened=Archive(root,readonly=True)
            try:
                return opened.capture_candidates('selected')[0]['id']
            finally:
                opened.close()
        def derivation_lookup():
            opened=Archive(root,readonly=True)
            try:
                found=opened.derivation_candidates(recipe,inputs)[0]
                return opened.verified_receipt(found['id'])['wrapper']['id']
            finally:
                opened.close()
        synthetic=dict(receipt_count=count,logical_bytes=sum(p.stat().st_size for p in root.rglob('*') if p.is_file()),
                       normal_open=measure(root,normal_open),capture_lookup=measure(root,capture_lookup),
                       derivation_lookup=measure(root,derivation_lookup),query_plans=plans)
        assert synthetic['logical_bytes']<64*1024*1024
    result=dict(real_readonly_legacy=real_result,real_protected_hashes=protected,real_hashes_unchanged=protected==after,synthetic_modern=synthetic,
                limits='5000 bootstrap wrappers plus five public modern receipts; process-crash tests do not simulate power loss; filesystem fsync durability uses supported local POSIX/SQLite semantics')
    output=Path(__file__).with_name('benchmark.json')
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()

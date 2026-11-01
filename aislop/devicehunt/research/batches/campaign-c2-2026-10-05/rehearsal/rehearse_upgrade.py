"""Offline rehearsal of the explicit legacy archive/operational upgrade.

Copies data/archive into a fresh scratch root, never writes the live archive,
and writes only --output. The real workspace supplies read-only artifact paths.
The rehearsal authority is bound to the scratch path and must never be copied
back; the live upgrade runs separately against data/archive itself.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt.artifacts import Archive, ArchiveRecoveryError  # noqa: E402
from devicehunt.campaign_state import CampaignState, StateError  # noqa: E402

LIVE = ROOT / 'data/archive'
DECISION = 'c2-2026-10-05 rehearsal: quiesced copy, no collectors, no network'


def tree(root):
    out = {}
    for path in sorted(Path(root).rglob('*')):
        if path.is_file():
            out[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return out


def immutable(hashes):
    return {k: v for k, v in hashes.items() if k.startswith(('receipts/', 'objects/'))}


def leases(path):
    with sqlite3.connect(Path(path).as_uri() + '?mode=ro', uri=True) as db:
        return db.execute('SELECT key,owner,expires FROM leases ORDER BY key').fetchall()


def negative_wrong_workspace(scratch):
    """Default workspace (root.parent) cannot resolve workspace-scope artifacts."""
    root = scratch / 'negative' / 'archive'
    shutil.copytree(LIVE, root)
    before = tree(root)
    try:
        Archive.upgrade(root, quiesced=True, decision=DECISION + ' (negative)')
        outcome = 'unexpected success'
    except (ValueError, ArchiveRecoveryError) as exc:
        outcome = type(exc).__name__ + ': ' + str(exc)[:160]
    after = tree(root)
    created = sorted(set(after) - set(before))
    changed = sorted(k for k in before if after.get(k) != before[k])
    return dict(outcome=outcome, created_files=created, changed_files=changed,
                failed_cleanly=outcome != 'unexpected success' and not changed
                and all(name.startswith('capture-locks/') for name in created))


def rehearse(scratch):
    root = scratch / 'positive' / 'archive'
    shutil.copytree(LIVE, root)
    live_before = tree(LIVE)
    before = tree(root)
    legacy_leases = leases(root / 'capture-leases.sqlite')
    with sqlite3.connect((root / 'index.sqlite').as_uri() + '?mode=ro', uri=True) as db:
        legacy_receipts = db.execute('SELECT id,kind,created_at,payload FROM receipts ORDER BY id').fetchall()
    upgraded = Archive.upgrade(root, ROOT, quiesced=True, decision=DECISION)
    state = CampaignState.migrate(root, quiesced=True)
    archive = Archive(root, ROOT)
    try:
        synced = state.synchronize_archive(archive, decision=DECISION)
        readiness = archive.readiness()
        with sqlite3.connect((root / 'index.sqlite').as_uri() + '?mode=ro', uri=True) as db:
            indexed = db.execute('SELECT id,kind,created_at,payload FROM receipts ORDER BY id').fetchall()
            safety_refs = db.execute('SELECT count(*) FROM safety_lookup').fetchone()[0]
    finally:
        archive.close()
    audit = Archive(root, ROOT, readonly=True).audit()
    after = tree(root)
    with state.db() as db:
        operational = dict(
            meta=dict(db.execute('SELECT version,recovery,archive_generation,safety_sequence FROM operational_meta').fetchone()),
            accounts=db.execute('SELECT count(*) FROM accounts').fetchone()[0],
            reservations=db.execute('SELECT count(*) FROM reservations').fetchone()[0],
            events=[dict(r) for r in db.execute('SELECT kind,scope,provenance,reason FROM events ORDER BY sequence')],
            activations=db.execute('SELECT count(*) FROM activations').fetchone()[0])
    # A second explicit migration of a completed authority must be refused.
    try:
        CampaignState.migrate(root, quiesced=True)
        remigration = 'unexpected success'
    except StateError as exc:
        remigration = 'refused: ' + str(exc)[:120]
    return dict(
        upgrade=dict((k, upgraded[k]) for k in ('status', 'generation', 'sequence')),
        synchronized_reference_ids=len(synced['imported_reference_ids']),
        readiness=readiness['status'],
        audit=dict(ok=audit['ok'], errors=len(audit['errors']), receipts=audit['receipts'],
                   orphan_objects=len(audit['orphan_objects'])),
        receipts_and_objects_unchanged=immutable(before) == immutable(after),
        receipt_files=sum(1 for k in before if k.startswith('receipts/')),
        object_files=sum(1 for k in before if k.startswith('objects/')),
        legacy_receipt_rows_preserved=legacy_receipts == indexed,
        indexed_receipts=len(indexed),
        safety_lookup_rows=safety_refs,
        legacy_leases=len(legacy_leases),
        legacy_backup_matches=leases(root / 'capture-leases.before-c1a.sqlite') == legacy_leases,
        migrated_leases_preserved=leases(root / 'capture-leases.sqlite') == legacy_leases,
        new_files=sorted(set(after) - set(before) - {k for k in after if k.startswith('capture-locks/')}),
        operational=operational,
        remigration=remigration,
        live_archive_unchanged=tree(LIVE) == live_before,
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('fresh output path required; prior proof is retained')
    live_before = tree(LIVE)
    with tempfile.TemporaryDirectory(prefix='archive-rehearsal-') as tmp:
        scratch = Path(tmp)
        result = dict(checked_at=datetime.now(timezone.utc).isoformat(), host=os.uname().sysname,
                      python=sys.version.split()[0], live_root=str(LIVE.relative_to(ROOT)),
                      workspace='project root (read-only)', negative_default_workspace=negative_wrong_workspace(scratch))
        result['positive'] = rehearse(scratch)
    result['live_archive_unchanged'] = tree(LIVE) == live_before
    p = result['positive']
    result['ok'] = (result['negative_default_workspace']['failed_cleanly'] and result['live_archive_unchanged']
                    and p['readiness'] == 'ready' and p['audit']['ok'] and p['receipts_and_objects_unchanged']
                    and p['legacy_receipt_rows_preserved'] and p['legacy_backup_matches']
                    and p['migrated_leases_preserved'] and p['remigration'].startswith('refused'))
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

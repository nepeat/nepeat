"""One-shot exclusive upgrade of the live legacy acquisition registry.

Historical once it has run; do not rerun. Requires the passing rehearsal proof,
takes exact metadata backups first, makes no network request and allocates or
activates no account. Writes data/archive metadata, the backup directory and
--output only.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(HERE.parent / 'rehearsal'))
from devicehunt.artifacts import Archive  # noqa: E402
from devicehunt.campaign_state import CampaignState  # noqa: E402
from rehearse_upgrade import immutable, leases, tree  # noqa: E402

LIVE = ROOT / 'data/archive'
BACKUP = ROOT / 'data/archive-registry-backup-2026-10-05'
REHEARSAL = HERE.parent / 'rehearsal/rehearsal-1.json'
DECISION = ('c2-2026-10-05 live registry upgrade by /root-claude: user confirmed the Linux '
            'workspace is long-term; rehearsal passed; no collectors running; no network')


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or BACKUP.exists():
        raise SystemExit('one-shot upgrade already attempted; inspect the retained proof/backup')
    if not json.loads(REHEARSAL.read_text())['ok']:
        raise SystemExit('passing rehearsal required')
    readiness = Archive(LIVE, ROOT, readonly=True).readiness()
    if readiness['status'] != 'legacy_readonly':
        raise SystemExit('live archive is not the expected legacy registry: ' + readiness['status'])
    before = tree(LIVE)
    legacy_leases = leases(LIVE / 'capture-leases.sqlite')
    with sqlite3.connect((LIVE / 'index.sqlite').as_uri() + '?mode=ro', uri=True) as db:
        legacy_receipts = db.execute('SELECT id,kind,created_at,payload FROM receipts ORDER BY id').fetchall()
    BACKUP.mkdir()
    backups = {}
    for name in ('index.sqlite', 'capture-leases.sqlite'):
        shutil.copy2(LIVE / name, BACKUP / name)
        if sha(BACKUP / name) != before[name]:
            raise SystemExit('backup verification failed: ' + name)
        backups[name] = before[name]
    (BACKUP / 'SHA256SUMS').write_text(''.join(f'{v}  {k}\n' for k, v in sorted(backups.items())))
    (BACKUP / 'README.md').write_text(
        '# legacy acquisition registry backup\n\nExact bytes of `data/archive/index.sqlite` and '
        '`capture-leases.sqlite` before the 2026-10-05 explicit upgrade '
        '([proof](../../research/batches/campaign-c2-2026-10-05/acceptance/live-upgrade.json)). '
        'The legacy index is reconstructible from receipts; restore only with an explicit coherent '
        'restore decision, never beside the upgraded authority.\n')

    upgraded = Archive.upgrade(LIVE, ROOT, quiesced=True, decision=DECISION)
    state = CampaignState.migrate(LIVE, quiesced=True)
    archive = Archive(LIVE, ROOT)
    try:
        synced = state.synchronize_archive(archive, decision=DECISION)
        ready = archive.readiness()['status']
    finally:
        archive.close()
    with sqlite3.connect((LIVE / 'index.sqlite').as_uri() + '?mode=ro', uri=True) as db:
        indexed = db.execute('SELECT id,kind,created_at,payload FROM receipts ORDER BY id').fetchall()
    audit = Archive(LIVE, ROOT, readonly=True).audit()
    after = tree(LIVE)
    with state.db() as db:
        meta = dict(db.execute('SELECT version,recovery,archive_generation,safety_sequence FROM operational_meta').fetchone())
        accounts = db.execute('SELECT count(*) FROM accounts').fetchone()[0]
        restrictions = db.execute("SELECT count(*) FROM events WHERE kind IN ('deny','throttle')").fetchone()[0]
    result = dict(
        checked_at=datetime.now(timezone.utc).isoformat(), decision=DECISION,
        backup=dict(path=str(BACKUP.relative_to(ROOT)), sha256=backups),
        upgrade=dict((k, upgraded[k]) for k in ('status', 'generation', 'sequence')),
        readiness=ready, operational_meta=meta, accounts=accounts, activations=0,
        restriction_events=restrictions, synchronized_reference_ids=len(synced['imported_reference_ids']),
        audit=dict(ok=audit['ok'], errors=len(audit['errors']), receipts=audit['receipts'],
                   orphan_objects=len(audit['orphan_objects'])),
        receipts_and_objects_unchanged=immutable(before) == immutable(after),
        legacy_receipt_rows_preserved=legacy_receipts == indexed,
        legacy_leases=len(legacy_leases),
        leases_preserved=leases(LIVE / 'capture-leases.sqlite') == legacy_leases,
        new_files=sorted(k for k in set(after) - set(before) if not k.startswith('capture-locks/')),
        live_archive_files_after={k: v for k, v in after.items() if not k.startswith('capture-locks/')},
        network_requests=0)
    result['ok'] = (ready == 'ready' and audit['ok'] and result['receipts_and_objects_unchanged']
                    and result['legacy_receipt_rows_preserved'] and result['leases_preserved'])
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'live_archive_files_after'}, indent=2, sort_keys=True))
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

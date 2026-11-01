"""Offline c2 rehearsal on a scratch copy of the live archive with the real pinned pilot.

Legacy -> schema 3 migration, a hard crash after a published validation result, explicit
resume, then a second run that must only reuse. The live archive is never written;
the real workspace supplies read-only pinned inputs. Writes only --output.
"""
import argparse
from datetime import datetime, timezone
import json
import multiprocessing
import os
from pathlib import Path
import shutil
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from devicehunt.artifacts import Archive  # noqa: E402
from devicehunt.campaign import Runner, load_definition  # noqa: E402
from devicehunt.campaign_state import CampaignState  # noqa: E402
from rehearse_upgrade import immutable, tree  # noqa: E402

LIVE = ROOT / 'data/archive'
BACKUP = ROOT / 'data/archive-registry-backup-2026-10-05'
CRASH_UNIT = 'rra-official-query-41e4ca912b33'


def runner(root, **kwargs):
    archive = Archive(root, ROOT)
    return Runner(archive, CampaignState(root), load_definition(ROOT), **kwargs)


def crash(root):
    def fault(point):
        if point == 'after_result:' + CRASH_UNIT + ':validate':
            os._exit(17)
    runner(Path(root), fault=fault).run('rehearsal-run-1')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('fresh output path required')
    live_before = tree(LIVE)
    with tempfile.TemporaryDirectory(prefix='campaign-rehearsal-') as tmp:
        root = Path(tmp) / 'archive'
        # A copied authority is path-bound and refused by design. Rebuild a scratch
        # authority from the verified legacy metadata backup plus live receipts/objects.
        shutil.copytree(LIVE, root, ignore=shutil.ignore_patterns(
            'capture-locks', 'capture-authority*.json', 'index-head.json', '*.sqlite'))
        for name in ('index.sqlite', 'capture-leases.sqlite'):
            shutil.copy2(BACKUP / name, root / name)
        before = tree(root)
        decision = 'c2 rehearsal on scratch copy'
        Archive.upgrade(root, ROOT, quiesced=True, decision=decision)
        state = CampaignState.migrate(root, quiesced=True)
        scratch = Archive(root, ROOT)
        try:
            state.synchronize_archive(scratch, decision=decision)
        finally:
            scratch.close()
        process = multiprocessing.get_context('spawn').Process(target=crash, args=(str(root),))
        process.start()
        process.join(600)
        r = runner(root)
        try:
            crashed_state = r.state.campaign_run('rehearsal-run-1')['state']
            resumed = r.resume('rehearsal-run-1', decision='c2 rehearsal: resume after injected hard crash')
            actions = [e['action'] for e in r.state.campaign_executions(run_id='rehearsal-run-1')]
            sequence = r.archive.readiness()['sequence']
            second = r.run('rehearsal-run-2')
            after_sequence = r.archive.readiness()['sequence']
            audit = Archive(root, ROOT, readonly=True).audit()
        finally:
            r.archive.close()
        after = tree(root)
    result = dict(
        checked_at=datetime.now(timezone.utc).isoformat(), network_requests=0,
        crash=dict(unit=CRASH_UNIT, point='after validate result publication', exitcode=process.exitcode,
                   run_state_after_crash=crashed_state),
        resumed_run=dict(state=resumed['run_state'], actions={a: actions.count(a) for a in set(actions)},
                         units_accounted_for=resumed['units_accounted_for'], new_bytes=resumed['new_bytes'],
                         units=[{k: u.get(k) for k in ('unit_id', 'outcome', 'occurrences', 'pinned_comparison', 'mismatches')}
                                for u in resumed['units']]),
        second_run=dict(state=second['run_state'], actions=second['job_actions'], new_bytes=second['new_bytes'],
                        changes=sorted({u['change_from_previous_run'] for u in second['units']}),
                        new_archive_sequences=after_sequence - sequence),
        audit=dict(ok=audit['ok'], errors=len(audit['errors']), receipts=audit['receipts'],
                   orphan_objects=len(audit['orphan_objects'])),
        prior_receipts_and_objects_unchanged=all(after.get(k) == v for k, v in immutable(before).items()),
        live_archive_unchanged=tree(LIVE) == live_before)
    result['ok'] = (result['crash']['exitcode'] == 17 and crashed_state == 'running'
                    and resumed['run_state'] == 'complete' and 'recovered' in actions
                    and all(u['pinned_comparison'] == 'matches_pinned' for u in resumed['units'])
                    and second['run_state'] == 'complete' and set(second['job_actions']) == {'reused'}
                    and second['new_bytes'] == 0 and after_sequence == sequence and audit['ok']
                    and result['prior_receipts_and_objects_unchanged'] and result['live_archive_unchanged'])
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

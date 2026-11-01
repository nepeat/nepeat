"""One-shot live c2 acceptance: schema 2->3 upgrade, then two offline pilot runs.

Historical once run; do not rerun. Needs the passing real-data rehearsal and the
independent fix verification. No network path exists; nothing writes the catalog,
manifest, exports or saved sources. Writes data/archive metadata/receipts/objects
and --output only.
"""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(HERE.parent / 'rehearsal'))
from devicehunt.artifacts import Archive  # noqa: E402
from devicehunt.campaign import open_runner  # noqa: E402
from devicehunt.campaign_state import CampaignState  # noqa: E402
from rehearse_upgrade import immutable, tree  # noqa: E402

LIVE = ROOT / 'data/archive'
REHEARSAL = HERE.parent / 'rehearsal/campaign-rehearsal.json'
REVIEW = HERE.parent / 'review/c2-runner-review.md'


def summary(report):
    return dict(run_id=report['run_id'], state=report['run_state'], actions=report['job_actions'],
                units_accounted_for=report['units_accounted_for'], new_bytes=report['new_bytes'],
                physical_growth=report['physical_growth_since_run_start'],
                units=[{k: u.get(k) for k in ('unit_id', 'outcome', 'occurrences', 'query_records',
                                              'pinned_comparison', 'change_from_previous_run', 'candidate_sha256')}
                       for u in report['units']])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('one-shot live campaign already attempted; inspect the retained proof')
    assert json.loads(REHEARSAL.read_text())['ok']
    verdict = [line for line in REVIEW.read_text().splitlines() if line.startswith('VERDICT:')]
    assert verdict and verdict[-1].split(':', 1)[1].strip() in ('ACCEPT', 'ACCEPT with notes'), verdict
    before = tree(LIVE)
    upgraded = CampaignState.upgrade_campaign_jobs(
        LIVE, quiesced=True, decision='c2-2026-10-05 live schema 3 upgrade by /root-claude after rehearsal and review')
    del upgraded
    runner = open_runner(ROOT, LIVE)
    try:
        first = runner.run('c2-live-run-1')
        sequence = runner.archive.readiness()['sequence']
        second = runner.run('c2-live-run-2')
        after_sequence = runner.archive.readiness()['sequence']
    finally:
        runner.archive.close()
    audit = Archive(LIVE, ROOT, readonly=True).audit()
    after = tree(LIVE)
    result = dict(checked_at=datetime.now(timezone.utc).isoformat(), network_requests=0,
                  operational_backup='data/archive/capture-leases.before-c2.sqlite',
                  first_run=summary(first), second_run=summary(second),
                  second_run_new_archive_sequences=after_sequence - sequence,
                  audit=dict(ok=audit['ok'], errors=len(audit['errors']), receipts=audit['receipts'],
                             orphan_objects=len(audit['orphan_objects'])),
                  prior_receipts_and_objects_unchanged=all(after.get(k) == v for k, v in immutable(before).items()))
    result['ok'] = (first['run_state'] == second['run_state'] == 'complete'
                    and all(u['pinned_comparison'] == 'matches_pinned' for u in first['units'])
                    and set(second['job_actions']) == {'reused'} and second['new_bytes'] == 0
                    and after_sequence == sequence and audit['ok'] and result['prior_receipts_and_objects_unchanged'])
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps({k: result[k] for k in ('ok', 'second_run_new_archive_sequences', 'audit')}, indent=2))
    print(json.dumps(first['units_accounted_for']), first['new_bytes'], second['job_actions'])
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

"""Promote the independently accepted local batch, preserving the old live pair."""
import json
import os
import sqlite3
from contextlib import closing
from pathlib import Path
from stage import ROOT, OUT, DATA, CAND_DB, CAND_MANIFEST, sha, save, clone, rebuild


def main():
    acceptance = json.loads((OUT / 'promotion-acceptance.json').read_text())
    assert acceptance['accepted'] is True
    required = {
        'research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json',
        'data/exports/breadth-2026-10-14-comparison.json',
        'data/exports/breadth-2026-10-14-validation.json',
        'data/rebuilt-breadth-2026-10-14.sqlite',
        'scripts/compare_rebuild.py',
        'research/batches/discovery-2026-10-12/integration-wave5/code-under-test.json',
        *{str((OUT / name).relative_to(ROOT)) for name in (
            'accepted-inputs.json', 'stage-report.json', 'stage.py', 'promote.py',
            'refresh_exports.py', 'query-check.json')},
        *{str((OUT.parent / 'integration-review' / name).relative_to(ROOT)) for name in (
            'fix-review.md', 'plan-review.md', 'promotion-helper-review.md', 'final-review.md')},
    }
    assert required <= set(acceptance['proof_hashes']), required - set(acceptance['proof_hashes'])
    for name, digest in acceptance['proof_hashes'].items():
        relative = Path(name)
        assert not relative.is_absolute() and '..' not in relative.parts, name
        path = (ROOT / relative).resolve()
        assert path.is_relative_to(ROOT), name
        assert sha(path) == digest, name
    assert acceptance['review_path'] == str((OUT.parent / 'integration-review/final-review.md').relative_to(ROOT))
    review = ROOT / acceptance['review_path']
    assert sha(review) == acceptance['review_sha256']
    report = json.loads((OUT / 'stage-report.json').read_text())
    proof = ROOT / 'research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json'
    suite = json.loads(proof.read_text())
    assert suite['passed'] and all(suite[k] == 0 for k in ('failures', 'errors', 'skipped', 'blocked_network_or_external_process_calls'))
    code = json.loads((ROOT / 'research/batches/discovery-2026-10-12/integration-wave5/code-under-test.json').read_text())
    assert all(sha(ROOT / path) == digest for path, digest in code['files'].items())
    comparison = json.loads((DATA / 'exports/breadth-2026-10-14-comparison.json').read_text())
    with closing(sqlite3.connect(CAND_DB.as_uri() + '?mode=ro', uri=True)) as candidate:
        expected_tables = {r[0] for r in candidate.execute("SELECT name FROM sqlite_master WHERE type='table'")
                           if not r[0].startswith(('sqlite_', 'search_index_', 'document_search_'))}
    assert len(comparison) == len(expected_tables) == 52
    assert set(comparison) == expected_tables
    assert {'search_index', 'document_search'} <= set(comparison)
    assert all(r['original'] == r['rebuilt'] and r['only_original'] == r['only_rebuilt'] == 0 for r in comparison.values())
    for name, count in report['counts'].items():
        assert comparison[name]['original'] == count
    validation = json.loads((DATA / 'exports/breadth-2026-10-14-validation.json').read_text())
    assert validation['matches_reference_baseline'] and validation['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
    assert Path(validation['reference_database']).resolve() == CAND_DB.resolve()
    assert Path(validation['database']).resolve() == (DATA / 'rebuilt-breadth-2026-10-14.sqlite').resolve()
    assert sha(CAND_DB) == report['candidate_database_sha256'] == acceptance['candidate_database_sha256']
    assert sha(CAND_MANIFEST) == report['candidate_manifest_sha256'] == acceptance['candidate_manifest_sha256']
    rebuild.read_manifest(CAND_MANIFEST)
    for name, digest in report['accepted_input_hashes'].items():
        assert sha(ROOT / name) == digest, name
    for name, digest in report['new_input_hashes'].items():
        assert sha(DATA / name) == digest, name
    live_db, live_manifest = DATA / 'devices.sqlite', DATA / 'import-manifest.json'
    assert {p.name: sha(p) for p in (live_db, live_manifest)} == report['live_unchanged']
    for suffix in ('-journal', '-wal', '-shm'):
        assert not (DATA / ('devices.sqlite' + suffix)).exists()
    backups = [(live_db, DATA / 'devices-before-breadth-2026-10-14.sqlite'),
               (live_manifest, DATA / 'import-manifest-before-breadth-2026-10-14.json')]
    pending_db, pending_manifest = DATA / '.devices-breadth-2026-10-14.pending', DATA / '.manifest-breadth-2026-10-14.pending'
    for path in [dest for _, dest in backups] + [pending_db, pending_manifest]:
        assert not path.exists(), path
    for source, dest in backups:
        clone(source, dest, report['live_unchanged'][source.name])
    clone(CAND_DB, pending_db, report['candidate_database_sha256'])
    clone(CAND_MANIFEST, pending_manifest, report['candidate_manifest_sha256'])
    journal = {'old': report['live_unchanged'], 'new': {live_db.name: sha(pending_db), live_manifest.name: sha(pending_manifest)},
               'backups': {str(p.relative_to(ROOT)): sha(p) for _, p in backups},
               'review_sha256': sha(review), 'suite_sha256': sha(proof)}
    save(OUT / 'promotion-journal.json', journal)
    # No imports/writers are active; publish complete validated copies.
    os.replace(pending_db, live_db)
    os.replace(pending_manifest, live_manifest)
    assert journal['new'] == {p.name: sha(p) for p in (live_db, live_manifest)}
    rebuild.read_manifest(live_manifest)
    save(OUT / 'promotion-result.json', {'promoted': True, **journal})
    print(json.dumps(journal, indent=2))


if __name__ == '__main__':
    main()

"""Promote the independently accepted local batch, preserving the old live pair."""
import json
import os
from pathlib import Path
from stage import ROOT, OUT, DATA, CAND_DB, CAND_MANIFEST, sha, save, clone, rebuild


def main():
    acceptance = json.loads((OUT / 'promotion-acceptance.json').read_text())
    assert acceptance['accepted'] is True
    for name, digest in acceptance['proof_hashes'].items():
        assert sha(ROOT / name) == digest, name
    review = ROOT / acceptance['review_path']
    assert sha(review) == acceptance['review_sha256']
    report = json.loads((OUT / 'stage-report.json').read_text())
    proof = ROOT / 'research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json'
    suite = json.loads(proof.read_text())
    assert suite['passed'] and all(suite[k] == 0 for k in ('failures', 'errors', 'skipped', 'blocked_network_or_external_process_calls'))
    code = json.loads((OUT / 'code-under-test.json').read_text())
    assert all(sha(ROOT / path) == digest for path, digest in code['files'].items())
    comparison = json.loads((DATA / 'exports/wave3-2026-10-12-comparison.json').read_text())
    assert len(comparison) == 52
    assert all(r['original'] == r['rebuilt'] and r['only_original'] == r['only_rebuilt'] == 0 for r in comparison.values())
    for name, count in report['counts'].items():
        assert comparison[name]['original'] == count
    validation = json.loads((DATA / 'exports/wave3-2026-10-12-validation.json').read_text())
    assert validation['matches_reference_baseline'] and validation['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
    assert Path(validation['reference_database']).resolve() == CAND_DB.resolve()
    assert Path(validation['database']).resolve() == (DATA / 'rebuilt-wave3-2026-10-12.sqlite').resolve()
    assert sha(CAND_DB) == report['candidate_database_sha256'] == acceptance['candidate_database_sha256']
    assert sha(CAND_MANIFEST) == report['candidate_manifest_sha256'] == acceptance['candidate_manifest_sha256']
    rebuild.read_manifest(CAND_MANIFEST)
    live_db, live_manifest = DATA / 'devices.sqlite', DATA / 'import-manifest.json'
    assert {p.name: sha(p) for p in (live_db, live_manifest)} == report['live_unchanged']
    for suffix in ('-journal', '-wal', '-shm'):
        assert not (DATA / ('devices.sqlite' + suffix)).exists()
    backups = [(live_db, DATA / 'devices-before-wave3-2026-10-12.sqlite'),
               (live_manifest, DATA / 'import-manifest-before-wave3-2026-10-12.json')]
    for source, dest in backups:
        clone(source, dest)
    pending_db, pending_manifest = DATA / '.devices-wave3-2026-10-12.pending', DATA / '.manifest-wave3-2026-10-12.pending'
    clone(CAND_DB, pending_db)
    clone(CAND_MANIFEST, pending_manifest)
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

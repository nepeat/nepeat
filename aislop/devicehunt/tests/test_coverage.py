"""C3a / contract E1: coverage manifests on isolated campaign fixtures only."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from devicehunt import coverage
from tests.test_campaign import INVENTORY, UNITS, build_workspace, fresh, rewrite, runner_for, tree


@unittest.skipUnless(INVENTORY.exists(), 'pinned c0 inventory unavailable')
class CoverageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.workspace = Path(self.tmp.name)
        for entry in ('socket.socket.connect', 'socket.create_connection', 'socket.getaddrinfo',
                      'urllib.request.OpenerDirector.open', 'subprocess.run', 'subprocess.Popen'):
            guard = patch(entry, side_effect=AssertionError('forbidden network/external entrypoint'))
            guard.start()
            self.addCleanup(guard.stop)

    def tearDown(self):
        self.tmp.cleanup()

    def start(self, **kwargs):
        build_workspace(self.workspace, **kwargs)
        fresh(self.workspace)[0].close()
        runner = runner_for(self.workspace)
        self.addCleanup(runner.archive.close)
        return runner

    def freeze(self, runner, run_id):
        manifest = coverage.build(runner, run_id)
        path = coverage.publish(manifest, self.workspace / 'coverage', workspace=self.workspace,
                                archive_root=runner.archive.root)
        return manifest, path

    def test_mixed_run_freezes_exact_closure_and_honest_counts(self):
        def mutate(workspace):
            rewrite(workspace, 'data/raw/rra-edge-core-hyphen-official/before.csv', lambda b: b'x' + b)
        runner = self.start(mutate=mutate)
        runner.run('run-1')
        manifest, path = self.freeze(runner, 'run-1')
        self.assertEqual(path.name, manifest['manifest_id'] + '.json')
        counts = manifest['counts']
        self.assertEqual((counts['declared'], counts['validated_complete'], counts['explicit_zero'], counts['failed'],
                          counts['observed'], counts['inputs_registered']), (4, 3, 2, 1, 3, 4))
        self.assertEqual(sum(r['kind'] == 'campaign-bundle' for r in manifest['receipts']), 4)
        self.assertEqual(len(manifest['receipts']), 12)  # bundle + validation + registration per unit
        selected = {a['sha256'] for a in manifest['artifacts']}
        pinned = {sha for unit in runner.definition.units for _, sha in unit.files}
        self.assertTrue(pinned <= selected)
        self.assertIsNone(manifest['evidence_release'])
        self.assertIn('not accepted catalog evidence', manifest['claims'])
        result = coverage.validate(json.loads(path.read_bytes()), workspace=self.workspace,
                                   archive_root=runner.archive.root)
        self.assertEqual(result['manifest_id'], manifest['manifest_id'])
        # Freezing is deterministic and exclusive.
        self.assertEqual(coverage.build(runner, 'run-1'), manifest)
        with self.assertRaises(coverage.CoverageError):
            self.freeze(runner, 'run-1')

    def test_explicit_zero_only_run(self):
        runner = self.start(units=('ised-rel-query-celestica', 'rra-official-query-78fc6b2e95f6'))
        runner.run('run-z')
        manifest, _ = self.freeze(runner, 'run-z')
        self.assertEqual({u['outcome'] for u in manifest['units']}, {'explicit_zero'})
        self.assertEqual(manifest['counts']['explicit_zero'], 2)

    def test_all_pending_run_freezes_with_zero_receipts_and_artifacts(self):
        runner = self.start(budgets=dict(new_metadata_and_candidates_bytes_cap=1))
        report = runner.run('run-empty')
        self.assertEqual(report['run_state'], 'stopped')
        manifest, path = self.freeze(runner, 'run-empty')
        self.assertEqual((manifest['receipts'], manifest['artifacts']), ([], []))
        self.assertEqual(manifest['counts']['budget_exhausted'], len(UNITS))
        self.assertEqual(manifest['counts']['observed'], 0)
        coverage.validate(json.loads(path.read_bytes()), workspace=self.workspace, archive_root=runner.archive.root)

    def test_missing_selected_dependency_fails_but_unrelated_damage_does_not(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092',))
        runner.run('run-1')
        manifest = coverage.build(runner, 'run-1')
        # Unrelated, never-selected archive object damaged after the run.
        unrelated = runner.archive.object_path(runner.archive.store_bytes(b'unrelated pending candidate'))
        unrelated.chmod(0o644)  # published objects are read-only; simulate outside damage
        unrelated.write_bytes(b'corrupted')
        self.assertEqual(coverage.build(runner, 'run-1'), manifest)
        self.freeze(runner, 'run-1')
        # A selected object vanishing is fatal for validation and for a new freeze.
        candidate = next(u['candidate_sha256'] for u in manifest['units'])
        path = runner.archive.object_path(candidate)
        held = path.read_bytes()
        path.unlink()
        with self.assertRaisesRegex(coverage.CoverageError, 'missing or corrupt'):
            coverage.validate(manifest, workspace=self.workspace, archive_root=runner.archive.root)
        with self.assertRaises(ValueError):
            coverage.build(runner, 'run-1')
        path.write_bytes(held)

    def test_tampering_and_scope_violations_are_rejected(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092',))
        runner.run('run-1')
        manifest = coverage.build(runner, 'run-1')
        checks = {
            'identity': lambda m: m['units'][0].update(outcome='explicit_zero'),
            'claims': lambda m: m.update(claims='device absent'),
            'bytes': lambda m: m['receipts'][0]['wrapper']['payload'].update(outcome='validated_nonempty?'),
            'closure': lambda m: m['receipts'].pop(),
            'extra': lambda m: m['artifacts'].append(dict(sha256='0' * 64, byte_count=1, locations=[])),
            'counts': lambda m: m['counts'].update(observed=99),
        }
        for label, change in checks.items():
            tampered = json.loads(json.dumps(manifest))
            change(tampered)
            if label != 'identity':
                tampered['manifest_id'] = coverage.identity(tampered)
            with self.assertRaises(coverage.CoverageError, msg=label):
                coverage.validate(tampered)

    def test_running_or_foreign_runs_are_refused_and_freeze_is_read_only(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        runner.state.start_run('run-live', campaign_id=runner.definition.campaign_id,
                               definition_sha256=runner.definition.sha256, identities=runner.identities(),
                               start_sequence=0, start_index_bytes=0)
        with self.assertRaisesRegex(coverage.CoverageError, 'terminal'):
            coverage.build(runner, 'run-live')
        runner.resume('run-live', decision='fixture')
        before = tree(runner.archive.root, locks=True)
        coverage.build(runner, 'run-live')
        self.assertEqual(tree(runner.archive.root, locks=True), before)

    def test_validate_checks_archive_bytes_honesty_and_strict_files(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092', 'rra-official-query-78fc6b2e95f6'))
        runner.run('run-1')
        manifest, path = self.freeze(runner, 'run-1')
        roots = dict(workspace=self.workspace, archive_root=runner.archive.root)
        self.assertEqual(path.stat().st_mode & 0o777, 0o444)
        self.assertEqual(coverage.load(path), manifest)
        # A self-consistent forgery: wrapper, unit summary, counts and identity all recomputed.
        forged = json.loads(json.dumps(manifest))
        bundle = next(r for r in forged['receipts'] if r['kind'] == 'campaign-bundle'
                      and r['wrapper']['payload']['outcome'] == 'validated_nonempty')
        bundle['wrapper']['payload']['outcome'] = 'explicit_zero'
        bundle['wrapper']['payload']['coverage']['explicit_zero'] = True
        bundle['wrapper_sha256'] = coverage.digest(coverage.canonical(bundle['wrapper']))
        unit = next(u for u in forged['units'] if u['bundle_receipt'] == bundle['id'])
        unit.update(outcome='explicit_zero', coverage=bundle['wrapper']['payload']['coverage'])
        forged['counts'] = coverage._counts(forged['units'])
        forged['manifest_id'] = coverage.identity(forged)
        coverage.validate(forged)  # structurally consistent on its own...
        with self.assertRaisesRegex(coverage.CoverageError, 'differs from the archive'):
            coverage.validate(forged, **roots)  # ...but not the archive's immutable bytes
        # Honesty: a failed or unproven unit cannot claim observation.
        overclaim = json.loads(json.dumps(manifest))
        receipt = next(r for r in overclaim['receipts'] if r['kind'] == 'campaign-bundle')
        receipt['wrapper']['payload']['outcome'] = 'failed'
        receipt['wrapper_sha256'] = coverage.digest(coverage.canonical(receipt['wrapper']))
        unit = next(u for u in overclaim['units'] if u['bundle_receipt'] == receipt['id'])
        unit['outcome'] = 'failed'
        overclaim['counts'] = coverage._counts(overclaim['units'])
        overclaim['manifest_id'] = coverage.identity(overclaim)
        with self.assertRaisesRegex(coverage.CoverageError, 'overclaim'):
            coverage.validate(overclaim)
        # Non-canonical or duplicate-key files are refused by the strict reader.
        loose = self.workspace / 'loose.json'
        loose.write_text(json.dumps(manifest, indent=1))
        with self.assertRaisesRegex(coverage.CoverageError, 'not canonical'):
            coverage.load(loose)
        tampered = json.loads(json.dumps(manifest))
        tampered['units'][0]['scope'] = dict(tampered['units'][0]['scope'], exact_query='changed')
        tampered['manifest_id'] = coverage.identity(tampered)
        with self.assertRaisesRegex(coverage.CoverageError, 'disagrees'):
            coverage.validate(tampered)
        with self.assertRaisesRegex(coverage.CoverageError, 'both'):
            coverage.validate(manifest, workspace=self.workspace)
        softened = json.loads(json.dumps(manifest))
        softened['claims'] = 'coverage only: device absent everywhere'
        softened['manifest_id'] = coverage.identity(softened)
        with self.assertRaisesRegex(coverage.CoverageError, 'claim-free'):
            coverage.validate(softened)
        loose.write_text('{"schema":NaN}')
        with self.assertRaisesRegex(coverage.CoverageError, 'non-finite'):
            coverage.load(loose)
        loose.write_text('{"schema":1,"schema":2}')
        with self.assertRaisesRegex(coverage.CoverageError, 'duplicate'):
            coverage.load(loose)

    def test_identity_ignores_later_copies_and_unreached_units_are_not_pending_claims(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092', 'rra-official-query-78fc6b2e95f6'))
        runner.run('run-1')
        first = coverage.build(runner, 'run-1')
        # Registering an archive copy of a selected input after the freeze changes nothing.
        selected = runner.definition.units[0].files[0]
        runner.archive.store_bytes((self.workspace / selected[0]).read_bytes())
        self.assertEqual(coverage.build(runner, 'run-1')['manifest_id'], first['manifest_id'])
        # An interrupted second run that reaches only one unit accounts only for that unit.
        stop_unit = sorted(u.unit_id for u in runner.definition.units)[0]

        def fault(point):
            if point == 'after_stage:' + stop_unit + ':bundle':
                raise KeyboardInterrupt()
        runner.fault = fault
        with self.assertRaises(KeyboardInterrupt):
            runner.run('run-2')
        partial = coverage.build(runner, 'run-2')
        outcomes = {u['unit_id']: u['outcome'] for u in partial['units']}
        self.assertEqual(sorted(outcomes.values()).count('not_reached'), 1)
        self.assertEqual(partial['counts']['not_reached'], 1)
        coverage.validate(partial, workspace=self.workspace, archive_root=runner.archive.root)

    def test_cli_freeze_and_validate(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        runner.run('run-1')
        runner.archive.close()
        common = ['--workspace', str(self.workspace), '--archive', str(self.workspace / 'archive'),
                  '--definition', 'definition.json', '--inventory', 'inventory.json']
        with patch('sys.stdout') as out:
            self.assertEqual(coverage.main(common + ['freeze', 'run-1', '--output', str(self.workspace / 'cov')]), 0)
        result = json.loads(''.join(c.args[0] for c in out.write.call_args_list))
        with patch('sys.stdout'):
            self.assertEqual(coverage.main(common + ['validate', result['path']]), 0)


if __name__ == '__main__':
    unittest.main()

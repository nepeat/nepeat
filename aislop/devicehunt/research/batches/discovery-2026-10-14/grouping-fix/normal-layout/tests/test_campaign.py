"""C2 offline campaign gates on isolated copies of small pinned saved units.

Real saved inputs are only read; every archive, operational store and fixture
workspace is temporary. Network and external-process entrypoints are forbidden.
"""
from contextlib import closing
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from devicehunt import campaign
from devicehunt.artifacts import Archive
from devicehunt.campaign import Runner, load_definition, open_runner
from devicehunt.campaign_state import CampaignState, FileLocks, LeaseBusy, StateError

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / campaign.DEFAULT_INVENTORY
DEFINITION = ROOT / campaign.DEFAULT_DEFINITION
UNITS = ('ised-rel-25869', 'ised-rel-query-celestica',
         'rra-official-query-5827f2fe7092', 'rra-official-query-78fc6b2e95f6')


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def build_workspace(workspace, units=UNITS, budgets=None, mutate=None):
    """Copy selected units, optionally mutate files, then pin their actual hashes."""
    inventory = json.loads(INVENTORY.read_text())
    selected = [u for u in inventory['units'] if u['unit_id'] in units]
    for unit in selected:
        for path in unit['dependency_paths']:
            target = workspace / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / path, target)
    if mutate:
        mutate(workspace)
    files = sorted({p for u in selected for p in u['dependency_paths']})
    pinned = dict(campaign_id=inventory['campaign_id'], units=selected,
                  pilot_source_files=[dict(path=p, sha256=sha(workspace / p)) for p in files if (workspace / p).exists()])
    definition = json.loads(DEFINITION.read_text())
    raw = json.dumps(pinned, sort_keys=True).encode()
    (workspace / 'inventory.json').write_bytes(raw)
    definition.update(inventory_file_sha256=hashlib.sha256(raw).hexdigest())
    if budgets:
        definition['budgets'] = dict(definition['budgets'], **budgets)
    (workspace / 'definition.json').write_text(json.dumps(definition))


def rewrite(workspace, relative, change):
    """Mutate one saved file and keep its sibling manifest's declared hash consistent."""
    path = workspace / relative
    old = sha(path)
    path.write_bytes(change(path.read_bytes()))
    for manifest in path.parent.glob('*.json'):
        text = manifest.read_text()
        if old in text:
            manifest.write_text(text.replace(old, sha(path)))


def fresh(workspace):
    archive = Archive(workspace / 'archive', workspace=workspace, create=True)
    state = CampaignState.initialize(archive.root)
    state.synchronize_archive(archive, decision='fixture binding')
    return archive, state


def runner_for(workspace, **kwargs):
    archive = Archive(workspace / 'archive', workspace=workspace)
    state = CampaignState(archive.root)
    return Runner(archive, state, load_definition(workspace, 'definition.json', 'inventory.json'), **kwargs)


def _crash_after_result(workspace, unit, stage):
    def fault(point):
        if point == 'after_result:' + unit + ':' + stage:
            os._exit(17)  # hard crash: no handlers, no run finalization
    runner_for(Path(workspace), fault=fault).run('run-crash')


def tree(root, locks=False):
    return {p.relative_to(root).as_posix(): sha(p) for p in sorted(Path(root).rglob('*'))
            if p.is_file() and (locks or 'capture-locks' not in p.parts)}


@unittest.skipUnless(INVENTORY.exists(), 'pinned c0 inventory unavailable')
class CampaignRunnerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.workspace = Path(self.tmp.name)
        for entry in ('socket.socket.connect', 'socket.create_connection', 'socket.getaddrinfo',
                      'http.client.HTTPConnection.connect', 'urllib.request.OpenerDirector.open',
                      'subprocess.run', 'subprocess.Popen'):
            guard = patch(entry, side_effect=AssertionError('forbidden network/external entrypoint'))
            guard.start()
            self.addCleanup(guard.stop)

    def tearDown(self):
        self.tmp.cleanup()

    def start(self, **kwargs):
        build_workspace(self.workspace, **kwargs)
        archive, state = fresh(self.workspace)
        archive.close()
        runner = runner_for(self.workspace)
        self.addCleanup(runner.archive.close)
        return runner

    def outcomes(self, report):
        return {u['unit_id']: u['outcome'] for u in report['units']}

    def test_two_offline_runs_match_pins_and_second_only_reuses(self):
        runner = self.start()
        first = runner.run('run-1')
        self.assertEqual(first['run_state'], 'complete')
        self.assertEqual(self.outcomes(first), {
            'ised-rel-25869': 'validated_nonempty', 'ised-rel-query-celestica': 'explicit_zero',
            'rra-official-query-5827f2fe7092': 'validated_nonempty', 'rra-official-query-78fc6b2e95f6': 'explicit_zero'})
        self.assertTrue(all(u['pinned_comparison'] == 'matches_pinned' for u in first['units']))
        self.assertEqual(first['job_actions'], {'executed': 12})
        self.assertEqual(first['units_accounted_for']['validated_complete'], 4)
        self.assertEqual(first['units_accounted_for']['explicit_zero'], 2)
        self.assertGreater(first['new_bytes'], 0)
        # Saved sources are registered in place: no source byte is copied.
        for path, digest_ in runner.definition.units[0].files:
            self.assertFalse(runner.archive.object_path(digest_).exists(), path)
        sequence = runner.archive.readiness()['sequence']
        second = runner.run('run-2')
        self.assertEqual(second['job_actions'], {'reused': 12})
        self.assertEqual(second['new_bytes'], 0)
        self.assertEqual(runner.archive.readiness()['sequence'], sequence)
        self.assertEqual({u['change_from_previous_run'] for u in second['units']}, {'unchanged'})
        self.assertEqual([u['candidate_sha256'] for u in first['units']], [u['candidate_sha256'] for u in second['units']])
        self.assertEqual(len(runner.state.campaign_jobs(runner.definition.campaign_id)), 12)

    def test_occurrences_keep_duplicate_certificates_and_stable_ids(self):
        runner = self.start(units=('ised-rel-25869',))
        report = runner.run('run-1')
        bundle = json.loads(runner.archive.resolve(report['units'][0]['candidate_sha256']).read_bytes())
        validation = runner.archive.verified_receipt(
            runner.archive.verified_receipt(runner.state.campaign_jobs(runner.definition.campaign_id)[0]['result_id'])
            ['wrapper']['payload']['validation_id'])['wrapper']['payload']
        output = json.loads(runner.archive.resolve(validation['outputs'][0]['sha256']).read_bytes())
        html = [o for o in output['occurrences'] if o['kind'] == 'ised-html-row']
        # The repeated certificate stays three separate observations.
        self.assertEqual([o['certificate'] for o in html].count('25869-TE1A001'), 3)
        self.assertEqual(len(set(bundle['occurrence_ids'])), len(bundle['occurrence_ids']))
        self.assertEqual(bundle['review'], 'candidate only; not accepted catalog evidence')

    def test_result_published_before_checkpoint_crash_recovers_once(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092',))
        context = multiprocessing.get_context('spawn')
        process = context.Process(target=_crash_after_result,
                                  args=(str(self.workspace), 'rra-official-query-5827f2fe7092', 'validate'))
        process.start()
        process.join(60)
        self.assertEqual(process.exitcode, 17)
        self.assertEqual(runner.state.campaign_run('run-crash')['state'], 'running')
        sequence = runner.archive.readiness()['sequence']
        report = runner.resume('run-crash', decision='fixture crash recovery')
        actions = [e['action'] for e in runner.state.campaign_executions(run_id='run-crash')]
        # The completed register job gets an operational reuse event, not new work.
        self.assertEqual(actions, ['executed', 'reused', 'recovered', 'executed'])
        self.assertEqual(report['run_state'], 'complete')
        # Only the bundle stage publishes after resume: candidate object registration + result.
        self.assertEqual(runner.archive.readiness()['sequence'] - sequence, 2)

    def test_interrupt_after_one_unit_then_explicit_resume(self):
        build_workspace(self.workspace)
        fresh(self.workspace)[0].close()
        first_unit = sorted(UNITS)[0]

        def fault(point):
            if point == 'after_stage:' + first_unit + ':bundle':
                raise KeyboardInterrupt()
        runner = runner_for(self.workspace, fault=fault)
        self.addCleanup(runner.archive.close)
        with self.assertRaises(KeyboardInterrupt):
            runner.run('run-i')
        self.assertTrue(runner.state.campaign_run('run-i')['reason'].startswith('interrupted'))
        with self.assertRaises(StateError):
            runner.run('run-i')  # run IDs are never reused
        runner.fault = lambda point: None
        report = runner.resume('run-i', decision='fixture resume after interrupt')
        self.assertEqual(report['run_state'], 'complete')
        executions = runner.state.campaign_executions(run_id='run-i')
        self.assertEqual(sum(e['action'] == 'executed' for e in executions), 12)
        self.assertEqual(sum(e['action'] == 'reused' for e in executions), 3)
        self.assertEqual(len(runner.state.campaign_jobs(runner.definition.campaign_id)), 12)
        clean = Path(self.tmp.name) / 'clean'
        clean.mkdir()
        build_workspace(clean)
        fresh(clean)[0].close()
        uninterrupted = runner_for(clean)
        self.addCleanup(uninterrupted.archive.close)
        expected = uninterrupted.run('run-c')
        self.assertEqual([u['candidate_sha256'] for u in report['units']],
                         [u['candidate_sha256'] for u in expected['units']])

    def test_parser_change_reruns_validation_and_bundle_never_stale_reuse(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092',))
        runner.run('run-1')
        old_bundle = runner.state.campaign_jobs(runner.definition.campaign_id)
        changed = dict(runner.recipes)
        code = dict(changed['validate:rra']['code_sha256'], **{'devicehunt/rra.py': '1' * 64})
        changed['validate:rra'] = dict(changed['validate:rra'], code_sha256=code, id='campaign-recipe-' + '1' * 64)
        runner.recipes = changed  # as if only the RRA parser file changed
        report = runner.run('run-2')
        self.assertEqual(report['job_actions'], {'reused': 1, 'executed': 2})
        unit = runner.definition.units[0]
        bundle = runner.archive.verified_receipt(campaign.result_id(unit, 'bundle', runner.recipe(unit, 'bundle')))
        self.assertEqual(bundle['wrapper']['payload']['validation_id'],
                         campaign.result_id(unit, 'validate', runner.recipe(unit, 'validate')))
        self.assertEqual(len(runner.state.campaign_jobs(runner.definition.campaign_id)), len(old_bundle) + 2)

    def test_changed_expectations_rerun_only_the_bundle_comparison(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092',))
        runner.run('run-1')
        unit = runner.definition.units[0]
        edited = dict(unit.expectations, model_assertions=-1)
        runner.definition = campaign.Definition(**{**runner.definition.__dict__,
                                                   'units': (campaign.Unit(**{**unit.__dict__, 'expectations': edited}),)})
        report = runner.run('run-2')
        self.assertEqual(report['job_actions'], {'reused': 2, 'executed': 1})
        self.assertEqual(report['units'][0]['pinned_comparison'], 'differs_from_pinned')
        self.assertEqual(report['units'][0]['mismatches'], ['model_assertions'])
        self.assertEqual(report['units'][0]['change_from_previous_run'], 'unchanged')  # same candidate bytes

    def test_run_refuses_while_a_crashed_run_is_unresolved(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        runner.state.start_run('run-crashed', campaign_id=runner.definition.campaign_id,
                               definition_sha256=runner.definition.sha256, identities=runner.identities(),
                               start_sequence=0, start_index_bytes=0)
        with self.assertRaisesRegex(StateError, 'resumed explicitly'):
            runner.run('run-new')
        self.assertEqual(runner.resume('run-crashed', decision='fixture')['run_state'], 'complete')

    def test_definition_change_refuses_resume(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        runner.state.start_run('run-d', campaign_id=runner.definition.campaign_id,
                               definition_sha256=runner.definition.sha256, identities=runner.identities(),
                               start_sequence=0, start_index_bytes=0)
        runner.state.finish_run('run-d', 'stopped', 'fixture')
        runner.definition = campaign.Definition(**{**runner.definition.__dict__, 'sha256': '2' * 64})
        with self.assertRaisesRegex(StateError, 'incompatible checkpoint'):
            runner.resume('run-d', decision='should fail')

    def test_byte_accounting_includes_interrupted_attempts(self):
        runner = self.start(units=('rra-official-query-5827f2fe7092',))
        files = lambda: sum(p.stat().st_size for d in ('receipts', 'objects')
                            for p in (runner.archive.root / d).rglob('*') if p.is_file())
        before = files()
        context = multiprocessing.get_context('spawn')
        process = context.Process(target=_crash_after_result,
                                  args=(str(self.workspace), 'rra-official-query-5827f2fe7092', 'bundle'))
        process.start()
        process.join(60)
        self.assertEqual(process.exitcode, 17)
        report = runner.resume('run-crash', decision='fixture crash recovery')
        self.assertEqual(report['physical_growth_since_run_start']['receipts_and_objects'], files() - before)
        self.assertEqual(report['campaign_growth_bytes'], report['new_bytes'])
        self.assertGreater(report['physical_growth_since_run_start']['index'], 0)

    def test_incompatible_checkpoint_resume_fails_visibly(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        runner.state.start_run('run-x', campaign_id=runner.definition.campaign_id,
                               definition_sha256=runner.definition.sha256, identities=runner.identities(),
                               start_sequence=0, start_index_bytes=0)
        runner.state.finish_run('run-x', 'stopped', 'fixture')
        changed = campaign.recipes()
        changed['validate:rra'] = dict(changed['validate:rra'], id='campaign-recipe-' + '0' * 64)
        runner.recipes = changed
        with self.assertRaisesRegex(StateError, 'incompatible checkpoint'):
            runner.resume('run-x', decision='should fail')

    def test_saved_input_failures_are_checkpointed_outcomes(self):
        def mutate(workspace):
            # ISED HTML error page in place of the result table.
            for saved in sorted((workspace / 'data/raw/ised-amazon-export').iterdir()):
                if b'equipmentSearchResultTable' in saved.read_bytes():
                    rewrite(workspace, saved.relative_to(workspace).as_posix(),
                            lambda b: b'<html><body>Service unavailable</body></html>')
            # Malformed RRA CSV header.
            rewrite(workspace, 'data/raw/rra-edge-core-hyphen-official/before.csv', lambda b: b'x' + b)
            # Changed ISED total on an explicit-zero page.
            pages = json.loads((workspace / 'data/raw/ised-celestica/capture-paged.json').read_text())['result_pages']
            rewrite(workspace, 'data/raw/ised-celestica/' + pages[0], lambda b: b.replace(b'foundItems">0<', b'foundItems">7<', 1))
        runner = self.start(mutate=mutate)
        report = runner.run('run-f')
        outcomes = self.outcomes(report)
        self.assertEqual(outcomes['ised-rel-25869'], 'failed')
        self.assertEqual(outcomes['rra-official-query-5827f2fe7092'], 'failed')
        self.assertEqual(outcomes['ised-rel-query-celestica'], 'failed')
        self.assertEqual(outcomes['rra-official-query-78fc6b2e95f6'], 'explicit_zero')
        counts = report['units_accounted_for']
        # Failed saved inputs are accounted for but are not successful observations.
        self.assertEqual((counts['failed'], counts['validated_complete'], counts['observed'], counts['inputs_registered']),
                         (3, 1, 1, 4))
        self.assertEqual(report['run_state'], 'complete')

    def test_missing_pinned_input_stops_with_job_pending_then_resumes(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        missing = self.workspace / runner.definition.units[0].files[0][0]
        held = missing.read_bytes()
        missing.unlink()
        with self.assertRaisesRegex(StateError, 'missing or changed'):
            runner.run('run-m')
        self.assertEqual({j['state'] for j in runner.state.campaign_jobs(runner.definition.campaign_id)}, {'pending'})
        missing.write_bytes(held)
        self.assertEqual(runner.resume('run-m', decision='input restored')['run_state'], 'complete')

    def test_capacity_cap_stops_and_reports_budget_exhaustion(self):
        runner = self.start(budgets=dict(new_metadata_and_candidates_bytes_cap=20000))
        report = runner.run('run-b')
        self.assertEqual(report['run_state'], 'stopped')
        self.assertTrue(report['reason'].startswith('budget_exhausted'))
        self.assertGreater(report['units_accounted_for']['budget_exhausted'], 0)
        self.assertLessEqual(report['new_bytes'], 20000)
        floor = runner_for(self.workspace, disk_usage=lambda path: shutil._ntuple_diskusage(10, 10, 0))
        self.addCleanup(floor.archive.close)
        self.assertEqual(floor.run('run-floor')['run_state'], 'stopped')

    def test_competing_runner_is_refused(self):
        runner = self.start(units=('rra-official-query-78fc6b2e95f6',))
        other = FileLocks(runner.archive.root / 'capture-locks')
        other.acquire(['campaign:' + runner.definition.campaign_id])
        try:
            with self.assertRaises(LeaseBusy):
                runner.run('run-busy')
        finally:
            other.release(['campaign:' + runner.definition.campaign_id])
        self.assertEqual(runner.state.campaign_runs(runner.definition.campaign_id), [])

    def test_read_only_plan_status_and_report_do_not_write(self):
        runner = self.start(units=('ised-rel-query-celestica',))
        runner.run('run-r')
        before = tree(self.workspace, locks=True)
        readonly = open_runner(self.workspace, self.workspace / 'archive', 'definition.json', 'inventory.json', readonly=True)
        try:
            plan = readonly.plan()
            self.assertEqual({s['state'] for u in plan['units'] for s in u['stages']}, {'complete'})
            self.assertEqual(readonly.report()['run_id'], 'run-r')
        finally:
            readonly.archive.close()
        self.assertEqual(tree(self.workspace, locks=True), before)
        missing = Path(self.tmp.name) / 'absent'
        with self.assertRaises(Exception):
            open_runner(self.workspace, missing, 'definition.json', 'inventory.json', readonly=True)
        self.assertFalse(missing.exists())

    def test_definition_pins_are_enforced(self):
        build_workspace(self.workspace, units=('rra-official-query-78fc6b2e95f6',))
        (self.workspace / 'inventory.json').write_bytes((self.workspace / 'inventory.json').read_bytes() + b' ')
        with self.assertRaisesRegex(ValueError, 'inventory does not match'):
            load_definition(self.workspace, 'definition.json', 'inventory.json')

    def test_cli_plan_is_read_only(self):
        self.start(units=('rra-official-query-78fc6b2e95f6',))
        before = tree(self.workspace)
        with patch('sys.stdout') as out:
            code = campaign.main(['--workspace', str(self.workspace), '--archive', str(self.workspace / 'archive'),
                                  '--definition', 'definition.json', '--inventory', 'inventory.json', 'plan'])
        self.assertEqual(code, 0)
        self.assertIn('rra-official-query-78fc6b2e95f6', ''.join(c.args[0] for c in out.write.call_args_list))
        self.assertEqual(tree(self.workspace), before)


class CampaignSchemaUpgradeTests(unittest.TestCase):
    def test_schema2_store_upgrades_explicitly_with_verified_backup(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Archive(Path(tmp) / 'archive', workspace=tmp, create=True)
            state = CampaignState.initialize(archive.root)
            state.allocate_account('a', definition_hash='d', policy_hash='p', request_limit=1, byte_limit=1)
            with closing(sqlite3.connect(state.path)) as db, db:
                for table in ('campaign_executions', 'campaign_jobs', 'campaign_runs'):
                    db.execute('DROP TABLE ' + table)
                db.execute('UPDATE operational_meta SET version=2')
                accounts = db.execute('SELECT * FROM accounts').fetchall()
            with self.assertRaises(StateError):
                CampaignState(archive.root)
            with self.assertRaises(StateError):
                CampaignState.upgrade_campaign_jobs(archive.root, quiesced=True)

            def fault(point):
                if point == 'campaign_jobs_before_commit':
                    raise KeyboardInterrupt()
            with self.assertRaises(KeyboardInterrupt):
                CampaignState.upgrade_campaign_jobs(archive.root, quiesced=True, decision='fixture', fault=fault)
            with closing(sqlite3.connect(state.path)) as db:
                self.assertEqual(db.execute('SELECT version FROM operational_meta').fetchone()[0], 2)
            # A retry reuses the interrupted attempt's exactly matching backup.
            upgraded = CampaignState.upgrade_campaign_jobs(archive.root, quiesced=True, decision='fixture')
            self.assertEqual(upgraded.campaign_runs('c'), [])
            with closing(sqlite3.connect(state.path)) as db:
                self.assertEqual(db.execute('SELECT * FROM accounts').fetchall(), accounts)
            with closing(sqlite3.connect(archive.root / 'capture-leases.before-c2.sqlite')) as db:
                self.assertEqual(db.execute('SELECT version FROM operational_meta').fetchone()[0], 2)
            with self.assertRaises(StateError):
                CampaignState.upgrade_campaign_jobs(archive.root, quiesced=True, decision='again')
            archive.close()

    def test_schema2_upgrade_refuses_conflicting_backup_and_active_leases(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Archive(Path(tmp) / 'archive', workspace=tmp, create=True)
            state = CampaignState.initialize(archive.root)
            with closing(sqlite3.connect(state.path)) as db, db:
                for table in ('campaign_executions', 'campaign_jobs', 'campaign_runs'):
                    db.execute('DROP TABLE ' + table)
                db.execute('UPDATE operational_meta SET version=2')
                db.execute("INSERT INTO leases VALUES ('host:x','owner',1)")
            with self.assertRaisesRegex(StateError, 'active leases'):
                CampaignState.upgrade_campaign_jobs(archive.root, quiesced=True, decision='fixture')
            with closing(sqlite3.connect(state.path)) as db, db:
                db.execute('DELETE FROM leases')
            shutil.copy2(state.path, archive.root / 'capture-leases.before-c2.sqlite')
            with closing(sqlite3.connect(state.path)) as db, db:
                db.execute("INSERT INTO lease_fences VALUES ('host:y',1,NULL,0)")
            with self.assertRaisesRegex(StateError, 'conflicts'):
                CampaignState.upgrade_campaign_jobs(archive.root, quiesced=True, decision='fixture')
            archive.close()


if __name__ == '__main__':
    unittest.main()

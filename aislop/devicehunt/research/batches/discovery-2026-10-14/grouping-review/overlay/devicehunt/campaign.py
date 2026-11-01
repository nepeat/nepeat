"""Bounded offline campaign runner (c2): plan, run, status, resume and report.

Saved inputs only; this runner has no network path. Jobs bind unit revision +
stage + recipe. Each result is a deterministic, immutable archive receipt that
is published before its checkpoint completes, so a crash between the two is
finished on resume from the durable receipt, without redoing the work. A new
run reuses completed compatible jobs and records only operational reuse events.
"""
from __future__ import annotations
import argparse
import json
import platform
import shutil
import sys
import time
import uuid
from dataclasses import dataclass
from pathlib import Path

from .artifacts import Archive, _safe, canonical, digest, file_digest
from .campaign_sources import ADAPTERS, INPUT_ERRORS, module_hashes
from .campaign_state import CampaignState, FileLocks, StateError, StopCollection

RUNNER_MODULES = ('devicehunt/campaign.py',)
STAGES = ('register', 'validate', 'bundle')
# c3b re-pin (2026-10-10): same units and pinned files as the c0 pilot revision, current checkpoints.
DEFAULT_DEFINITION = 'research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/campaign-definition.json'
DEFAULT_INVENTORY = 'research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/inventory.json'


def _sha(value):
    return digest(canonical(value))


class CapacityStop(StopCollection):
    pass


@dataclass(frozen=True)
class Unit:
    unit_id: str
    source: str
    manifest: str
    files: tuple
    scope: dict
    expectations: dict
    revision: str


@dataclass(frozen=True)
class Definition:
    campaign_id: str
    version: int
    sha256: str
    inventory_sha256: str
    mode: str
    budgets: dict
    units: tuple


def load_definition(workspace, definition_path=DEFAULT_DEFINITION, inventory_path=DEFAULT_INVENTORY):
    """Pinned definition + inventory; every unit input carries an exact hash."""
    workspace = Path(workspace).absolute()
    raw = _safe(workspace, definition_path).read_bytes()
    definition = json.loads(raw)
    inventory_raw = _safe(workspace, inventory_path).read_bytes()
    if digest(inventory_raw) != definition.get('inventory_file_sha256'):
        raise ValueError('inventory does not match the pinned campaign definition')
    inventory = json.loads(inventory_raw)
    if inventory.get('campaign_id') != definition.get('campaign_id') or not definition.get('campaign_id'):
        raise ValueError('campaign identity mismatch')
    if definition.get('mode') != 'offline_saved_inputs_only':
        raise ValueError('only offline saved-input campaigns are implemented')
    budgets = definition.get('budgets', {})
    if budgets.get('network_requests') != 0 or budgets.get('network_bytes') != 0:
        raise ValueError('offline campaign requires zero network budget')
    for key in ('new_metadata_and_candidates_bytes_cap', 'free_space_floor_bytes', 'max_source_jobs',
                'max_referenced_source_paths'):
        if type(budgets.get(key)) is not int or budgets[key] < 0:
            raise ValueError('definition requires a nonnegative integer budget: ' + key)
    hashes = {}
    for entry in inventory['pilot_source_files']:
        if hashes.setdefault(entry['path'], entry['sha256']) != entry['sha256']:
            raise ValueError('conflicting pinned hash: ' + entry['path'])
    units, seen = [], set()
    for item in inventory['units']:
        if item['source'] not in ADAPTERS:
            raise ValueError('no saved adapter for source: ' + item['source'])
        if item['unit_id'] in seen:
            raise ValueError('duplicate unit: ' + item['unit_id'])
        seen.add(item['unit_id'])
        paths = sorted(set(item['dependency_paths']))
        if item['source_manifest'] not in paths:
            raise ValueError('unit manifest is not a pinned dependency')
        files = tuple((path, hashes[path]) for path in paths)
        revision = 'unit-' + _sha(dict(campaign_id=definition['campaign_id'], definition_version=definition['version'],
                                       unit_id=item['unit_id'], source=item['source'], manifest=item['source_manifest'],
                                       files=[list(f) for f in files], scope=item['scope']))
        units.append(Unit(item['unit_id'], item['source'], item['source_manifest'], files, item['scope'],
                          item.get('expectations') or {}, revision))
    if not units:
        raise ValueError('campaign declares no units')
    if len(units) > budgets['max_source_jobs'] or len({f for u in units for f in u.files}) > budgets['max_referenced_source_paths']:
        raise ValueError('campaign exceeds its declared unit/path limits')
    return Definition(definition['campaign_id'], definition['version'], digest(raw), digest(inventory_raw),
                      definition['mode'], budgets, tuple(sorted(units, key=lambda u: u.unit_id)))


def recipes():
    """Code/runtime identities from each stage's full local import closure.

    A changed parser yields new jobs over the same bytes; stage recipes are then
    chained per unit (see Runner.recipe) so downstream jobs follow their upstream.
    """
    runtime = dict(python=platform.python_version())
    result = dict(register=dict(name='in-place-registration', version='1',
                                code_sha256=module_hashes(RUNNER_MODULES + ('devicehunt/artifacts.py',)), **runtime),
                  bundle=dict(name='candidate-bundle', version='1',
                              code_sha256=module_hashes(RUNNER_MODULES + ('devicehunt/campaign_sources.py',)), **runtime))
    for source, adapter in ADAPTERS.items():
        result['validate:' + source] = dict(name='saved-' + source + '-validation', version=adapter.version,
                                            code_sha256=module_hashes(RUNNER_MODULES + adapter.modules), **runtime)
    return {key: dict(value, id='campaign-recipe-' + _sha(value)) for key, value in result.items()}


def job_id(unit, stage, recipe):
    return 'job-' + _sha([unit.revision, stage, recipe['id']])


def result_id(unit, stage, recipe):
    return 'campaign-' + stage + '-' + _sha([unit.revision, stage, recipe['id']])


class Runner:
    def __init__(self, archive, state, definition, *, clock=time.time, fault=None, disk_usage=shutil.disk_usage):
        if not archive.workspace_explicit:
            raise ValueError('campaign runner requires an explicit workspace root')
        if state.root != archive.root:
            raise StateError('operational state must belong to this archive')
        self.archive, self.state, self.definition = archive, state, definition
        self.clock, self.fault, self.disk_usage = clock, fault or (lambda point: None), disk_usage
        self.recipes = recipes()
        self._spent = None

    # -- identities and planning (read-only) -----------------------------------------
    def identities(self):
        return dict(definition_sha256=self.definition.sha256, inventory_sha256=self.definition.inventory_sha256,
                    recipes={key: value['id'] for key, value in sorted(self.recipes.items())})

    def recipe(self, unit, stage):
        """Per-unit chained recipe: a downstream job binds its upstream recipe and,
        for bundles, the pinned expectations it is compared against."""
        register = self.recipes['register']
        if stage == 'register':
            return register
        validate = dict(self.recipes['validate:' + unit.source], upstream=register['id'])
        validate['id'] = 'campaign-recipe-' + _sha({k: v for k, v in validate.items() if k != 'id'})
        if stage == 'validate':
            return validate
        bundle = dict(self.recipes['bundle'], upstream=validate['id'], expectations_sha256=_sha(unit.expectations))
        bundle['id'] = 'campaign-recipe-' + _sha({k: v for k, v in bundle.items() if k != 'id'})
        return bundle

    def plan(self):
        """Read-only: declared work and its durable checkpoint state."""
        jobs = {row['id']: row for row in self.state.campaign_jobs(self.definition.campaign_id)}
        units = []
        for unit in self.definition.units:
            stages = []
            for stage in STAGES:
                recipe = self.recipe(unit, stage)
                row = jobs.get(job_id(unit, stage, recipe))
                stages.append(dict(stage=stage, job_id=job_id(unit, stage, recipe), recipe_id=recipe['id'],
                                   state=row['state'] if row else 'unplanned', outcome=row['outcome'] if row else None))
            units.append(dict(unit_id=unit.unit_id, source=unit.source, unit_revision=unit.revision,
                              files=len(unit.files), stages=stages))
        return dict(campaign_id=self.definition.campaign_id, definition_sha256=self.definition.sha256,
                    mode=self.definition.mode, network_requests=0, units=units)

    # -- execution -------------------------------------------------------------------
    def run(self, run_id=None):
        run_id = run_id or 'run-' + str(uuid.uuid4())
        with self._exclusive():
            unfinished = [r['id'] for r in self.state.campaign_runs(self.definition.campaign_id) if r['state'] == 'running']
            if unfinished:
                raise StateError('crashed run must be resumed explicitly: ' + ', '.join(unfinished))
            ready = self.archive.require_ready()
            self.state.start_run(run_id, campaign_id=self.definition.campaign_id,
                                 definition_sha256=self.definition.sha256, identities=self.identities(),
                                 start_sequence=ready['sequence'], start_index_bytes=self._index_bytes())
            return self._execute(run_id)

    def resume(self, run_id, *, decision):
        with self._exclusive():
            run = self.state.campaign_run(run_id)
            if run['campaign_id'] != self.definition.campaign_id:
                raise StateError('run belongs to another campaign')
            if run['definition_sha256'] != self.definition.sha256 or run['identities'] != self.identities():
                raise StateError('incompatible checkpoint: definition, adapter or runtime identity changed')
            if run['state'] == 'running':
                # Only reachable after a crash: the exclusive lock proves no live owner.
                self.state.finish_run(run_id, 'stopped', 'interrupted; found running without its lock')
            self.state.reopen_run(run_id, decision=decision)
            return self._execute(run_id)

    def _exclusive(self):
        # One runner per campaign; a second one is refused (LeaseBusy), never queued.
        locks = FileLocks(_safe(self.archive.root, self.archive.root / 'capture-locks'))
        return locks.held(['campaign:' + self.definition.campaign_id])

    def _execute(self, run_id):
        try:
            self.archive.require_ready()
            for unit in self.definition.units:
                for stage in STAGES:
                    self._stage(run_id, unit, stage)
                    self.fault('after_stage:' + unit.unit_id + ':' + stage)
        except CapacityStop as exc:
            self.state.finish_run(run_id, 'stopped', 'budget_exhausted: ' + str(exc))
            return self.report(run_id)
        except BaseException as exc:
            # Leave pending jobs for an explicit resume; never silently restart work.
            try:
                if self.state.campaign_run(run_id)['state'] == 'running':
                    self.state.finish_run(run_id, 'stopped', 'interrupted: ' + type(exc).__name__)
            except (StateError, OSError):
                pass
            raise
        self.state.finish_run(run_id, 'complete')
        return self.report(run_id)

    def _stage(self, run_id, unit, stage):
        recipe = self.recipe(unit, stage)
        identifier = job_id(unit, stage, recipe)
        job = self.state.plan_job(identifier, campaign_id=self.definition.campaign_id, unit_id=unit.unit_id,
                                  unit_revision=unit.revision, stage=stage, recipe=recipe['id'], run_id=run_id)
        expected = result_id(unit, stage, recipe)
        if job['state'] == 'complete':
            self._check_result(self.archive.verified_receipt(job['result_id'])['wrapper'], unit, stage, recipe)
            self.state.complete_job(identifier, run_id=run_id, result_id=job['result_id'], outcome=job['outcome'], action='reused')
            return
        # Exact growth since the campaign baseline, including interrupted attempts.
        self._spent = before = self._growth()
        if self._published(expected):
            # Published before a checkpoint crash: finish the same job, no new work.
            existing = self.archive.verified_receipt(expected)['wrapper']
            self._check_result(existing, unit, stage, recipe)
            self.state.complete_job(identifier, run_id=run_id, result_id=expected,
                                    outcome=existing['payload']['outcome'], action='recovered')
            return
        payload = getattr(self, '_' + stage)(unit, recipe)
        self._capacity(_receipt_cost(payload))
        self.archive.receipt('campaign-' + stage, payload, expected)
        self.fault('after_result:' + unit.unit_id + ':' + stage)
        self.state.complete_job(identifier, run_id=run_id, result_id=expected, outcome=payload['outcome'],
                                action='executed', new_bytes=self._growth() - before)

    def _published(self, receipt_id):
        """Index point lookup; a damaged dependency is an error, never 'absent'."""
        self.archive.require_ready()
        return self.archive.db.execute('SELECT 1 FROM receipts WHERE id=?', (receipt_id,)).fetchone() is not None

    def _check_result(self, wrapper, unit, stage, recipe):
        p = wrapper['payload']
        upstream = {'validate': ('registration_id', 'register'), 'bundle': ('validation_id', 'validate')}.get(stage)
        if (wrapper['kind'] != 'campaign-' + stage or p.get('unit_revision') != unit.revision
                or p.get('recipe_id') != recipe['id'] or p.get('outcome') is None
                or (upstream and p.get(upstream[0]) != result_id(unit, upstream[1], self.recipe(unit, upstream[1])))):
            raise StateError('checkpoint result does not prove this job')

    # -- stages ----------------------------------------------------------------------
    def _register(self, unit, recipe):
        """Register every pinned dependency in place.

        A missing or changed pinned input stops the run with the job still pending;
        it is an environment fault to restore, not a checkpointed source outcome.
        """
        inputs = []
        for path, sha in unit.files:
            self._capacity(_receipt_cost(dict(sha256=sha, path=path, size=0)))
            try:
                registered = self.archive.register_file(self.archive.workspace / path, expected_sha256=sha)
            except (FileNotFoundError, ValueError) as exc:
                raise StateError('pinned saved input missing or changed: ' + path) from exc
            inputs.append(dict(role='saved-input', path=path, sha256=registered))
        return dict(unit_id=unit.unit_id, unit_revision=unit.revision, recipe_id=recipe['id'], recipe=recipe,
                    outcome='observed', inputs=inputs,
                    saved_context=dict(acquisition='saved', request_context_known=False,
                                       capture_time='unknown', collector_version='unknown'))

    def _validate(self, unit, recipe):
        registration_id = result_id(unit, 'register', self.recipe(unit, 'register'))
        reg = self.archive.verified_receipt(registration_id)['wrapper']['payload']
        base = dict(unit_id=unit.unit_id, unit_revision=unit.revision, recipe_id=recipe['id'], recipe=recipe,
                    registration_id=registration_id)
        manifest = self.archive.workspace / unit.manifest
        # The validator reads the exact registered bytes from their pinned locations.
        for item in reg['inputs']:
            if file_digest(self.archive.workspace / item['path']) != item['sha256']:
                raise StateError('registered saved input changed: ' + item['path'])
        adapter = ADAPTERS[unit.source]
        try:
            output = adapter.records(manifest, unit.manifest)
        except INPUT_ERRORS as exc:
            output = dict(outcome='failed', error=dict(type=type(exc).__name__, message=str(exc)), occurrences=[])
        if output['outcome'] == 'explicit_zero' and not output.get('zero_proof'):
            output = dict(output, outcome='failed', error=dict(type='ValueError', message='explicit zero without saved proof'))
        output = dict(output, unit_id=unit.unit_id, unit_revision=unit.revision, schema_version=1)
        for occurrence in output['occurrences']:
            occurrence['occurrence_id'] = 'occ-' + _sha([unit.revision, occurrence['logical_sha256'],
                                                         occurrence['kind'], occurrence['selector']])
        sha = self._store(canonical(output))
        return dict(base, outcome=output['outcome'], inputs=[dict(role='saved-input', path=i['path'], sha256=i['sha256'])
                                                             for i in reg['inputs']],
                    outputs=[dict(role='validated-records', sha256=sha)], error=output.get('error'))

    def _bundle(self, unit, recipe):
        validation_id = result_id(unit, 'validate', self.recipe(unit, 'validate'))
        validation = self.archive.verified_receipt(validation_id)['wrapper']['payload']
        base = dict(unit_id=unit.unit_id, source=unit.source, unit_revision=unit.revision, recipe_id=recipe['id'],
                    recipe=recipe, validation_id=validation_id, scope=unit.scope)
        output = json.loads(self.archive.resolve(validation['outputs'][0]['sha256']).read_bytes())
        complete = output['outcome'] in ('validated_nonempty', 'explicit_zero')
        # Successful observation only; failed saved inputs are accounted for, not observed coverage.
        coverage = dict(declared=True, inputs_registered=True, observed=complete, validated_complete=complete,
                        explicit_zero=output['outcome'] == 'explicit_zero', query_records=output.get('query_records'),
                        upstream_total=None, upstream_total_known=False)
        comparison = (ADAPTERS[unit.source].compare(output, unit.expectations)
                      if complete else dict(status='failed', mismatches=[]))
        candidate = dict(schema_version=1, unit_id=unit.unit_id, unit_revision=unit.revision,
                         outcome=output['outcome'], scope=output.get('scope', unit.scope),
                         zero_proof=output.get('zero_proof'), coverage=coverage,
                         occurrence_ids=[o['occurrence_id'] for o in output['occurrences']],
                         occurrences_sha256=_sha([o['logical_sha256'] for o in output['occurrences']]),
                         record_sets=output.get('record_sets', {}), error=output.get('error'),
                         review='candidate only; not accepted catalog evidence')
        sha = self._store(canonical(candidate))
        return dict(base, outcome=output['outcome'], coverage=coverage, comparison=comparison,
                    occurrences=len(output['occurrences']), candidate_sha256=sha,
                    outputs=[dict(role='candidate-bundle', sha256=sha)])

    # -- capacity --------------------------------------------------------------------
    def _store(self, data):
        self._capacity(len(data) + _receipt_cost(dict(sha256='0' * 64, size=len(data), path='objects/00/' + '0' * 64)))
        return self.archive.store_bytes(data)

    def _capacity(self, projected):
        """Stop before a write could cross the cap or free-space floor.

        `_spent` is the exact measured growth at stage start plus conservative
        projections of writes made since, so interrupted attempts are charged.
        """
        budgets = self.definition.budgets
        if self.disk_usage(self.archive.root).free - projected < budgets['free_space_floor_bytes']:
            raise CapacityStop('free-space floor reached')
        if self._spent is None:
            self._spent = self._growth()
        if self._spent + projected > budgets['new_metadata_and_candidates_bytes_cap']:
            raise CapacityStop('campaign new-byte cap reached')
        self._spent += projected

    def _index_bytes(self):
        return self.archive.database.stat().st_size

    def _baseline(self, run_id=None):
        runs = self.state.campaign_runs(self.definition.campaign_id)
        if run_id is not None:
            runs = [r for r in runs if r['id'] == run_id]
        if not runs:
            return None
        first = min(runs, key=lambda r: r['start_sequence'])
        return first['start_sequence'], first['start_index_bytes']

    def _growth(self, run_id=None, detail=False):
        """Measured archive growth since the campaign (or one run) began.

        Receipt files plus archive-scope object bytes, read from the index table (no
        receipt-file scan), plus index file growth. Other concurrent publishers would
        be over-counted, which only makes the cap more conservative.
        """
        baseline = self._baseline(run_id)
        if baseline is None:
            return dict(receipts_and_objects=0, index=0, total=0) if detail else 0
        sequence, index_bytes = baseline
        rows = self.archive.db.execute('SELECT id,kind,payload FROM receipts WHERE sequence>?', (sequence,)).fetchall()
        files = 0
        for row in rows:
            files += self.archive._receipt_path(row['id']).stat().st_size
            if row['kind'] == 'artifact-registration':
                payload = json.loads(row['payload'])
                if payload['scope'] == 'archive':
                    files += payload['size']
        index = max(0, self._index_bytes() - index_bytes)
        result = dict(receipts_and_objects=files, index=index, total=files + index)
        return result if detail else result['total']

    # -- reporting -------------------------------------------------------------------
    def report(self, run_id=None):
        """Read-only. Accounted-for work includes failures; coverage counts successes only."""
        runs = self.state.campaign_runs(self.definition.campaign_id)
        if run_id is None:
            if not runs:
                raise StateError('campaign has no runs')
            run_id = max(runs, key=lambda r: (r['start_sequence'], r['start_wall'], r['id']))['id']
        run = self.state.campaign_run(run_id)
        executions = self.state.campaign_executions(run_id=run_id)
        jobs = {row['id']: row for row in self.state.campaign_jobs(self.definition.campaign_id)}
        previous = sorted((r for r in runs if (r['start_sequence'], r['start_wall'], r['id'])
                           < (run['start_sequence'], run['start_wall'], run['id'])),
                          key=lambda r: (r['start_sequence'], r['start_wall'], r['id']))
        prior_bundles = {}
        if previous:
            for e in self.state.campaign_executions(run_id=previous[-1]['id']):
                job = jobs.get(e['job_id'])
                if job and job['stage'] == 'bundle':
                    prior_bundles[job['unit_id']] = e['result_id']
        units, counts = [], dict(declared=0, inputs_registered=0, observed=0, validated_complete=0, explicit_zero=0,
                                 failed=0, blocked=0, unknown=0, pending=0, budget_exhausted=0)
        for unit in self.definition.units:
            counts['declared'] += 1
            bundle_job = jobs.get(job_id(unit, 'bundle', self.recipe(unit, 'bundle')))
            entry = dict(unit_id=unit.unit_id, source=unit.source, unit_revision=unit.revision)
            if not bundle_job or bundle_job['state'] != 'complete':
                status = 'budget_exhausted' if run['reason'] and run['reason'].startswith('budget_exhausted') else 'pending'
                counts[status] += 1
                units.append(dict(entry, outcome=status))
                continue
            p = self.archive.verified_receipt(bundle_job['result_id'])['wrapper']['payload']
            coverage = p['coverage']
            for key in ('inputs_registered', 'observed', 'validated_complete', 'explicit_zero'):
                counts[key] += coverage[key]
            if p['outcome'] in ('failed', 'blocked', 'unknown'):
                counts[p['outcome']] += 1
            prior = prior_bundles.get(unit.unit_id)
            if prior is None:
                change = 'no prior run'
            else:
                old = self.archive.verified_receipt(prior)['wrapper']['payload']
                change = 'unchanged' if old.get('candidate_sha256') == p.get('candidate_sha256') else 'changed'
            units.append(dict(entry, outcome=p['outcome'], occurrences=p.get('occurrences', 0),
                              query_records=coverage['query_records'], upstream_total='unknown',
                              pinned_comparison=(p['comparison'] or {}).get('status'),
                              mismatches=(p['comparison'] or {}).get('mismatches', []),
                              change_from_previous_run=change, candidate_sha256=p.get('candidate_sha256')))
        actions = {}
        for e in executions:
            actions[e['action']] = actions.get(e['action'], 0) + 1
        growth = self._growth(run_id, detail=True)
        return dict(campaign_id=self.definition.campaign_id, run_id=run_id, run_state=run['state'], reason=run['reason'],
                    previous_run=previous[-1]['id'] if previous else None, network_requests=0,
                    units_accounted_for=counts, job_actions=actions,
                    new_bytes=growth['total'], physical_growth_since_run_start=growth,
                    campaign_growth_bytes=self._growth(),
                    cap_bytes=self.definition.budgets['new_metadata_and_candidates_bytes_cap'],
                    coverage_note='finite saved query snapshots; not regulator-wide coverage or a change feed; '
                                  'failed/blocked/unknown units are accounted for, not observed coverage',
                    units=units)


def _receipt_cost(payload):
    """Conservative bytes for one receipt: wrapper file, index wrapper/payload copies
    and one possible new index page."""
    return 3 * (len(canonical(payload)) + 512) + 4096


def open_runner(workspace, archive_root, definition_path=DEFAULT_DEFINITION, inventory_path=DEFAULT_INVENTORY, readonly=False):
    workspace = Path(workspace).absolute()
    archive = Archive(archive_root, workspace, readonly=readonly)
    state = CampaignState(archive.root, readonly=readonly)
    return Runner(archive, state, load_definition(workspace, definition_path, inventory_path))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, default=Path.cwd())
    parser.add_argument('--archive', type=Path, default=Path('data/archive'))
    parser.add_argument('--definition', default=DEFAULT_DEFINITION)
    parser.add_argument('--inventory', default=DEFAULT_INVENTORY)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('plan')
    commands.add_parser('status')
    run = commands.add_parser('run')
    run.add_argument('--offline', action='store_true', required=True)
    resume = commands.add_parser('resume')
    resume.add_argument('run_id')
    resume.add_argument('--decision', required=True)
    report = commands.add_parser('report')
    report.add_argument('run_id', nargs='?')
    args = parser.parse_args(argv)
    readonly = args.command in ('plan', 'status', 'report')
    runner = open_runner(args.workspace, args.archive, args.definition, args.inventory, readonly=readonly)
    try:
        if args.command == 'plan':
            result = runner.plan()
        elif args.command == 'status':
            result = dict(runs=[{k: r[k] for k in ('id', 'state', 'reason', 'start_wall', 'end_wall')}
                                for r in runner.state.campaign_runs(runner.definition.campaign_id)],
                          plan=runner.plan())
        elif args.command == 'run':
            result = runner.run()
        elif args.command == 'resume':
            result = runner.resume(args.run_id, decision=args.decision)
        else:
            result = runner.report(args.run_id)
        json.dump(result, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write('\n')
        return 0 if result.get('run_state', 'complete') != 'stopped' else 2
    finally:
        runner.archive.close()


if __name__ == '__main__':
    raise SystemExit(main())

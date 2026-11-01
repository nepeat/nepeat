"""C3b gates: review queue, selected release and rebuild gate on isolated fixtures.

Real saved units are only read; the campaign run, archive, catalogs, queue and release
all live in a temporary workspace. Synthetic changes never touch live evidence.
"""
from contextlib import closing
import json
from pathlib import Path
import shutil
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from devicehunt import coverage, review_queue as rq
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256, rebuild
from tests.test_campaign import INVENTORY, ROOT, UNITS, build_workspace, fresh, rewrite, runner_for

AMAZON = 'ised-rel-25869'
RRA = 'rra-official-query-5827f2fe7092'
CSV = 'data/raw/ised-amazon-export/results.csv'


def amend_timestamp(workspace):
    rewrite(workspace, CSV, lambda b: b.replace(b'2022-11-21T06:30:14-05:00', b'2022-11-21T06:30:15-05:00'))


@unittest.skipUnless(INVENTORY.exists(), 'pinned c0 inventory unavailable')
class ReviewQueueTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.workspace = Path(self.tmp.name)
        for entry in ('socket.socket.connect', 'socket.create_connection', 'socket.getaddrinfo',
                      'urllib.request.OpenerDirector.open', 'subprocess.run', 'subprocess.Popen'):
            guard = patch(entry, side_effect=AssertionError('forbidden network/external entrypoint'))
            guard.start()
            self.addCleanup(guard.stop)
        self.units = {u['unit_id']: u for u in json.loads(INVENTORY.read_text())['units'] if u['unit_id'] in UNITS}

    def tearDown(self):
        self.tmp.cleanup()

    def accepted_catalog(self, skip=()):
        """The 'live' baseline: unchanged real units imported read-only from the workspace root."""
        path = self.workspace / 'data' / 'devices.sqlite'
        c = Catalog(path)
        for unit in (u for k, u in self.units.items() if k not in skip):
            import_source(c, unit['source'], ROOT / unit['source_manifest'])
        c.db.commit()
        c.db.close()
        # The accepted import manifest hashes the original (unmutated) unit files.
        accepted = dict(schema_version=1, imports=[dict(source=u['source'], path=u['source_manifest'][len('data/'):])
                                                   for k, u in self.units.items() if k not in skip],
                        files={p[len('data/'):]: file_sha256(ROOT / p) for u in self.units.values()
                               for p in u['dependency_paths']})
        (self.workspace / 'data' / 'accepted-manifest.json').write_text(json.dumps(accepted))
        return path

    def campaign(self, mutate=None, skip=()):
        build_workspace(self.workspace, mutate=mutate)
        fresh(self.workspace)[0].close()
        runner = runner_for(self.workspace)
        runner.run('run-1')
        manifest = coverage.build(runner, 'run-1')
        path = coverage.publish(manifest, self.workspace / 'coverage', workspace=self.workspace,
                                archive_root=runner.archive.root)
        runner.archive.close()
        self.coverage = path
        catalog = self.accepted_catalog(skip)
        items = rq.build_queue(self.workspace, catalog, path,
                               dict(definition='definition.json', inventory='inventory.json'), self.workspace / 'queue',
                               self.workspace / 'data' / 'accepted-manifest.json')
        return {i['unit_id']: i for i in items}

    def item_path(self, item):
        return self.workspace / 'queue' / 'items' / (item['item_id'] + '.json')

    def release(self):
        return rq.build_release(self.workspace / 'queue', self.workspace / 'data' / 'campaign-releases', self.coverage)

    def manifest(self, release, skip=()):
        """A gated import manifest over the workspace's current unit files."""
        data = self.workspace / 'data'
        files = {p[len('data/'):]: file_sha256(self.workspace / p) for u in self.units.values() for p in u['dependency_paths']}
        name = 'campaign-releases/' + release['release_id'] + '.json'
        files[name] = file_sha256(data / name)
        covered = data / 'coverage' / self.coverage.name
        covered.parent.mkdir(exist_ok=True)
        if not covered.exists():
            shutil.copy2(self.coverage, covered)
        files['coverage/' + covered.name] = file_sha256(covered)
        with closing(sqlite3.connect(data / 'devices.sqlite')) as db:
            expected = dict(devices=db.execute('SELECT count(*) FROM devices').fetchone()[0],
                            records=db.execute('SELECT count(*) FROM records').fetchone()[0],
                            sources=dict(db.execute('SELECT id,records_seen FROM sources')),
                            rra_equipment=db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],
                            ised_equipment=db.execute('SELECT count(*) FROM ised_equipment').fetchone()[0])
        manifest = dict(schema_version=1, files=files, expected=expected,
                        campaign_gate=dict(release=name, coverage_manifest='coverage/' + covered.name),
                        imports=[dict(source=u['source'], path=u['source_manifest'][len('data/'):])
                                 for k, u in sorted(self.units.items()) if k not in skip])
        path = data / 'import-manifest.json'
        path.write_text(json.dumps(manifest))
        return path, manifest

    def test_unchanged_real_units_release_and_gate_a_rebuild_without_the_archive(self):
        items = self.campaign()
        self.assertEqual(set(items), set(UNITS))
        self.assertTrue(all(i['no_changes'] and i['reviewable'] for i in items.values()))
        self.assertEqual(items['ised-rel-query-celestica']['candidate']['records'], 0)  # explicit zero, empty projection
        repeated = [c for c in items[AMAZON]['conflicts'] if c['kind'] == 'repeated_certificate_distinct_devices']
        self.assertEqual([c['certificate'] for c in repeated], ['25869-TE1A001'])
        self.assertEqual(len(repeated[0]['device_ids']), 3)
        # Items are immutable, canonical and content-addressed.
        self.assertEqual(rq.load(self.item_path(items[AMAZON])), items[AMAZON])
        again = rq.build_queue(self.workspace, self.workspace / 'data/devices.sqlite',
                               next((self.workspace / 'coverage').glob('*.json')),
                               dict(definition='definition.json', inventory='inventory.json'), self.workspace / 'queue',
                               self.workspace / 'data' / 'accepted-manifest.json')
        self.assertEqual({i['unit_id']: i for i in again}, items)  # deterministic, republishing identical bytes
        rq.decide(self.item_path(items[AMAZON]), 'accepted', 'fixture reviewer', 'no changes', self.workspace / 'queue')
        with self.assertRaisesRegex(rq.ReviewError, 'already decided'):
            rq.decide(self.item_path(items[AMAZON]), 'rejected', 'fixture reviewer', 'second thoughts', self.workspace / 'queue')
        release = self.release()
        statuses = {e['unit_id']: (e['status'], e['reason']) for e in release['entries']}
        self.assertEqual(statuses[AMAZON], ('accepted', None))
        self.assertEqual(statuses[RRA], ('baseline', 'unreviewed'))
        path, manifest = self.manifest(release)
        # The mutable registry is out of reach: the release is a plain hashed file.
        shutil.move(self.workspace / 'archive', self.workspace / 'archive-away')
        shutil.move(self.workspace / 'queue', self.workspace / 'queue-away')
        result = rebuild(path, self.workspace / 'rebuilt.sqlite')
        self.assertEqual(result['records'], manifest['expected']['records'])
        # A gated job missing from the manifest is refused before any import.
        manifest['imports'] = [j for j in manifest['imports'] if not j['path'].startswith('raw/ised-amazon-export/')]
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(rq.ReviewError, 'exactly the released job'):
            rebuild(path, self.workspace / 'rebuilt-2.sqlite')
        self.assertFalse((self.workspace / 'rebuilt-2.sqlite').exists())

    def test_amended_candidate_needs_acceptance_before_its_inputs_can_be_imported(self):
        items = self.campaign(mutate=amend_timestamp)
        item = items[AMAZON]
        self.assertFalse(item['no_changes'])
        changed = item['changes']['records_changed']
        self.assertEqual(len(changed), 1)
        self.assertEqual(changed[0]['from_key'], changed[0]['to_key'])
        self.assertIn('facts', changed[0]['fields'])
        self.assertEqual((len(item['changes']['raw_rows_added']), len(item['changes']['raw_rows_removed'])), (1, 1))
        self.assertTrue(all(i['no_changes'] for u, i in items.items() if u != AMAZON))
        # Unreviewed: the release keeps the baseline, so the amended inputs are refused.
        release = self.release()
        path, _ = self.manifest(release)
        with self.assertRaisesRegex(rq.ReviewError, 'input differs from the reviewed release'):
            rebuild(path, self.workspace / 'refused.sqlite')
        # Missing item: a release can never silently drop a unit from the gate.
        hidden = self.workspace / 'hidden.json'
        shutil.move(self.item_path(item), hidden)
        with self.assertRaisesRegex(rq.ReviewError, 'exactly one item per coverage unit'):
            self.release()
        shutil.move(hidden, self.item_path(item))
        # Accepted: the same inputs now pass the gate and the projection matches.
        rq.decide(self.item_path(item), 'accepted', 'fixture reviewer', 'timestamp correction', self.workspace / 'queue')
        release = self.release()
        path, manifest = self.manifest(release)
        manifest['expected'] = json.loads(path.read_text())['expected']
        rebuild(path, self.workspace / 'accepted.sqlite')
        with closing(sqlite3.connect(self.workspace / 'accepted.sqlite')) as db:
            self.assertEqual(db.execute("SELECT value FROM facts WHERE name='ised_approval_timestamp_raw' "
                                        "AND value LIKE '2022-11-21%'").fetchone()[0], '2022-11-21T06:30:15-05:00')

    def test_another_job_rewriting_a_gated_source_fails_the_projection_gate(self):
        items = self.campaign()
        release = self.release()
        path, manifest = self.manifest(release)
        # A curated job that redeclares a gated source replaces its records after the reviewed import.
        hijack = self.workspace / 'data' / 'hijack.json'
        hijack.write_text(json.dumps({'sources': [dict(id=RRA, url='https://example.org', tier='vendor', license='t',
                                                       coverage='t')], 'devices': []}))
        manifest['imports'].append(dict(source='curated', path='hijack.json'))
        manifest['files']['hijack.json'] = file_sha256(hijack)
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(rq.ReviewError, 'projection of ' + RRA):
            rebuild(path, self.workspace / 'hijacked.sqlite')
        self.assertFalse((self.workspace / 'hijacked.sqlite').exists())
        self.assertTrue(items[RRA]['no_changes'])

    def test_failed_and_rejected_units_keep_their_baseline(self):
        def break_rra(workspace):
            rewrite(workspace, 'data/raw/rra-edge-core-hyphen-official/before.csv', lambda b: b'x' + b)
        items = self.campaign(mutate=break_rra)
        failed = items[RRA]
        self.assertEqual((failed['outcome'], failed['reviewable'], failed['candidate']), ('failed', False, None))
        with self.assertRaisesRegex(rq.ReviewError, 'cannot be accepted'):
            rq.decide(self.item_path(failed), 'accepted', 'fixture reviewer', 'x', self.workspace / 'queue')
        rq.decide(self.item_path(items[AMAZON]), 'rejected', 'fixture reviewer', 'disputed', self.workspace / 'queue')
        release = self.release()
        entries = {e['unit_id']: e for e in release['entries']}
        self.assertEqual((entries[RRA]['status'], entries[RRA]['reason']), ('baseline', 'not reviewable: failed'))
        self.assertEqual(entries[RRA]['projection_sha256'], failed['baseline']['projection_sha256'])
        self.assertEqual((entries[AMAZON]['status'], entries[AMAZON]['reason']), ('baseline', 'rejected'))

    def test_rejected_candidate_inputs_are_refused_by_the_gate(self):
        items = self.campaign(mutate=amend_timestamp)
        rq.decide(self.item_path(items[AMAZON]), 'rejected', 'fixture reviewer', 'unverified amendment', self.workspace / 'queue')
        path, _ = self.manifest(self.release())
        with self.assertRaisesRegex(rq.ReviewError, 'input differs from the reviewed release'):
            rebuild(path, self.workspace / 'refused.sqlite')

    def test_new_unit_without_baseline_is_excluded_until_accepted(self):
        items = self.campaign(skip=(RRA,))
        self.assertIsNone(items[RRA]['baseline']['inputs'])
        self.assertIsNone(items[RRA]['baseline']['projection_sha256'])
        entries = {e['unit_id']: e for e in self.release()['entries']}
        self.assertEqual((entries[RRA]['status'], entries[RRA]['reason']), ('excluded', 'unreviewed'))
        release = self.release()
        path, manifest = self.manifest(release)  # imports every unit, including the excluded one
        with self.assertRaisesRegex(rq.ReviewError, 'excluded unit must not be imported'):
            rebuild(path, self.workspace / 'refused.sqlite')
        path, _ = self.manifest(release, skip=(RRA,))
        rebuild(path, self.workspace / 'without.sqlite')
        with closing(sqlite3.connect(self.workspace / 'without.sqlite')) as db:
            self.assertIsNone(db.execute('SELECT 1 FROM sources WHERE id=?', (RRA,)).fetchone())

    def test_gate_requires_the_matching_coverage_manifest(self):
        self.campaign()
        path, manifest = self.manifest(self.release())
        del manifest['campaign_gate']['coverage_manifest']
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(rq.ReviewError, 'name a release and its coverage manifest'):
            rebuild(path, self.workspace / 'refused.sqlite')

    def test_edited_documents_are_refused(self):
        items = self.campaign()
        path = self.item_path(items[AMAZON])
        edited = dict(items[AMAZON], no_changes=False)
        forged = self.workspace / 'forged.json'
        forged.write_bytes(rq.canonical(edited) + b'\n')
        with self.assertRaisesRegex(rq.ReviewError, 'item_id does not match'):
            rq.decide(forged, 'accepted', 'fixture reviewer', 'x', self.workspace / 'queue')
        forged.write_text(json.dumps(items[AMAZON], indent=1))
        with self.assertRaisesRegex(rq.ReviewError, 'not canonical'):
            rq.decide(forged, 'accepted', 'fixture reviewer', 'x', self.workspace / 'queue')
        with self.assertRaisesRegex(rq.ReviewError, 'named reviewer'):
            rq.decide(path, 'accepted', ' ', 'x', self.workspace / 'queue')
        bogus = rq._with_id(dict(schema=rq.DECISION_SCHEMA, item_id=items[AMAZON]['item_id'], unit_id=AMAZON,
                                 decision='maybe', reviewer='r', reason='x', accepted_projection_sha256=None),
                            'decision_id', 'review-decision-')
        rq._publish(bogus, self.workspace / 'queue' / 'decisions', bogus['decision_id'] + '.json')
        with self.assertRaisesRegex(rq.ReviewError, 'unknown review decision'):
            self.release()
        (self.workspace / 'queue' / 'decisions' / (bogus['decision_id'] + '.json')).unlink()
        release = self.release()
        with self.assertRaisesRegex(rq.ReviewError, 'release_id does not match'):
            rq.check_release(dict(release, claims='device absent'))
        with self.assertRaisesRegex(rq.ReviewError, 'claims were edited'):
            rq.check_release(rq._with_id(dict(release, claims='device absent'), 'release_id', 'campaign-release-'))


class ProjectionDiffTests(unittest.TestCase):
    """Synthetic additions, amendments, removals and reorders on scratch catalogs only."""
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def project(self, name, rows):
        c = Catalog(self.root / (name + '.sqlite'))
        c.source('s', 'https://example.org', 'regulatory', 't', 't')
        for key, model, cert, fact in rows:
            c.add('s', key, 'https://example.org', 'Oracle', model, model, '', {'model': model},
                  identifiers=[('ic_id', cert)], facts=[('date', fact)])
        c.db.commit()
        try:
            return rq.projection(c.db, 's', 'ised_equipment')
        finally:
            c.db.close()

    def test_content_matching_separates_added_removed_changed_moved_and_duplicates(self):
        before = self.project('before', [('1', 'A1', 'C-1', 'd1'), ('2', 'B1', 'C-2', 'd1'),
                                         ('3', 'C1', 'C-3', 'd1'), ('4', 'D1', 'C-4', 'd1'), ('5', 'D1', 'C-4', 'd1')])
        after = self.project('after', [('1', 'A1', 'C-1', 'd2'), ('2', 'C1', 'C-3', 'd1'),
                                       ('3', 'E1', 'C-5', 'd1'), ('4', 'D1', 'C-4', 'd1'), ('5', 'D1', 'C-4', 'd1')])
        changes = rq.diff(before, after)
        self.assertEqual([c['from_key'] for c in changes['records_changed']], ['1'])
        self.assertEqual(sorted(changes['records_changed'][0]['fields']), ['facts'])
        self.assertEqual([(m['from_key'], m['to_key']) for m in changes['records_moved']], [('3', '2')])
        self.assertEqual([r['source_key'] for r in changes['records_removed']], ['2'])
        self.assertEqual([r['source_key'] for r in changes['records_added']], ['3'])
        self.assertEqual(changes['records_unchanged'], 2)  # duplicate identities preserved, not collapsed
        self.assertFalse(rq.no_changes(changes))
        self.assertTrue(rq.no_changes(rq.diff(before, before)))

    def test_reorder_alone_is_moved_not_changed(self):
        before = self.project('before', [('1', 'A1', 'C-1', 'd'), ('2', 'B1', 'C-2', 'd')])
        after = self.project('after', [('1', 'B1', 'C-2', 'd'), ('2', 'A1', 'C-1', 'd')])
        changes = rq.diff(before, after)
        self.assertEqual(len(changes['records_moved']), 2)
        self.assertEqual((changes['records_changed'], changes['records_added'], changes['records_removed']), ([], [], []))

    def test_projection_canary_is_frozen(self):
        """The live campaign release pins projection hashes; any change to projection() must
        come with a new queue, decisions, release and manifest gate. Update this literal only then."""
        c = Catalog(self.root / 'canary.sqlite')
        c.source('s', 'https://example.org', 'regulatory', 't', 't')
        c.add('s', '1', 'https://example.org', 'Oracle', 'A1', 'A1', '', {'model': 'A1'},
              affiliations=[('Oracle America, Inc.', 'registry_manufacturer')], identifiers=[('ic_id', 'C-1')],
              facts=[('date', 'd1')])
        c.db.execute("INSERT INTO ised_equipment VALUES ('s',1,'1','Oracle','A1','','','','C-1','t',1,'[]','https://example.org')")
        c.db.commit()
        try:
            value = rq.projection_sha256(rq.projection(c.db, 's', 'ised_equipment'))
        finally:
            c.db.close()
        self.assertEqual(value, '63fe658f8e1614a71736f6051a6000f1390fa8b8d400f0d97c4c4e5f90fc0c9f')

    def test_lineage_statement_on_a_moved_key_is_flagged(self):
        after = self.project('after', [('1', 'B1', 'C-2', 'd'), ('2', 'A1', 'C-1', 'd')])
        statement = dict(statement_id='st', source_key='1', kind='fact', projection=dict(name='date', value='x'))
        self.assertEqual([c['kind'] for c in rq._lineage_conflicts([statement], after)], ['accepted_lineage_would_dangle'])
        statement['projection']['value'] = 'd'
        self.assertEqual(rq._lineage_conflicts([statement], after), [])


ACCEPTED_MANIFEST = ROOT / 'data' / 'import-manifest.json'


@unittest.skipUnless(ACCEPTED_MANIFEST.exists() and INVENTORY.exists(), 'accepted manifest unavailable')
class AcceptedManifestGateTests(unittest.TestCase):
    """Read-only promotion check: the accepted manifest gates every current campaign unit."""
    def test_accepted_manifest_declares_a_gate_over_every_campaign_unit(self):
        from devicehunt.campaign import load_definition, DEFAULT_DEFINITION, DEFAULT_INVENTORY
        data = json.loads(ACCEPTED_MANIFEST.read_text())
        self.assertIn('campaign_gate', data, 'accepted import manifest must declare campaign_gate')
        release = rq.gate_inputs(ACCEPTED_MANIFEST.parent, data)
        definition = load_definition(ROOT, DEFAULT_DEFINITION, DEFAULT_INVENTORY)
        self.assertEqual({e['unit_id'] for e in release['entries']}, {u.unit_id for u in definition.units})
        self.assertEqual(release['definition_sha256'], definition.sha256)


if __name__ == '__main__':
    unittest.main()



OLD_BUNDLE = 'raw/rra-old-bundle/manifest.json'


@unittest.skipUnless(INVENTORY.exists(), 'pinned c0 inventory unavailable')
class MovedBundleBaselineTests(unittest.TestCase):
    """A refreshed unit's definition points at a new bundle path; the baseline stays at the old one."""
    setUp = ReviewQueueTests.setUp
    tearDown = ReviewQueueTests.tearDown
    accepted_catalog = ReviewQueueTests.accepted_catalog
    campaign = ReviewQueueTests.campaign
    item_path = ReviewQueueTests.item_path
    release = ReviewQueueTests.release

    def new_job(self):
        return dict(source='rra', path=self.units[RRA]['source_manifest'][len('data/'):])

    def old_inputs(self):
        return [dict(path=OLD_BUNDLE, sha256=rq.digest(b'old manifest')),
                dict(path='raw/rra-old-bundle/results.csv', sha256=rq.digest(b'old csv'))]

    def moved_campaign(self, mutate=None, gated=True):
        """Queue built against an accepted manifest whose release maps RRA to the old bundle path."""
        self.campaign(mutate=mutate)  # baseline catalog, coverage and path-matched accepted manifest
        first = self.release()
        shutil.rmtree(self.workspace / 'queue')
        entries = [dict(e, job=dict(source='rra', path=OLD_BUNDLE), inputs=self.old_inputs()) if e['unit_id'] == RRA else e
                   for e in first['entries']]
        frozen = rq._with_id(dict({k: v for k, v in first.items() if k != 'release_id'}, entries=entries),
                             'release_id', 'campaign-release-')
        frozen_path = rq._publish(rq.check_release(frozen), self.workspace / 'data' / 'old-releases',
                                  frozen['release_id'] + '.json')
        self.frozen = frozen
        accepted = json.loads((self.workspace / 'data' / 'accepted-manifest.json').read_text())
        new = self.new_job()
        accepted['imports'] = [dict(source='rra', path=OLD_BUNDLE) if (j['source'], j['path']) == (new['source'], new['path']) else j
                               for j in accepted['imports']]
        accepted['files'].update({i['path']: i['sha256'] for i in self.old_inputs()})
        name = 'old-releases/' + frozen_path.name
        accepted['files'][name] = file_sha256(frozen_path)
        covered = self.workspace / 'data' / 'coverage' / self.coverage.name
        covered.parent.mkdir(exist_ok=True)
        if not covered.exists():
            shutil.copy2(self.coverage, covered)
        accepted['files']['coverage/' + covered.name] = file_sha256(covered)
        if gated:
            accepted['campaign_gate'] = dict(release=name, coverage_manifest='coverage/' + covered.name)
        path = self.workspace / 'data' / 'accepted-gated.json'
        path.write_text(json.dumps(accepted))
        self.accepted_path, self.accepted = path, accepted
        return self.queue_items(path)

    def queue_items(self, accepted_path):
        items = rq.build_queue(self.workspace, self.workspace / 'data' / 'devices.sqlite', self.coverage,
                               dict(definition='definition.json', inventory='inventory.json'), self.workspace / 'queue',
                               accepted_path)
        return {i['unit_id']: i for i in items}

    def gate_data(self, release, rra_paths, files=None):
        data = self.workspace / 'data'
        files = dict(files or {})
        files.update({p[len('data/'):]: file_sha256(self.workspace / p) for u in self.units.values() for p in u['dependency_paths']})
        files.update({i['path']: i['sha256'] for e in release['entries'] for i in (e['inputs'] or [])})
        name = 'campaign-releases/' + release['release_id'] + '.json'
        files[name] = file_sha256(data / name)
        covered = data / 'coverage' / self.coverage.name
        covered.parent.mkdir(exist_ok=True)
        if not covered.exists():
            shutil.copy2(self.coverage, covered)
        files['coverage/' + covered.name] = file_sha256(covered)
        imports = [dict(source=u['source'], path=u['source_manifest'][len('data/'):]) for k, u in sorted(self.units.items()) if k != RRA]
        imports += [dict(source='rra', path=p) for p in rra_paths]
        return dict(schema_version=1, files=files, imports=imports,
                    campaign_gate=dict(release=name, coverage_manifest='coverage/' + covered.name))

    def entry(self, release):
        return next(e for e in release['entries'] if e['unit_id'] == RRA)

    def test_unreviewed_unit_keeps_the_old_job_and_inputs(self):
        items = self.moved_campaign()
        new = self.new_job()
        item = items[RRA]
        self.assertEqual(item['job'], new)
        self.assertEqual(item['baseline']['job'], dict(source='rra', path=OLD_BUNDLE))
        self.assertEqual(item['baseline']['inputs'], self.old_inputs())
        self.assertIsNotNone(item['baseline']['projection_sha256'])
        release = self.release()
        entry = self.entry(release)
        self.assertEqual((entry['status'], entry['reason']), ('baseline', 'unreviewed'))
        self.assertEqual((entry['job'], entry['inputs']), (dict(source='rra', path=OLD_BUNDLE), self.old_inputs()))
        self.assertEqual(entry['candidate_job'], new)
        self.assertTrue(all('candidate_job' not in e for e in release['entries'] if e['unit_id'] != RRA))
        other = items[AMAZON]
        self.assertEqual(other['baseline']['job'], other['job'])  # unmoved units are unaffected by the gate
        rq.check_release(release)

    def test_accepted_unit_takes_the_new_job_and_rejected_keeps_the_old(self):
        items = self.moved_campaign()
        rq.decide(self.item_path(items[RRA]), 'accepted', 'fixture reviewer', 'refresh', self.workspace / 'queue')
        entry = self.entry(self.release())
        self.assertEqual((entry['status'], entry['reason']), ('accepted', None))
        self.assertEqual((entry['job'], entry['inputs']), (self.new_job(), items[RRA]['inputs']))
        self.assertNotIn('candidate_job', entry)

    def test_rejected_unit_keeps_the_old_job(self):
        items = self.moved_campaign()
        rq.decide(self.item_path(items[RRA]), 'rejected', 'fixture reviewer', 'disputed', self.workspace / 'queue')
        entry = self.entry(self.release())
        self.assertEqual((entry['status'], entry['reason']), ('baseline', 'rejected'))
        self.assertEqual((entry['job'], entry['candidate_job']), (dict(source='rra', path=OLD_BUNDLE), self.new_job()))

    def test_gate_imports_old_only_for_a_baseline_unit(self):
        self.moved_campaign()
        release = self.release()
        new_path = self.new_job()['path']
        root = self.workspace / 'data'
        self.assertIs(rq.gate_inputs(root, self.gate_data(release, [OLD_BUNDLE])) is not None, True)
        with self.assertRaisesRegex(rq.ReviewError, 'exactly the released job'):
            rq.gate_inputs(root, self.gate_data(release, [new_path]))
        with self.assertRaisesRegex(rq.ReviewError, 'unaccepted candidate job must not be imported'):
            rq.gate_inputs(root, self.gate_data(release, [OLD_BUNDLE, new_path]))

    def test_gate_for_an_accepted_unit_wants_the_new_job(self):
        items = self.moved_campaign()
        rq.decide(self.item_path(items[RRA]), 'accepted', 'fixture reviewer', 'refresh', self.workspace / 'queue')
        release = self.release()
        root = self.workspace / 'data'
        rq.gate_inputs(root, self.gate_data(release, [self.new_job()['path']]))
        with self.assertRaisesRegex(rq.ReviewError, 'exactly the released job'):
            rq.gate_inputs(root, self.gate_data(release, [OLD_BUNDLE]))

    def test_manifest_without_a_gate_falls_back_to_the_path_match(self):
        items = self.campaign()
        self.assertTrue(all(i['baseline']['job'] == i['job'] for i in items.values()))
        entries = {e['unit_id']: e for e in self.release()['entries']}
        self.assertTrue(all(e['status'] == 'baseline' and 'candidate_job' not in e for e in entries.values()))
        # Same moved manifest with its gate removed: the old path no longer matches, as before.
        shutil.rmtree(self.workspace / 'queue')
        accepted = json.loads((self.workspace / 'data' / 'accepted-manifest.json').read_text())
        accepted['imports'] = [dict(source='rra', path=OLD_BUNDLE) if j['path'] == self.new_job()['path'] else j
                               for j in accepted['imports']]
        path = self.workspace / 'data' / 'accepted-ungated.json'
        path.write_text(json.dumps(accepted))
        moved = self.queue_items(path)
        self.assertEqual((moved[RRA]['baseline']['job'], moved[RRA]['baseline']['inputs']), (None, None))
        self.assertEqual(moved[AMAZON]['baseline']['job'], moved[AMAZON]['job'])
        self.assertEqual({e['unit_id']: e['status'] for e in self.release()['entries']}[RRA], 'excluded')

    def test_excluded_release_entry_stays_excluded_and_absent_unit_uses_path_match(self):
        self.moved_campaign()
        shutil.rmtree(self.workspace / 'queue')
        new = self.new_job()
        base = {k: v for k, v in self.frozen.items() if k != 'release_id'}

        def variant(entries, name, coverage_units=None, swap=True):
            release = dict(base, entries=entries)
            accepted = json.loads(json.dumps(self.accepted))
            if coverage_units is not None:  # an older run that did not yet cover every current unit
                old = coverage.load(self.coverage)
                old['units'] = [u for u in old['units'] if u['unit_id'] in coverage_units]
                old['manifest_id'] = coverage.identity(old)
                old_path = self.workspace / 'data' / 'coverage' / (old['manifest_id'] + '.json')
                old_path.write_bytes(rq.canonical(old) + b'\n')
                release['coverage_manifest_id'] = old['manifest_id']
                accepted['campaign_gate']['coverage_manifest'] = 'coverage/' + old_path.name
                accepted['files']['coverage/' + old_path.name] = file_sha256(old_path)
            frozen = rq._with_id(release, 'release_id', 'campaign-release-')
            path = rq._publish(rq.check_release(frozen), self.workspace / 'data' / 'old-releases', frozen['release_id'] + '.json')
            accepted['campaign_gate']['release'] = 'old-releases/' + path.name
            accepted['files']['old-releases/' + path.name] = file_sha256(path)
            # the manifest imports the unit's NEW path, which a path match would find
            if swap:
                accepted['imports'] = [dict(source='rra', path=new['path']) if j['path'] == OLD_BUNDLE else j
                                       for j in accepted['imports']]
            target = self.workspace / 'data' / name
            target.write_text(json.dumps(accepted))
            return target

        # excluded in the accepted release: no path-match fall-through even though its candidate path is imported
        excluded = [dict(e, status='excluded', reason='unreviewed', inputs=None, projection_sha256=None, decision_id=None)
                    if e['unit_id'] == RRA else e for e in self.frozen['entries']]
        items = self.queue_items(variant(excluded, 'accepted-excluded.json'))
        self.assertEqual((items[RRA]['baseline']['job'], items[RRA]['baseline']['inputs']), (None, None))
        self.assertEqual(items[AMAZON]['baseline']['job'], items[AMAZON]['job'])
        self.assertEqual({e['unit_id']: e['status'] for e in self.release()['entries']}[RRA], 'excluded')
        shutil.rmtree(self.workspace / 'queue')
        # absent from the accepted release (older coverage did not include the unit): path match applies
        keep = {u for u in self.units if u != AMAZON}
        items = self.queue_items(variant([e for e in self.frozen['entries'] if e['unit_id'] != AMAZON],
                                         'accepted-absent.json', keep, swap=False))
        self.assertEqual(items[AMAZON]['baseline']['job'], items[AMAZON]['job'])
        self.assertIsNotNone(items[AMAZON]['baseline']['inputs'])
        self.assertEqual(items[RRA]['baseline']['job'], dict(source='rra', path=OLD_BUNDLE))

    def test_release_with_two_entries_sharing_a_job_is_refused(self):
        self.moved_campaign()
        twin_of = next(e for e in self.frozen['entries'] if e['adapter'] == 'rra' and e['unit_id'] != RRA)
        entries = [dict(e, job=twin_of['job'], inputs=twin_of['inputs']) if e['unit_id'] == RRA else e
                   for e in self.frozen['entries']]
        twin = rq._with_id(dict({k: v for k, v in self.frozen.items() if k != 'release_id'}, entries=entries),
                           'release_id', 'campaign-release-')
        with self.assertRaisesRegex(rq.ReviewError, 'share one job'):
            rq.check_release(twin)

    def test_tampered_accepted_release_is_refused(self):
        self.moved_campaign()
        shutil.rmtree(self.workspace / 'queue')
        good = dict(self.accepted)
        name = good['campaign_gate']['release']
        path = self.workspace / 'data' / name
        # A different (even valid) release than the hashed one.
        other = rq._with_id(dict({k: v for k, v in self.frozen.items() if k != 'release_id'}, entries=[
            dict(e, reason='x') if e['status'] == 'baseline' else e for e in self.frozen['entries']]), 'release_id', 'campaign-release-')
        original = path.read_bytes()
        path.chmod(0o644)
        path.write_bytes(rq.canonical(other) + b'\n')
        with self.assertRaisesRegex(rq.ReviewError, 'differs from its manifest hash'):
            self.queue_items(self.accepted_path)
        path.write_bytes(original)
        # An entry input whose hash differs from the accepted manifest's files.
        tampered = json.loads(json.dumps(good))
        tampered['files'][OLD_BUNDLE] = rq.digest(b'something else')
        bad = self.workspace / 'data' / 'accepted-tampered.json'
        bad.write_text(json.dumps(tampered))
        with self.assertRaisesRegex(rq.ReviewError, 'input differs from the reviewed release|differs from the accepted manifest'):
            self.queue_items(bad)
        # A release the manifest does not hash, and a job the manifest does not import.
        unhashed = json.loads(json.dumps(good))
        del unhashed['files'][name]
        bad.write_text(json.dumps(unhashed))
        with self.assertRaisesRegex(rq.ReviewError, 'does not hash its campaign release'):
            self.queue_items(bad)
        no_job = json.loads(json.dumps(good))
        no_job['imports'] = [j for j in no_job['imports'] if j['path'] != OLD_BUNDLE]
        bad.write_text(json.dumps(no_job))
        with self.assertRaisesRegex(rq.ReviewError, 'exactly the released job'):
            self.queue_items(bad)
        self.assertEqual(self.queue_items(self.accepted_path)[RRA]['baseline']['job']['path'], OLD_BUNDLE)

    def test_release_frozen_before_candidate_job_still_validates(self):
        self.moved_campaign()
        release = self.release()
        stripped = rq._with_id(dict({k: v for k, v in release.items() if k != 'release_id'}, entries=[
            {k: v for k, v in e.items() if k != 'candidate_job'} for e in release['entries']]),
            'release_id', 'campaign-release-')
        self.assertTrue(all('candidate_job' not in e for e in stripped['entries']))
        self.assertEqual(rq.check_release(stripped), stripped)
        path = rq._publish(stripped, self.workspace / 'data' / 'campaign-releases', stripped['release_id'] + '.json')
        self.assertTrue(path.exists())
        rq.gate_inputs(self.workspace / 'data', self.gate_data(stripped, [OLD_BUNDLE]))
        # Without the field the old behaviour stands (no double-import check).
        rq.gate_inputs(self.workspace / 'data', self.gate_data(stripped, [OLD_BUNDLE, self.new_job()['path']]))
        # The field is validated when present.
        bad = rq._with_id(dict({k: v for k, v in release.items() if k != 'release_id'}, entries=[
            dict(e, candidate_job=e['job']) if e['unit_id'] == RRA else e for e in release['entries']]),
            'release_id', 'campaign-release-')
        with self.assertRaisesRegex(rq.ReviewError, 'invalid candidate_job'):
            rq.check_release(bad)


class DuplicateOccurrenceTests(unittest.TestCase):
    def test_identical_identity_rows_are_flagged_not_collapsed(self):
        with tempfile.TemporaryDirectory() as tmp:
            c = Catalog(Path(tmp) / 'dup.sqlite')
            c.source('s', 'https://example.org', 'regulatory', 't', 't')
            for key, model, cert in (('1', 'EAP101N', '27052-EAP101N'), ('2', 'EAP101N', '27052-EAP101N'),
                                     ('3', 'OTHER', '27052-OTHER')):
                c.add('s', key, 'https://example.org', 'Edgecore', model, model, '', {'model': model},
                      identifiers=[('ic_id', cert)], facts=[('date', 'd')])
            c.db.commit()
            try:
                candidate = rq.projection(c.db, 's', 'ised_equipment')
            finally:
                c.db.close()
        conflicts = rq._duplicate_occurrence_conflicts(candidate)
        self.assertEqual(len(conflicts), 1)
        self.assertEqual(conflicts[0]['kind'], 'duplicate_occurrence')
        self.assertEqual((conflicts[0]['source_keys'], conflicts[0]['count']), (['1', '2'], 2))
        self.assertIn('not independent corroboration', conflicts[0]['note'])
        self.assertEqual(len(candidate['records']), 3)  # preserved as separate records
        self.assertEqual(rq._duplicate_occurrence_conflicts(None), [])


def _decide_worker(item_path, decision, output, ready, go, results):
    from devicehunt import review_queue as q
    ready.put(decision)
    go.wait(10)
    try:
        q.decide(item_path, decision, 'reviewer ' + decision, 'race fixture', output)
        results.put((decision, 'ok'))
    except q.ReviewError as exc:
        results.put((decision, str(exc)))


def _fixture_item(directory, name):
    item = rq._with_id(dict(schema=rq.ITEM_SCHEMA, unit_id=name, outcome='validated_nonempty', reviewable=True,
                            candidate=dict(projection_sha256='a' * 64)), 'item_id', 'review-item-')
    return item, rq._publish(item, Path(directory) / 'items', item['item_id'] + '.json')


class DecideRaceTests(unittest.TestCase):
    def test_concurrent_decisions_for_one_item_publish_exactly_one(self):
        import multiprocessing
        ctx = multiprocessing.get_context('spawn')
        with tempfile.TemporaryDirectory() as tmp:
            for round_ in range(3):
                item, path = _fixture_item(tmp, 'unit-%d' % round_)
                ready, results, go = ctx.Queue(), ctx.Queue(), ctx.Event()
                workers = [ctx.Process(target=_decide_worker, args=(str(path), d, tmp, ready, go, results))
                           for d in ('accepted', 'rejected')]
                for w in workers:
                    w.start()
                for _ in workers:
                    ready.get(timeout=30)
                go.set()
                outcomes = dict(results.get(timeout=30) for _ in workers)
                for w in workers:
                    w.join(30)
                    self.assertEqual(w.exitcode, 0)
                self.assertEqual(sorted(v == 'ok' for v in outcomes.values()), [False, True], outcomes)
                loser = next(v for v in outcomes.values() if v != 'ok')
                self.assertIn('item already decided', loser)
                files = [p for p in (Path(tmp) / 'decisions').glob('review-decision-*.json')
                         if rq.load(p)['item_id'] == item['item_id']]
                self.assertEqual(len(files), 1)

    def test_interrupted_claim_is_completed_by_the_same_decision_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            item, path = _fixture_item(tmp, 'unit-x')
            record = rq.decide(path, 'rejected', 'r', 'why', tmp)
            # Simulate a crash between claim and decision publication.
            (Path(tmp) / 'decisions' / (record['decision_id'] + '.json')).unlink()
            with self.assertRaisesRegex(rq.ReviewError, 'item already decided'):
                rq.decide(path, 'accepted', 'r', 'why', tmp)
            self.assertEqual(list((Path(tmp) / 'decisions').glob('review-decision-*.json')), [])
            self.assertEqual(rq.decide(path, 'rejected', 'r', 'why', tmp), record)
            self.assertEqual(len(list((Path(tmp) / 'decisions').glob('review-decision-*.json'))), 1)
            with self.assertRaisesRegex(rq.ReviewError, 'item already decided'):
                rq.decide(path, 'rejected', 'r', 'why', tmp)

    def test_claim_check_refuses_marker_without_decision_and_foreign_markers(self):
        with tempfile.TemporaryDirectory() as tmp:
            item, path = _fixture_item(tmp, 'unit-y')
            record = rq.decide(path, 'rejected', 'r', 'why', tmp)
            by_id, decisions = {item['item_id']: item}, {item['item_id']: record}
            claims = Path(tmp) / 'decisions' / rq.CLAIMS
            rq._check_claims(claims, by_id, decisions)  # consistent
            rq._check_claims(claims, by_id, decisions)
            (claims / '.review-leftover.json').write_bytes(b'junk')
            rq._check_claims(claims, by_id, decisions)  # temp leftovers are ignored
            with self.assertRaisesRegex(rq.ReviewError, 'interrupted decide'):
                rq._check_claims(claims, by_id, {})
            with self.assertRaisesRegex(rq.ReviewError, 'outside this queue'):
                rq._check_claims(claims, {}, decisions)
            (claims / 'stray.txt').write_text('x')
            with self.assertRaisesRegex(rq.ReviewError, 'unexpected file'):
                rq._check_claims(claims, by_id, decisions)


@unittest.skipUnless(INVENTORY.exists(), 'pinned c0 inventory unavailable')
class DecideClaimReleaseTests(unittest.TestCase):
    setUp = ReviewQueueTests.setUp
    tearDown = ReviewQueueTests.tearDown
    accepted_catalog = ReviewQueueTests.accepted_catalog
    campaign = ReviewQueueTests.campaign
    item_path = ReviewQueueTests.item_path
    release = ReviewQueueTests.release

    def test_release_refuses_a_claim_without_a_decision_then_completes(self):
        items = self.campaign()
        item = items[AMAZON]
        record = rq.decide(self.item_path(item), 'rejected', 'fixture reviewer', 'claim fixture', self.workspace / 'queue')
        decision = self.workspace / 'queue' / 'decisions' / (record['decision_id'] + '.json')
        self.assertEqual(len(self.release()['entries']), len(items))
        decision.unlink()
        with self.assertRaisesRegex(rq.ReviewError, 'interrupted decide'):
            self.release()
        rq.decide(self.item_path(item), 'rejected', 'fixture reviewer', 'claim fixture', self.workspace / 'queue')
        self.release()
        # Legacy decision (published before markers existed) stays valid.
        (self.workspace / 'queue' / 'decisions' / rq.CLAIMS / (item['item_id'] + '.json')).unlink()
        self.release()


@unittest.skipUnless(INVENTORY.exists(), 'pinned c0 inventory unavailable')
class CandidateBundleShaTests(unittest.TestCase):
    """candidate_sha256 hashes the archived derived bundle, not a unit file: it is cross-checked
    against its receipt and against what the adapter derives from the manifest being projected."""
    setUp = ReviewQueueTests.setUp
    tearDown = ReviewQueueTests.tearDown
    accepted_catalog = ReviewQueueTests.accepted_catalog
    campaign = ReviewQueueTests.campaign

    def prepared(self):
        from devicehunt.campaign import load_definition
        self.campaign()
        self.manifest = coverage.load(self.coverage)
        definition = load_definition(self.workspace, 'definition.json', 'inventory.json')
        self.unit_by_id = {u.unit_id: u for u in definition.units}
        self.entry_by_id = {u['unit_id']: u for u in self.manifest['units']}

    def check(self, unit_id, entry=None, reviewable=True):
        rq.check_candidate_bundle(self.unit_by_id[unit_id], entry or self.entry_by_id[unit_id], self.manifest,
                                  self.workspace, None, reviewable)

    def test_recorded_sha_matches_receipt_archive_bytes_and_derived_bundle(self):
        self.prepared()
        for unit_id, entry in self.entry_by_id.items():
            self.check(unit_id, reviewable=entry['outcome'] in rq.REVIEWABLE)
        receipt = self.manifest['receipts']
        payload = next(r for r in receipt if r['id'] == self.entry_by_id[AMAZON]['bundle_receipt'])['wrapper']['payload']
        self.assertEqual(payload['candidate_sha256'], self.entry_by_id[AMAZON]['candidate_sha256'])

    def test_tampered_bundle_bytes_are_refused_by_the_queue(self):
        self.prepared()
        sha = self.entry_by_id[AMAZON]['candidate_sha256']
        path = self.workspace / 'archive' / 'objects' / sha[:2] / sha
        path.chmod(0o644)
        path.write_bytes(path.read_bytes().replace(b'"unit_id"', b'"unit_ID"', 1))
        with self.assertRaisesRegex(rq.ReviewError, 'candidate bundle unreadable'):
            rq.build_queue(self.workspace, self.workspace / 'data' / 'devices.sqlite', self.coverage,
                           dict(definition='definition.json', inventory='inventory.json'), self.workspace / 'queue2',
                           self.workspace / 'data' / 'accepted-manifest.json')

    def test_sha_naming_another_bundle_is_refused(self):
        self.prepared()
        entry = dict(self.entry_by_id[AMAZON], candidate_sha256=self.entry_by_id[RRA]['candidate_sha256'])
        with self.assertRaisesRegex(rq.ReviewError, 'does not name the recorded candidate sha'):
            self.check(AMAZON, entry)

    def test_bundle_derived_from_other_bytes_than_the_projected_manifest_is_refused(self):
        from devicehunt.campaign_sources import ADAPTERS
        self.prepared()
        adapter = ADAPTERS['ised']
        real = adapter.records

        def changed(manifest, relative):
            output = real(manifest, relative)
            output['occurrences'] = output['occurrences'][1:]
            return output
        with patch.object(adapter, 'records', changed):
            with self.assertRaisesRegex(rq.ReviewError, 'candidate projection input differs'):
                self.check(AMAZON)

    def test_non_reviewable_outcomes_may_carry_no_sha_but_reviewable_may_not(self):
        self.prepared()
        none = dict(self.entry_by_id[AMAZON], outcome='not_reached', bundle_receipt=None, candidate_sha256=None)
        self.check(AMAZON, none, reviewable=False)
        with self.assertRaisesRegex(rq.ReviewError, 'no bundle receipt'):
            self.check(AMAZON, dict(none, outcome='validated_nonempty'))
        with self.assertRaisesRegex(rq.ReviewError, 'no bundle receipt'):  # a sha without a receipt is not enough
            self.check(AMAZON, dict(none, outcome='validated_nonempty', candidate_sha256='0' * 64))
        # A receipt-bearing non-reviewable entry still has to agree with its receipt.
        failed = dict(self.entry_by_id[AMAZON], outcome='failed', candidate_sha256=None)
        with self.assertRaisesRegex(rq.ReviewError, 'outcome differs'):
            self.check(AMAZON, failed, reviewable=False)
        other = dict(self.entry_by_id[AMAZON], candidate_sha256=None)
        with self.assertRaisesRegex(rq.ReviewError, 'does not name the recorded candidate sha'):
            self.check(AMAZON, other, reviewable=False)

    def test_unit_outcome_must_match_receipt_and_bundle(self):
        self.prepared()
        swapped = dict(self.entry_by_id[AMAZON], outcome='explicit_zero')  # both outcomes are reviewable
        with self.assertRaisesRegex(rq.ReviewError, 'outcome differs'):
            self.check(AMAZON, swapped)

    def with_receipt_payload(self, which, **changes):
        manifest = json.loads(json.dumps(self.manifest))
        receipt = next(r for r in manifest['receipts'] if r['id'] == self.entry_by_id[which]["bundle_receipt"])
        receipt['wrapper']['payload'].update(changes)
        receipt['wrapper_sha256'] = rq.digest(rq.canonical(receipt['wrapper']))  # consistent, so only the binding can fail
        self.manifest = manifest

    def test_receipt_of_another_revision_or_unit_is_refused(self):
        self.prepared()
        original = self.manifest
        self.with_receipt_payload(AMAZON, unit_revision='0' * 64)
        with self.assertRaisesRegex(rq.ReviewError, 'does not name the recorded candidate sha'):
            self.check(AMAZON)
        self.manifest = original
        self.with_receipt_payload(AMAZON, unit_id=RRA)
        with self.assertRaisesRegex(rq.ReviewError, 'does not name the recorded candidate sha'):
            self.check(AMAZON)

    def test_non_object_bundle_is_a_review_error(self):
        self.prepared()
        for raw in (b'[]', b'not json'):
            with patch.object(coverage, '_read_artifact', return_value=raw), \
                    patch.object(rq, 'digest', side_effect=lambda b, real=rq.digest: self.entry_by_id[AMAZON]['candidate_sha256']
                                 if b == raw else real(b)):
                with self.assertRaisesRegex(rq.ReviewError, 'not a JSON object'):
                    self.check(AMAZON)

    def test_queue_refuses_a_structurally_invalid_coverage_manifest(self):
        self.prepared()
        with patch.object(coverage, 'validate', side_effect=coverage.CoverageError('boom')):
            with self.assertRaisesRegex(rq.ReviewError, 'coverage manifest invalid: boom'):
                rq.build_queue(self.workspace, self.workspace / 'data' / 'devices.sqlite', self.coverage,
                               dict(definition='definition.json', inventory='inventory.json'), self.workspace / 'queue3',
                               self.workspace / 'data' / 'accepted-manifest.json')

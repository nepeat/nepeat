"""Malformed saved JSON fails as source evidence; bugs and I/O still stop replay."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from devicehunt.campaign_sources import ADAPTERS, rra_input_shape, ised_input_shape
from tests.test_campaign import build_workspace, fresh, runner_for, ROOT, UNITS


RRA_ZERO = 'rra-official-query-78fc6b2e95f6'
RRA_MANIFEST = 'data/raw/rra-edge-core-space-official/bundle.json'
ISED_ZERO = 'ised-rel-query-celestica'
ISED_MANIFEST = 'data/raw/ised-celestica/capture-paged.json'


class SavedInputShapeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.workspace = Path(self.tmp.name)
        for name in ('socket.socket.connect', 'socket.create_connection', 'socket.getaddrinfo',
                     'http.client.HTTPConnection.connect', 'urllib.request.OpenerDirector.open',
                     'subprocess.run', 'subprocess.Popen'):
            guard = patch(name, side_effect=AssertionError('network/external process forbidden'))
            guard.start()
            self.addCleanup(guard.stop)

    def assert_bad_shapes(self, check, original, changes):
        path = self.workspace / 'bundle.json'
        for label, mutate in changes:
            with self.subTest(label=label):
                data = copy.deepcopy(original)
                mutated = mutate(data)
                path.write_text(json.dumps(data if mutated is None else mutated))
                with self.assertRaisesRegex(ValueError, 'saved-input shape'):
                    check(path)

    def test_rra_required_and_nested_bundle_shapes(self):
        data = json.loads((ROOT / RRA_MANIFEST).read_text())
        # No company-evidence dependency in this explicit-zero source.
        self.assert_bad_shapes(rra_input_shape, data, [
            ('root list', lambda d: []),
            ('missing form', lambda d: d.pop('form') and None),
            ('form array', lambda d: d.update(form=[])),
            ('missing hash', lambda d: d['csv_before'].pop('sha256') and None),
            ('query wrong shape', lambda d: d.update(query=[])),
            ('query nonstring', lambda d: d['query'].update(maker=['Edge-Core'])),
            ('details nonlist', lambda d: d.update(details={})),
            ('detail nonobject', lambda d: d.update(details=[None])),
            ('detail missing path', lambda d: d.update(details=[{'sha256': 'a', 'certificate': 'x', 'url': 'x'}])),
            ('detail url nonstring', lambda d: d.update(details=[{'path': 'x', 'sha256': 'a', 'certificate': 'x', 'url': []}])),
            ('optional page malformed', lambda d: d.update(first_result_page={'path': 'x'})),
            ('empty proof malformed', lambda d: d.update(empty_result=[])),
            ('company entry malformed', lambda d: d.update(company_evidence=[])),
        ])

    def test_ised_required_and_nested_bundle_shapes(self):
        data = json.loads((ROOT / ISED_MANIFEST).read_text())
        self.assert_bad_shapes(ised_input_shape, data, [
            ('root null', lambda d: False),
            ('missing query', lambda d: d.pop('company_query') and None),
            ('responses nonlist', lambda d: d.update(responses={})),
            ('response nonobject', lambda d: d.update(responses=[[]])),
            ('url array', lambda d: d['responses'][0].update(url=[])),
            ('final_url null', lambda d: d['responses'][0].update(final_url=None)),
            ('method object', lambda d: d['responses'][0].update(method={})),
            ('result_pages nonlist', lambda d: d.update(result_pages={})),
            ('result_pages nested object', lambda d: d.update(result_pages=[{}])),
        ])
        data['schema_version'] = 1
        self.assert_bad_shapes(ised_input_shape, data, [
            ('legacy response nonobject', lambda d: d.update(responses=[None])),
        ])

    def evidence(self, name, data):
        path = self.workspace / name
        path.write_text(json.dumps(data))
        return {'path': name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}

    def test_company_evidence_nested_shapes(self):
        from devicehunt.campaign_sources import company_evidence_shape
        for label, data in [
            ('root list', []),
            ('missing reviewed date', {}),
            ('date nonstring', {'reviewed_on': []}),
            ('historical missing base', {'kind': 'celestica-historical-company-names', 'exhibits': {}}),
            ('composite entries nonlist', {'kind': 'rra-composite-manufacturer-cells', 'reviewed_on': '2026-10-12',
                                         'base': self.evidence('base.json', {'reviewed_on': '2026-10-12'}), 'entries': {}}),
            ('historical exhibit nonobject', {'kind': 'celestica-historical-company-names',
                                             'base': self.evidence('base.json', {'reviewed_on': '2026-10-12'}),
                                             'exhibits': {'x': []}}),
        ]:
            with self.subTest(label=label):
                entry = self.evidence('evidence.json', data)
                with self.assertRaisesRegex(ValueError, 'saved-input shape'):
                    company_evidence_shape(self.workspace, entry)

    def test_composite_entry_and_part_shapes(self):
        from devicehunt.campaign_sources import company_evidence_shape
        original = json.loads((ROOT / 'data/raw/rra-celestica-official/rra-composite-manufacturer-cells-2026-10-11.json').read_text())
        original['base'] = self.evidence('base.json', {'reviewed_on': '2026-10-12'})
        for label, mutate in [
            ('entry nonobject', lambda d: d.update(entries=[None])),
            ('missing detail URL', lambda d: d['entries'][0].pop('detail_url')),
            ('raw slice nonobject', lambda d: d['entries'][0]['cell'].update(raw_html_cell_bytes=[])),
            ('raw offset noninteger', lambda d: d['entries'][0]['cell']['raw_html_cell_bytes'].update(offset='0')),
            ('part nonobject', lambda d: d['entries'][0].update(parts=[None])),
            ('part text nonstring', lambda d: d['entries'][0]['parts'][0].update(text=[])),
            ('missing context target', lambda d: d['entries'][0]['parts'][0].pop('target')),
        ]:
            with self.subTest(label=label):
                data = copy.deepcopy(original)
                mutate(data)
                with self.assertRaisesRegex(ValueError, 'saved-input shape'):
                    company_evidence_shape(self.workspace, self.evidence('composite.json', data))

    def test_unsupported_wrapper_chain_is_rejected_before_descendant_reads(self):
        from devicehunt.campaign_sources import company_evidence_shape
        from devicehunt import documents, rra_company_names
        entry = self.evidence('evidence-0.json', {'reviewed_on': '2026-10-12'})
        for index in range(1, 1051):
            entry = self.evidence('evidence-' + str(index) + '.json', {
                'kind': 'celestica-historical-company-names', 'base': entry, 'exhibits': {}})
        with self.assertRaises(ValueError):
            rra_company_names.load(self.workspace, entry)
        with patch('devicehunt.documents.dependency', wraps=documents.dependency) as read:
            with self.assertRaisesRegex(ValueError, 'unsupported company evidence nesting'):
                company_evidence_shape(self.workspace, entry)
            self.assertEqual(read.call_count, 2)

        # A composite base may be historical, never another composite wrapper.
        entry = self.evidence('nested-composite.json', {
            'kind': 'rra-composite-manufacturer-cells', 'reviewed_on': '2026-10-12',
            'base': self.evidence('composite-base.json', {
                'kind': 'rra-composite-manufacturer-cells', 'reviewed_on': '2026-10-12'}), 'entries': []})
        with self.assertRaisesRegex(ValueError, 'unsupported company evidence nesting'):
            company_evidence_shape(self.workspace, entry)

        # Unused JSON kind values must not make membership testing raise TypeError.
        # Existing semantic readers retain responsibility for supported leaf content.
        for kind in ([], {}):
            with self.subTest(unused_kind=kind):
                entry = self.evidence('unused-kind.json', {
                    'kind': 'celestica-historical-company-names',
                    'base': self.evidence('unused-kind-base.json', {
                        'kind': kind, 'reviewed_on': '2026-10-12'}), 'exhibits': {}})
                company_evidence_shape(self.workspace, entry)
                with self.assertRaises(ValueError):
                    rra_company_names.load(self.workspace, entry)

    def start(self, *, units=UNITS, mutate=None, name='fixture'):
        workspace = self.workspace / name
        workspace.mkdir()
        build_workspace(workspace, units=units, mutate=mutate)
        fresh(workspace)[0].close()
        runner = runner_for(workspace)
        self.addCleanup(runner.archive.close)
        return runner

    def test_malformed_bundles_checkpoint_failed_and_reuse(self):
        def mutate(workspace):
            for relative, change in [(RRA_MANIFEST, lambda d: d.pop('form')),
                                     (ISED_MANIFEST, lambda d: d.update(result_pages=[{}]))]:
                path = workspace / relative
                data = json.loads(path.read_text())
                change(data)
                path.write_text(json.dumps(data))
        runner = self.start(units=(RRA_ZERO, ISED_ZERO), mutate=mutate)
        report = runner.run('malformed-1')
        self.assertEqual(report['run_state'], 'complete')
        self.assertEqual([u['outcome'] for u in report['units']], ['failed', 'failed'])
        self.assertEqual(report['units_accounted_for']['observed'], 0)
        self.assertEqual(report['units_accounted_for']['failed'], 2)
        self.assertEqual({j['state'] for j in runner.state.campaign_jobs(runner.definition.campaign_id)}, {'complete'})
        for unit in report['units']:
            candidate = json.loads(runner.archive.resolve(unit['candidate_sha256']).read_bytes())
            self.assertEqual(candidate['error']['type'], 'ValueError')
            self.assertIn('saved-input shape', candidate['error']['message'])
        again = runner.run('malformed-2')
        self.assertEqual(again['job_actions'], {'reused': 6})
        self.assertEqual(again['new_bytes'], 0)

    def test_healthy_replay_preserves_pins_and_reuses(self):
        runner = self.start()
        report = runner.run('healthy-1')
        self.assertEqual(report['run_state'], 'complete')
        self.assertEqual(report['units_accounted_for']['validated_complete'], 4)
        self.assertTrue(all(u['pinned_comparison'] == 'matches_pinned' for u in report['units']))
        sequence = runner.archive.readiness()['sequence']
        again = runner.run('healthy-2')
        self.assertEqual(again['job_actions'], {'reused': 12})
        self.assertEqual(again['new_bytes'], 0)
        self.assertEqual(runner.archive.readiness()['sequence'], sequence)

    def test_validator_bugs_and_io_errors_propagate_with_pending_job(self):
        for exception in (RuntimeError('bug'), KeyError('bug'), TypeError('bug'), OSError('disk')):
            with self.subTest(exception=type(exception).__name__):
                runner = self.start(units=(RRA_ZERO,), name=type(exception).__name__)
                with patch('devicehunt.rra.validate', side_effect=exception):
                    with self.assertRaises(type(exception)):
                        runner.run('bug')
                jobs = runner.state.campaign_jobs(runner.definition.campaign_id)
                self.assertIn('pending', {j['state'] for j in jobs})
                self.assertEqual(runner.state.campaign_run('bug')['state'], 'stopped')

    def test_missing_manifest_and_dependency_are_io_errors(self):
        with self.assertRaises(FileNotFoundError):
            ADAPTERS['rra'].records(self.workspace / 'absent.json', 'absent.json')
        entry = {'path': 'missing.json', 'sha256': 'a' * 64}
        data = json.loads((ROOT / RRA_MANIFEST).read_text())
        data['company_evidence'] = entry
        path = self.workspace / 'bundle.json'
        path.write_text(json.dumps(data))
        with self.assertRaises(FileNotFoundError):
            rra_input_shape(path)


if __name__ == '__main__':
    unittest.main()

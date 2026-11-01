import contextlib
import csv
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import specifications
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog


class SourceFactTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'db.sqlite'
        self.c = Catalog(self.path)
        rows = [
            ('curated', 'Celestica', 'R4023-F9102-A0', [('power_description', 'Curated 2 x PSU')]),
            ('netbox', 'Celestica', 'R4023-F9102-A0', [
                ('netbox_u_height', '1 (a.yaml:6)'), ('netbox_airflow', 'front-to-rear (a.yaml:8)'),
                ('netbox_power_ports', 'PS1: maximum_draw=750 (line 12)'),
                ('sonic_psu_count', '2'), ('pddf_num_psus', '2 (x:4)'), ('power', '{"connector": "barrel"}'),
                ('redhat_cert_detail_form_factors', '["Server"]')]),
            ('other', 'Oracle', 'R4023-F9102-A0', [('spec_power_supply', '2 x 800 W')]),
            ('nopn', 'Celestica', None, [('spec_nodes_per_system', '4')]),
        ]
        for sid, company, pn, facts in rows:
            self.c.source(sid, 'https://example.org/' + sid, 'community' if sid == 'netbox' else 'vendor', 'fixture', 'fixture')
            self.c.add(sid, sid, 'https://example.org/' + sid, 'Celestica', 'DS4000', 'DS4000', 'switch', {},
                       [(company, 'brand')], [('part_number', pn)] if pn else [], facts)
            self.c.finish(sid, 1)

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def cli(self, *args):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(main(['--db', str(self.path), 'specs', *args]), 0)
        return out.getvalue()

    def test_default_output_has_only_curated_rows_and_no_new_columns(self):
        rows = specifications.search(self.c, 'DS4000', limit=-1)
        self.assertEqual([r['statement'] for r in rows], ['Curated 2 x PSU'])
        self.assertNotIn('origin', rows[0])
        on = specifications.search(self.c, 'DS4000', limit=-1, include_source_facts=True)
        stripped = [{k: v for k, v in r.items() if k not in ('origin', 'source_fact_name')}
                    for r in on if r['origin'] == 'curated statement']
        self.assertEqual(stripped, rows)

    def test_whitelist_maps_categories_and_labels_origin(self):
        rows = specifications.search(self.c, 'DS4000', limit=-1, include_source_facts=True)
        facts = {r['source_fact_name']: r for r in rows if r['origin'] == 'source fact'}
        self.assertEqual({n: r['category'] for n, r in facts.items()}, {
            'netbox_u_height': 'physical_description', 'netbox_airflow': 'cooling_description',
            'netbox_power_ports': 'power_description', 'spec_power_supply': 'power_description',
            'spec_nodes_per_system': 'physical_description'})
        for name in ('netbox_u_height', 'netbox_airflow'):
            self.assertEqual((facts[name]['source_id'], facts[name]['tier']), ('netbox', 'community'))
            self.assertEqual(facts[name]['source_url'], 'https://example.org/netbox')
        self.assertTrue(all(r['origin'] == 'curated statement' for r in rows if not r['source_fact_name']))

    def test_software_declarations_and_blobs_are_excluded(self):
        for name in ('sonic_psu_count', 'pddf_num_psus', 'stratum_phal_psu_count', 'em_exposes_psu_entries',
                     'power', 'power_supply', 'redhat_cert_detail_form_factors', 'processor_power_limit'):
            self.assertIsNone(specifications.source_fact_category(name), name)
        for name in ('netbox_power_ports', 'spec_shared_power_supply_details', 'energystar_typical_psu_count',
                     'dmi_chassis_type', 'form_factor', 'spec_number_of_identical_nodes'):
            self.assertIsNotNone(specifications.source_fact_category(name), name)
        # Whole-name matching only: no substring or prefix leakage.
        self.assertIsNone(specifications.source_fact_category('xnetbox_u_height'))
        self.assertIsNone(specifications.source_fact_category('netbox_u_height_extra'))

    def test_exact_modes_and_vendor_filters_still_apply(self):
        exact = specifications.search(self.c, 'R4023-F9102-A0', limit=-1, exact_part_number=True, include_source_facts=True)
        self.assertEqual({r['source_id'] for r in exact}, {'curated', 'netbox', 'other'})
        self.assertTrue(all(r['matched_part_numbers'] == ['R4023-F9102-A0'] for r in exact))
        celestica = specifications.search(self.c, 'R4023-F9102-A0', 'Celestica', -1, exact_part_number=True, include_source_facts=True)
        self.assertEqual({r['source_id'] for r in celestica}, {'curated', 'netbox'})
        model = specifications.search(self.c, 'DS4000', 'Oracle', -1, exact_model=True, include_source_facts=True)
        self.assertEqual({r['source_id'] for r in model}, {'other'})
        self.assertEqual(specifications.search(self.c, 'DS4000', 'Oracle', 0, include_source_facts=True), [])
        self.assertEqual(len(specifications.search(self.c, 'DS4000', None, 2, include_source_facts=True)), 2)
        # A record without the part number never contributes its facts.
        self.assertNotIn('nopn', {r['source_id'] for r in exact})

    def test_cli_flag_adds_columns_only_when_requested(self):
        base = json.loads(self.cli('DS4000', '--limit', '-1', '--format', 'json'))
        self.assertTrue(all('origin' not in r for r in base))
        flagged = json.loads(self.cli('DS4000', '--include-source-facts', '--limit', '-1', '--format', 'json'))
        self.assertGreater(len(flagged), len(base))
        rows = list(csv.DictReader(io.StringIO(self.cli('DS4000', '--include-source-facts', '--limit', '-1', '--format', 'csv'))))
        self.assertEqual({r['origin'] for r in rows}, {'curated statement', 'source fact'})
        before = self.path.read_bytes()
        self.cli('DS4000', '--include-source-facts')
        self.assertEqual(self.path.read_bytes(), before)


EXPECTED_CATEGORIES = {
    'power_description': '''spec_power_supply spec_power_supply_details spec_power_supply_quantity_and_rating_w
        spec_shared_power_supply_details spec_shared_power_supply_quantity_and_rating_w netbox_power_ports
        netbox_power_port_count psu_configuration_1 psu_configuration_2 psu_configuration_3 psu_rated_output_raw
        psu_model_as_printed psu_redundancy_as_printed psu_interchangeability_as_printed power_statement_as_printed
        power_redundancy_statement_as_printed power_supply_note_as_printed power_supply_part_numbers_as_printed
        power_supply_fan_tray_part_numbers_as_printed back_panel_power_supplies_as_printed'''.split() +
        ['energystar_%s_%s' % (t, k) for t in ('high', 'low', 'typical')
         for k in ('psu_count', 'psu_model', 'psu_rated_output_watts', 'idle_watts')],
    'cooling_description': '''netbox_airflow failed_psu_slot_airflow_note_as_printed fan_service_constraint_as_printed
        back_panel_fan_modules_as_printed rear_fan_part_numbers_as_printed'''.split(),
    'physical_description': '''netbox_u_height netbox_is_full_depth netbox_weight form_factor spec_form_factor
        spec_shared_form_factor energystar_form_factor chassis_height chassis_height_as_printed chassis_dimensions
        compatibility_dimensions_in compatibility_dimensions_mm compatibility_weight sm_1u_chassis_description
        sm_2u_chassis_description dmi_chassis_type spec_nodes_per_system spec_number_of_identical_nodes
        spec_shared_enclosure'''.split(),
}


class WhitelistTableTests(unittest.TestCase):
    def test_every_whitelisted_name_is_pinned_to_its_category(self):
        for category, names in EXPECTED_CATEGORIES.items():
            for name in names:
                self.assertEqual(specifications.source_fact_category(name), category, name)
        # No pattern group may match a name outside the pinned table.
        probes = ['power_limit', 'power_configuration_limit', 'power', 'power_supply', 'processor_power_limit',
                  'energystar_typical_idle_power', 'energystar_archive_Power Supply Rated Output Typical or Single Configuration (W)',
                  'redhat_cert_detail_form_factors', 'sonic_psu_count', 'stratum_node_count', 'node_models']
        for name in probes:
            self.assertIsNone(specifications.source_fact_category(name), name)

    def test_placeholders_and_labels(self):
        statement = specifications.source_fact_statement
        for value in ('', ' ', '--', 'N/A', 'n/a', 'None', 'TBD'):
            self.assertIsNone(statement('spec_form_factor', value), value)
        self.assertEqual(statement('spec_form_factor', '2U'), 'form factor as submitted to SPEC: 2U')
        self.assertEqual(statement('energystar_typical_idle_watts', '204.80'),
                         'idle power as measured in the ENERGY STAR typical or single test configuration: 204.80 W')
        self.assertEqual(statement('form_factor', 'Full Height Standard PCIe Card'),
                         'component form factor (not a chassis): Full Height Standard PCIe Card')
        self.assertEqual(statement('form_factor', '2U Rackmount, 4 Nodes'), 'form factor: 2U Rackmount, 4 Nodes')


class TwoDeviceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'db.sqlite'
        self.c = Catalog(self.path)
        for sid, model, facts in [
                ('one', 'ALPHA-1', [('netbox_u_height', '1'), ('spec_form_factor', '--'), ('dmi_chassis_type', 'Other')]),
                ('two', 'BRAVO-2', [('netbox_u_height', '2'), ('power_limit', 'disclaimer')])]:
            self.c.source(sid, 'https://example.org/' + sid, 'vendor', 'fixture', 'fixture')
            self.c.add(sid, sid, 'https://example.org/' + sid, 'Celestica', model, model, 'switch', {},
                       [('Celestica', 'brand')], [], facts)
            self.c.finish(sid, 1)

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def test_discovery_filter_keeps_other_devices_out(self):
        rows = specifications.search(self.c, 'ALPHA-1', limit=-1, include_source_facts=True)
        self.assertEqual({r['model'] for r in rows}, {'ALPHA-1'})
        self.assertEqual({(r['source_fact_name'], r['category']) for r in rows},
                         {('netbox_u_height', 'physical_description'), ('dmi_chassis_type', 'physical_description')})
        both = specifications.search(self.c, 'BRAVO-2', limit=-1, include_source_facts=True)
        self.assertEqual([(r['model'], r['statement']) for r in both], [('BRAVO-2', 'NetBox rack height (U, library entry): 2')])


class LabelDedupeTests(unittest.TestCase):
    DMI = 'Other (Desktop/Inspur/SA5248/SA5248M4/003B271C69BC:66) [19 of 19 dumps]'

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'db.sqlite'
        self.c = Catalog(self.path)
        self.c.source('s1', 'https://example.org/s1', 'vendor', 'fixture', 'fixture')
        self.c.source('s2', 'https://example.org/s2', 'vendor', 'fixture', 'fixture')
        for sid, rid, facts in [
                ('s1', 'r1', [('spec_power_supply_quantity_and_rating_w', '1 x 650'), ('form_factor', '2U'),
                              ('netbox_power_ports', 'PS1: maximum_draw=337 (line 12)'), ('dmi_chassis_type', self.DMI),
                              ('power_description', 'Curated PSU')]),
                ('s2', 'r2', [('form_factor', '2U')])]:
            self.c.add(sid, rid, 'https://example.org/' + rid, 'Celestica', 'DS4000', 'DS4000', 'switch', {},
                       [('Celestica', 'brand')], [], facts)
            self.c.finish(sid, 1)

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def facts(self):
        return [r for r in specifications.search(self.c, 'DS4000', limit=-1, include_source_facts=True)
                if r['origin'] == 'source fact']

    def test_labels_prefix_bare_values_and_keep_raw_value(self):
        by = {(r['source_fact_name'], r['source_id']): r['statement'] for r in self.facts()}
        self.assertEqual(by['spec_power_supply_quantity_and_rating_w', 's1'], 'PSU quantity and rating (W): 1 x 650')
        self.assertEqual(by['form_factor', 's1'], 'form factor: 2U')
        draw = by['netbox_power_ports', 's1']
        self.assertTrue(draw.startswith('NetBox power port maximum draw entry (W, library entry'), draw)
        self.assertIn('not PSU capacity or measured draw', draw)
        self.assertTrue(draw.endswith('PS1: maximum_draw=337 (line 12)'))

    def test_every_whitelisted_name_has_a_label_or_special_wording(self):
        special = {n for n in sum(EXPECTED_CATEGORIES.values(), []) if specifications.ENERGYSTAR_IDLE.fullmatch(n)}
        for name in sum(EXPECTED_CATEGORIES.values(), []):
            if name not in special:
                self.assertIn(name, specifications.SOURCE_FACT_LABELS, name)
                self.assertTrue(specifications.source_fact_statement(name, 'X').endswith(': X'), name)

    def test_dmi_citation_path_is_stripped_from_text_but_kept_in_url(self):
        row = next(r for r in self.facts() if r['source_fact_name'] == 'dmi_chassis_type')
        self.assertEqual(row['statement'],
                         'chassis type reported by firmware in submitted dmidecode dumps: Other [19 of 19 dumps]')
        self.assertNotIn('003B271C69BC', row['statement'])
        self.assertEqual(row['source_url'], 'https://example.org/r1')
        s = specifications.source_fact_statement
        self.assertEqual(s('dmi_chassis_type', 'Rack Mount Chassis (Server/Oracle/SUN/SUN FIRE X4270 M2 SERVER/FF52124EF1E7:71)'),
                         'chassis type reported by firmware in submitted dmidecode dumps: Rack Mount Chassis')
        # No citation: value untouched apart from the label.
        self.assertEqual(s('dmi_chassis_type', 'Other'), 'chassis type reported by firmware in submitted dmidecode dumps: Other')

    def test_different_sources_stay_separate_identical_rows_collapse(self):
        form = [r for r in self.facts() if r['source_fact_name'] == 'form_factor']
        self.assertEqual({r['source_id'] for r in form}, {'s1', 's2'})
        base = specifications.search(self.c, 'DS4000', limit=-1, include_source_facts=True)
        dup = next(r for r in base if r['source_fact_name'] == 'form_factor')
        # Multiple records of one source with an identical fact: keep one.
        again = dict(dup, record_id='r1-copy')
        curated = [dict(r) for r in base if r['origin'] == 'curated statement']
        out = specifications.dedupe_source_facts(base + [again, dict(dup, source_id='s3')])
        self.assertEqual(len(out), len(base) + 1)
        self.assertNotIn('r1-copy', {r['record_id'] for r in out})
        # Curated rows are never collapsed, even when fully identical.
        twice = specifications.dedupe_source_facts(curated + curated)
        self.assertEqual(len(twice), 2 * len(curated))

    def test_duplicate_records_of_one_source_collapse_end_to_end(self):
        self.c.add('s1', 'r1b', 'https://example.org/r1b', 'Celestica', 'DS4000', 'DS4000', 'switch', {},
                   [('Celestica', 'brand')], [], [('form_factor', '2U')])
        self.c.finish('s1', 1)
        rows = [r for r in self.facts() if r['source_fact_name'] == 'form_factor' and r['source_id'] == 's1']
        self.assertEqual(self.c.db.execute("SELECT COUNT(*) FROM records WHERE source_id='s1'").fetchone()[0], 2)
        self.assertEqual(len(rows), 1)

    def test_flag_off_output_is_byte_identical_to_curated_rows(self):
        off = specifications.search(self.c, 'DS4000', limit=-1)
        self.assertEqual([r['statement'] for r in off], ['Curated PSU'])
        self.assertTrue(all('origin' not in r for r in off))
        on = specifications.search(self.c, 'DS4000', limit=-1, include_source_facts=True)
        curated_on = [{k: v for k, v in r.items() if k not in ('origin', 'source_fact_name')}
                      for r in on if r['origin'] == 'curated statement']
        self.assertEqual(json.dumps(curated_on, sort_keys=True), json.dumps(off, sort_keys=True))


class DedupeMutantTests(unittest.TestCase):
    def row(self, **kw):
        base = {'record_id': 'r', 'device_id': 'd1', 'category': 'physical_description', 'statement': 'form factor: 2U',
                'source_id': 's1', 'source_fact_name': 'form_factor', 'origin': 'source fact'}
        return base | kw

    def test_dmi_type_text_with_parentheses_survives_stripping(self):
        value = 'Rack Mount Chassis (2U) (Server/Quanta/D51/D51B-2U/ED213C10A885:63)'
        self.assertEqual(specifications.source_fact_statement('dmi_chassis_type', value),
                         'chassis type reported by firmware in submitted dmidecode dumps: Rack Mount Chassis (2U)')

    def test_dedupe_keeps_same_statement_and_source_on_two_devices(self):
        rows = [self.row(), self.row(record_id='r2', device_id='d2')]
        self.assertEqual(len(specifications.dedupe_source_facts(rows)), 2)

    def test_dedupe_keeps_two_fact_names_from_one_source_on_one_device(self):
        rows = [self.row(), self.row(record_id='r2', source_fact_name='spec_form_factor')]
        self.assertEqual(len(specifications.dedupe_source_facts(rows)), 2)
        self.assertEqual(len(specifications.dedupe_source_facts(rows + [self.row(record_id='r3')])), 2)

import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import buying
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main


class BuyingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / 'catalog.sqlite'
        self.c = Catalog(self.path)

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def add(self, source, model='MODEL-V', vendor='Wiwynn', facts=(), ids=(), tier='vendor', revision='r1'):
        capture = self.path.parent / (source + '.txt')
        capture.write_bytes(b'capture')
        self.c.source(source, 'https://example.org/' + source, tier, 'test', 'source scope', path=capture)
        self.c.add(source, source, 'https://example.org/' + source, vendor, model, model, 'server', {},
                   [(vendor, 'brand')], identifiers=ids, facts=facts, revision=revision)
        self.c.finish(source, 1)

    def report(self, query, vendor=None):
        return buying.run(self.c.db, query, vendor)

    def test_literal_alias_and_part_record_isolation_and_casefold(self):
        self.add('a', facts=[('power', 'AC option')], ids=[('alias', 'SPECIAL-V'), ('part_number', 'FULL-AC-F')])
        self.add('b', facts=[('power', 'DC option')])
        self.assertEqual(len(self.report('model-v')['matched_records']), 2)
        for query in ('special-v', 'full-ac-f'):
            r = self.report(query)
            self.assertEqual([s['statement'] for s in r['categories']['power']['statements']], ['AC option'])
        self.assertEqual(self.report('FULL-AC')['match_status'], 'no_exact_records')

    def test_vendor_is_same_record_and_unknown_variants_never_borrow(self):
        self.add('a', facts=[('power', 'A')], ids=[('alias', 'SHARED')])
        self.add('b', vendor='Hyve', facts=[('power', 'B')], ids=[('alias', 'SHARED')])
        r = self.report('SHARED', 'Wiwynn')
        self.assertEqual([s['statement'] for s in r['categories']['power']['statements']], ['A'])
        self.assertEqual(self.report('MODEL-V+', 'Wiwynn')['matched_records'], [])
        self.add('plus', model='MODEL-V+', facts=[('power', 'PLUS')])
        self.assertEqual([s['statement'] for s in self.report('MODEL-V+')['categories']['power']['statements']], ['PLUS'])
        self.assertEqual(self.report('MODEL')['match_status'], 'no_exact_records')

    def test_conflicts_context_provenance_and_unclassified_are_preserved(self):
        self.add('a', facts=[('power', '100 W'), ('power', '200 W'), ('source_conflict', 'unresolved'),
                             ('source_page', '7'), ('document_date', '2018'), ('source_scope', 'configuration A'),
                             ('novel_field', 'AC PSU in prose must not be classified'), ('label_observed_firmware', '1.2')])
        r = self.report('MODEL-V')
        statements = r['categories']['power']['statements']
        self.assertEqual([s['statement'] for s in statements], ['100 W', '200 W'])
        s = statements[0]
        self.assertEqual(s['revision'], 'r1')
        self.assertEqual(len(s['sha256']), 64)
        self.assertEqual(s['company_roles'][0]['role'], 'brand')
        self.assertEqual(s['source_url'], 'https://example.org/a')
        self.assertEqual(s['context_record_id'], r['matched_records'][0]['record_id'])
        self.assertIn({'name': 'source_page', 'value': '7'}, r['matched_records'][0]['context_facts'])
        self.assertIn('novel_field', [s['fact_name'] for s in r['unclassified_facts']])
        table = buying.render_table(r)
        for text in ('unresolved', '2018', 'source_page: 7', 'configuration A', 'bootability'):
            self.assertIn(text, table)
        self.assertEqual(r['categories']['licensing']['status'], 'not_collected')

    def test_empty_category_seller_only_and_mixed(self):
        self.add('seller', tier='marketplace', facts=[('power', 'seller says 200 W')])
        r = self.report('MODEL-V')
        self.assertEqual(r['categories']['power']['status'], 'seller_only')
        self.assertEqual(r['categories']['cooling']['status'], 'not_collected')
        self.add('vendor', facts=[('power', 'AC options')])
        self.assertEqual(self.report('MODEL-V')['categories']['power']['status'], 'collected_source_statements')
        self.assertEqual({s['claim_status'] for s in self.report('MODEL-V')['categories']['power']['statements']}, {'source_statement', 'seller_claim'})

    def test_reviewed_label_voltage_and_bios_array_mapping_preserves_scope(self):
        rows = [{'bios': 'AMI 001 UEFI', 'certId': '0001'}, {'bios': 'AMI 001 UEFI', 'certId': '0001'}]
        self.add('label', facts=[('label_observed_input_voltage_range', '100-240V~ 50-60Hz 10-5A'),
                                 ('evidence_scope', 'photographed unit only')])
        self.add('cert', facts=[('compatibility_bios_certification_rows', json.dumps(rows)),
                                ('evidence_scope', 'submitted compatibility configurations only')])
        report = self.report('MODEL-V')
        power = report['categories']['power']['statements']
        firmware = report['categories']['firmware_boot_software']['statements']
        self.assertEqual([s['fact_name'] for s in power], ['label_observed_input_voltage_range'])
        self.assertEqual([s['fact_name'] for s in firmware], ['compatibility_bios_certification_rows'])
        self.assertEqual(json.loads(firmware[0]['statement']), rows)
        self.assertNotEqual(power[0]['context_record_id'], firmware[0]['context_record_id'])
        self.assertEqual(report['categories']['licensing']['status'], 'not_collected')
        self.assertNotIn('compatibility_bios_certification_rows', [s['fact_name'] for s in report['unclassified_facts']])

    def test_oracle_variants_preserve_labels_and_processor_wattage_scope(self):
        facts = [('configuration_limit', 'variant descriptions; installed configuration unknown'),
                 ('processor_power_limit', 'processor maxima, not PSU/input/wall draw'),
                 ('sm_1u_product_description_literal', '1U: memory, PCIe and processor 150W'),
                 ('sm_2u_product_description_literal', '2U: memory, bays and processor 205W')]
        for variant, slots, watts in [('sm_1u', 'one PCIe slot', '150W Max'),
                                      ('sm_2u', 'five PCIe slots', '205W Max / 165W Max for GPU')]:
            facts.extend([(variant + '_chassis_description', variant),
                          (variant + '_memory_description', '24 DDR4 DIMM slots'),
                          (variant + '_pcie_description', slots),
                          (variant + '_drive_bays_description', variant + ' drive bays'),
                          (variant + '_processor_power_raw', watts)])
        self.add('oracle', model='Oracle Server X7-2c', vendor='Oracle', facts=facts)
        self.add('other', model='Oracle Server X7-2', vendor='Oracle', facts=[('power', 'other PSU')])
        report = self.report('Oracle Server X7-2c')
        self.assertEqual(report['categories']['power']['status'], 'not_collected')
        chassis = report['categories']['chassis_physical']['statements']
        self.assertEqual({s['fact_name'] for s in chassis},
                         {'sm_1u_chassis_description', 'sm_2u_chassis_description'})
        compute = report['categories']['compute_network_storage']['statements']
        self.assertEqual(len(compute), 8)
        self.assertEqual({s['statement'] for s in compute if s['fact_name'].endswith('processor_power_raw')},
                         {'150W Max', '205W Max / 165W Max for GPU'})
        self.assertEqual(len({s['context_record_id'] for s in chassis + compute}), 1)
        unclassified = {s['fact_name'] for s in report['unclassified_facts']}
        self.assertTrue({'sm_1u_product_description_literal', 'sm_2u_product_description_literal',
                         'processor_power_limit', 'configuration_limit'} <= unclassified)
        table = buying.render_table(report)
        self.assertIn('installed configuration unknown', table)
        self.assertIn('sm_2u_processor_power_raw: 205W Max / 165W Max for GPU', table)
        self.assertNotIn('other PSU', table)

    def test_oracle_variant_fields_do_not_cross_record_identifier_scope(self):
        self.add('one', model='Oracle Server X7-2c', vendor='Oracle',
                 ids=[('part_number', 'SM-1U-ONLY')],
                 facts=[('sm_1u_pcie_description', 'one PCIe slot'), ('source_scope', '1U record')])
        self.add('two', model='Oracle Server X7-2c', vendor='Oracle',
                 ids=[('part_number', 'SM-2U-ONLY')],
                 facts=[('sm_2u_pcie_description', 'five PCIe slots'), ('source_scope', '2U record')])
        report = self.report('SM-1U-ONLY')
        self.assertEqual([s['fact_name'] for s in report['categories']['compute_network_storage']['statements']],
                         ['sm_1u_pcie_description'])
        self.assertEqual(len(report['matched_records']), 1)
        self.assertNotIn('2U record', buying.render_table(report))

    def test_community_artifact_declarations_keep_record_tier_and_no_related_alias(self):
        declarations = [('artifact_model_declaration', 'accton,as7772_32x'),
                        ('artifact_compatible_declaration', 'accton,as7772_32x'),
                        ('artifact_serial0_alias', '/soc@ffe000000/serial@11c500'),
                        ('artifact_partition_labels', 'RCW; onie; uboot')]
        limit = 'no installed firmware, safe flashing, vendor endorsement or owned-unit inference'
        self.add('artifact', model='AS7772-32X', vendor='', tier='community',
                 facts=declarations + [('source_scope', 'published DTB-source declarations only'),
                                       ('source_location', 'gistfile1.txt lines 7-8, 75'),
                                       ('configuration_limit', limit)])
        self.add('vendor', model='AS7772-32X', vendor='Accton', tier='vendor',
                 ids=[('regulatory_model', 'RELATED-REG')], facts=[('power', 'vendor PSU claim')])
        report = self.report('AS7772-32X')
        firmware = report['categories']['firmware_boot_software']['statements']
        self.assertEqual({s['fact_name']: s['statement'] for s in firmware}, dict(declarations))
        for statement in firmware:
            self.assertEqual(statement['tier'], 'community')
            self.assertEqual(statement['company_roles'], [])
            self.assertEqual(statement['source_id'], 'artifact')
            self.assertEqual(statement['claim_status'], 'source_statement')
            self.assertEqual(statement['source_url'], 'https://example.org/artifact')
            self.assertEqual(len(statement['sha256']), 64)
        artifact = next(r for r in report['matched_records'] if r['source_id'] == 'artifact')
        self.assertEqual({s['context_record_id'] for s in firmware}, {artifact['record_id']})
        self.assertIn({'name': 'configuration_limit', 'value': limit}, artifact['context_facts'])
        self.assertIn(limit, buying.render_table(report))
        self.assertIn('gistfile1.txt lines 7-8, 75', buying.render_table(report))
        self.assertEqual(self.report('AS7772-32X', 'Accton')['categories']['firmware_boot_software']['status'], 'not_collected')
        for query in ('accton,as7772_32x', 'RELATED-REG', '/soc@ffe000000/serial@11c500'):
            self.assertEqual(self.report(query)['match_status'], 'no_exact_records')

    def test_matched_identity_with_no_practical_facts_still_reports_gaps(self):
        self.add('index', facts=[('source_scope', 'identity only')])
        r = self.report('MODEL-V')
        self.assertEqual(r['match_status'], 'exact_records_found')
        self.assertTrue(all(c['status'] == 'not_collected' for c in r['categories'].values()))
        with self.assertRaises(ValueError):
            self.report('')
        with self.assertRaises(ValueError):
            self.report('MODEL-V', 'not a vendor')

    def test_cli_read_only_bytes_json_and_missing_database(self):
        self.add('a', facts=[('power_description', 'AC')])
        before = self.path.read_bytes()
        for fmt in ('table', 'json'):
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                result = main(['--db', str(self.path), 'buying-report', 'MODEL-V', '--format', fmt])
            self.assertEqual(result, 0)
            if fmt == 'json':
                self.assertEqual(json.loads(out.getvalue())['match_status'], 'exact_records_found')
            self.assertEqual(self.path.read_bytes(), before)
        missing = self.path.parent / 'missing.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertNotEqual(main(['--db', str(missing), 'buying-report', 'MODEL-V']), 0)
        self.assertFalse(missing.exists())


if __name__ == '__main__':
    unittest.main()

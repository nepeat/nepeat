import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import broadcom as b
from devicehunt.catalog import Catalog, digest
from devicehunt import buying, pipeline


class BroadcomTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.row = {'uuid': 123, 'programId': 'server', 'programName': 'Systems / Servers',
                    'title1': 'Oracle', 'title2': 'Oracle Server E5-2c',
                    'description1': ['ESXi 8.0 U3'], 'description2': 'Oracle'}
        self.detail = {'programId': 'server', 'details': [
            {'title': 'Model Details', 'subsections': [{'type': 'kv', 'fields': [
                {'displayKey': 'model', 'fieldValues': ['Oracle Server E5-2c']},
                {'displayKey': 'partner', 'fieldValues': ['Oracle']},
                {'displayKey': 'cpuSeries', 'fieldValues': ['AMD EPYC 9004 Series']},
                {'displayKey': 'notes', 'fieldValues': ['Boot from SD/USB is not supported.<br>Check vendor firmware.']},
            ]}]}]}

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def save(self, alter=None, records=None):
        records = records or [(self.row, self.detail)]
        bodies, refs = {}, {}
        for query in b.QUERIES:
            for phase in ['before', 'after']:
                key = phase + ':' + query
                rows = [copy.deepcopy(row) for row, _ in records] if query == 'Oracle' else []
                bodies[key] = {'totalCount': len(rows), 'results': rows}
                refs[key] = (b.search_url(query), 'GET')
        for row, detail in records:
            key = 'detail:' + str(row['uuid'])
            bodies[key] = copy.deepcopy(detail)
            refs[key] = (b.detail_url(row['uuid']), 'POST')
        if alter:
            alter(bodies)
        bundle = {'schema_version': 1, 'persona': 'live', 'queries': b.QUERIES, 'responses': {}}
        for n, (key, body) in enumerate(bodies.items()):
            p = self.root / (str(n) + '.json')
            p.write_text(json.dumps({'success': True, 'data': body}))
            bundle['responses'][key] = {'path': p.name, 'sha256': digest(p.read_bytes()),
                                        'url': refs[key][0], 'method': refs[key][1]}
        p = self.root / 'bundle.json'
        p.write_text(json.dumps(bundle))
        return p

    def with_bios(self, values):
        detail = copy.deepcopy(self.detail)
        detail['details'] = [s for s in detail['details'] if s['title'] != 'Certification Details']
        detail['details'].append({'title': 'Certification Details', 'subsections': [
            {'type': 'filter', 'fields': [{'displayKey': 'vmwareProductName', 'fieldValues': ['OS A', 'OS B']}]},
            {'type': 'table', 'dependsOn': 'certDetails.releaseDetails.releaseVersion',
             'fieldHeaders': [{'displayKey': 'bios', 'field': 'certDetails.BIOS'}], 'fieldValues': values}]})
        return detail

    def snapshot(self):
        return {table: [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + table + ' ORDER BY 1,2')]
                for table in ('sources', 'records', 'devices', 'facts', 'identifiers', 'affiliations', 'search_index')}

    def test_literal_bios_array_sql_duplicates_raw_types_and_record_isolation(self):
        literal = {'bios': ' AMI <b>001.00 UEFI</b>\n', 'certId': '00017', '_id': 'same',
                   'other': {'null': None, 'number': 1.25, 'text': '001', 'nested': [True, None]}}
        rows = [literal, copy.deepcopy(literal), dict(literal, certId=None), dict(literal, certId=17), dict(literal, certId=17.5)]
        self.detail = self.with_bios(rows)
        second_row = dict(self.row, uuid=124, title2='Oracle Server E5-2L')
        second_detail = self.with_bios([{'bios': 'different unit model', 'certId': 17}])
        second_detail['details'][0]['subsections'][0]['fields'][0]['fieldValues'] = [second_row['title2']]
        path = self.save(records=[(self.row, self.detail), (second_row, second_detail)])
        for _ in range(2):
            self.assertEqual(pipeline.import_source(self.c, 'broadcom', path)['imported'], 2)
        fact = self.c.db.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id WHERE r.source_key='123' AND f.name='compatibility_bios_certification_rows'").fetchone()[0]
        self.assertEqual(json.loads(fact), rows)
        sql_rows = [tuple(r) for r in self.c.db.execute("SELECT j.key,json_extract(j.value,'$.bios'),json_extract(j.value,'$.certId'),json_type(j.value,'$.certId') FROM facts f JOIN records r ON r.id=f.record_id,json_each(f.value) j WHERE r.source_key='123' AND f.name='compatibility_bios_certification_rows' ORDER BY j.key")]
        self.assertEqual([r[0] for r in sql_rows], list(range(5)))
        self.assertEqual([r[3] for r in sql_rows], ['text', 'text', 'null', 'integer', 'real'])
        self.assertEqual(sql_rows[0][1:], sql_rows[1][1:])
        raw = json.loads(self.c.db.execute("SELECT raw_json FROM records WHERE source_key='123'").fetchone()[0])
        self.assertEqual(raw, {'search': self.row, 'detail': self.detail})
        report = buying.run(self.c.db, 'E5-2c', 'Oracle')
        bios = [s for s in report['categories']['firmware_boot_software']['statements'] if s['fact_name'] == 'compatibility_bios_certification_rows']
        self.assertEqual(len(bios), 1)
        self.assertEqual(json.loads(bios[0]['statement']), rows)
        self.assertNotIn('different unit model', buying.render_table(report))
        self.assertFalse(any('release' in r or 'os' in r for r in json.loads(fact)))
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])

    def test_absent_and_empty_bios_tables_are_distinct(self):
        b.import_bundle(self.c, self.save())
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='compatibility_bios_certification_rows'").fetchone()[0], 0)
        self.detail = self.with_bios([])
        b.import_bundle(self.c, self.save())
        self.assertEqual(self.c.db.execute("SELECT value FROM facts WHERE name='compatibility_bios_certification_rows'").fetchone()[0], '[]')

    def test_null_empty_and_numeric_bios_literals_and_unrelated_tables(self):
        values = [{'bios': value, 'certId': value} for value in (None, '', '0007', 7, 7.25)]
        self.detail = self.with_bios(values)
        self.detail['details'][1]['subsections'].append({'type': 'table',
            'fieldHeaders': [{'displayKey': 'other'}], 'fieldValues': [{'other': 'ignore as BIOS'}]})
        b.import_bundle(self.c, self.save())
        fact = self.c.db.execute("SELECT value FROM facts WHERE name='compatibility_bios_certification_rows'").fetchone()[0]
        self.assertEqual(json.loads(fact), values)

    def test_previous_facts_identifiers_roles_and_other_sources_preserved(self):
        b.import_bundle(self.c, self.save())
        before = {t: {tuple(r) for r in self.c.db.execute('SELECT * FROM ' + t)}
                  for t in ('facts', 'identifiers', 'affiliations')}
        self.c.source('unrelated', 'https://example.org', 'vendor', 'test', 'unrelated scope')
        self.c.add('unrelated', '1', 'https://example.org/1', 'Oracle', 'Other exact model',
                   'Other exact model', 'server', {'literal': 'keep'}, [('Oracle', 'brand')],
                   [('alias', 'OTHER')], [('power', '100 W')])
        self.c.finish('unrelated', 1)
        unrelated = {t: [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + t)]
                     for t in ('records', 'facts', 'identifiers', 'affiliations')}
        self.detail = self.with_bios([{'bios': 'AMI 001', 'certId': 1}])
        b.import_bundle(self.c, self.save())
        for table, rows in before.items():
            self.assertTrue(rows.issubset({tuple(r) for r in self.c.db.execute('SELECT * FROM ' + table)}))
        rid = self.c.db.execute("SELECT id FROM records WHERE source_id='unrelated'").fetchone()[0]
        for table, rows in unrelated.items():
            index = 0
            self.assertEqual([r for r in rows if r[index] == rid],
                             [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + table) if r[index] == rid])

    def test_malformed_late_bios_capture_preserves_all_prior_source_state(self):
        self.detail = self.with_bios([{'bios': 'old BIOS', 'certId': 1}])
        pipeline.import_source(self.c, 'broadcom', self.save())
        before = self.snapshot()
        second_row = dict(self.row, uuid=999, title1='Unrelated Vendor', description2='Unrelated Vendor')
        second_detail = copy.deepcopy(self.detail)
        second_detail['details'][0]['subsections'][0]['fields'][1]['fieldValues'] = ['Unrelated Vendor']
        def table(detail):
            return detail['details'][1]['subsections'][1]
        mutations = [lambda d: table(d).update(fieldValues={}),
                     lambda d: table(d).update(fieldHeaders=None),
                     lambda d: table(d).update(fieldHeaders=[]),
                     lambda d: table(d).update(fieldValues=[{'bios': 'new', 'certId': []}]),
                     lambda d: table(d).update(fieldValues=[{'bios': [], 'certId': 1}]),
                     lambda d: table(d).update(fieldValues=[{'bios': 'new'}]),
                     lambda d: table(d).update(fieldValues=[{'bios': 'new', 'certId': float('nan')}]),
                     lambda d: d['details'][1]['subsections'].append(copy.deepcopy(table(d)))]
        for mutation in mutations:
            late = copy.deepcopy(second_detail)
            mutation(late)
            with self.assertRaises(ValueError):
                pipeline.import_source(self.c, 'broadcom', self.save(records=[(self.row, self.detail), (second_row, late)]))
            self.assertEqual(self.snapshot(), before)

    def test_saved_24_record_snapshot_rows_match_every_capture(self):
        path = Path(__file__).resolve().parents[1] / 'data/raw/broadcom-servers-bundle.json'
        records = b.validate_bundle(path)
        self.assertEqual(len(records), 24)
        self.assertEqual(sum(len(b.bios_certification_rows(d)) for _, d, _ in records), 206)
        b.import_bundle(self.c, path)
        for row, detail, _ in records:
            imported = self.c.db.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id WHERE r.source_id=? AND r.source_key=? AND f.name='compatibility_bios_certification_rows'", (b.SOURCE, str(row['uuid']))).fetchone()[0]
            self.assertEqual(json.loads(imported), b.bios_certification_rows(detail))

    def test_scoped_identity_and_configuration_evidence_survives_reimport(self):
        path = self.save()
        for _ in range(2):
            self.assertEqual(b.import_bundle(self.c, path), {'query_records': 1, 'imported': 1})
        self.assertEqual(len(self.c.search('E5-2c', target='Oracle')), 1)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        ids = list(self.c.db.execute('SELECT kind,value FROM identifiers'))
        self.assertIn(('model_search', 'E5-2c'), [tuple(r) for r in ids])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 1)
        notes = self.c.db.execute("SELECT value FROM facts WHERE name='compatibility_notes'").fetchone()[0]
        self.assertIn('not supported.', notes)
        self.assertIn('Check vendor firmware.', notes)
        self.assertNotIn('<br>', notes)

    def test_truncation_changes_and_identity_mismatches_preserve_previous_data(self):
        b.import_bundle(self.c, self.save())
        def truncate(bodies):
            bodies['before:Oracle']['totalCount'] = 2
        def change(bodies):
            bodies['after:Oracle']['results'][0]['description1'] = ['ESXi changed']
        def mismatch(bodies):
            bodies['detail:123']['details'][0]['subsections'][0]['fields'][0]['fieldValues'] = ['Oracle Server E5-2L']
        def wrong_partner(bodies):
            bodies['detail:123']['details'][0]['subsections'][0]['fields'][1]['fieldValues'] = ['Another Company']
        for alteration in [truncate, change, mismatch, wrong_partner]:
            with self.assertRaises(ValueError):
                b.import_bundle(self.c, self.save(alteration))
            self.assertEqual(len(self.c.search('E5-2c')), 1)

    def test_corrupt_or_unexpected_dependencies_rejected(self):
        path = self.save()
        bundle = json.loads(path.read_text())
        ref = bundle['responses']['detail:123']
        (self.root / ref['path']).write_text('{}')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            b.import_bundle(self.c, path)
        path = self.save()
        bundle = json.loads(path.read_text())
        bundle['responses']['detail:123']['method'] = 'GET'
        path.write_text(json.dumps(bundle))
        with self.assertRaisesRegex(ValueError, 'reference'):
            b.import_bundle(self.c, path)

    def test_non_target_partner_is_not_promoted_by_keyword_match(self):
        row = dict(self.row, title1='Unrelated Vendor', description2='Unrelated Vendor')
        detail = copy.deepcopy(self.detail)
        detail['details'][0]['subsections'][0]['fields'][1]['fieldValues'] = ['Unrelated Vendor']
        def alter(bodies):
            for phase in ['before', 'after']:
                bodies[phase + ':Oracle']['results'] = [row]
            bodies['detail:123'] = detail
        with self.assertRaisesRegex(ValueError, 'no exact target'):
            b.import_bundle(self.c, self.save(alter))

    def test_duplicate_record_ids_and_bad_result_types_fail(self):
        for body in [{'totalCount': 2, 'results': [self.row, self.row]},
                     {'totalCount': True, 'results': [self.row]},
                     {'totalCount': 1, 'results': [dict(self.row, programId='io')]},
                     {'totalCount': 1, 'results': [dict(self.row, title1='Amazon')]}]:
            with self.assertRaises(ValueError):
                b.search_rows(body)

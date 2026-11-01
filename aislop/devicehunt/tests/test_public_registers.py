import html
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import lookup, public_registers as registers
from devicehunt.catalog import Catalog, digest


class PublicRegistersTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def bundle(self, kind, rows, change=None):
        config = registers.SOURCES[kind]
        def save(name, body):
            (self.root / name).write_bytes(body)
            return {'path': name, 'sha256': digest(body)}
        def tr(row):
            return '<tr>' + ''.join('<td>' + html.escape(v) + '</td>' for v in row) + '</tr>'
        body = '<title>' + config['title'] + '</title><table id="' + config['table'] + '">'
        body += tr(config['header']) + ''.join(tr(row) for row in rows)
        if kind == 'mauritius-icta': body += tr(config['header'])
        body += '</table>'
        if change == 'heading': body = body.replace('Foreign Certification #', 'Applicant')
        if change == 'unclosed': body = body.replace('</table>', '')
        data = {'schema_version': 1, 'kind': kind, 'source_url': config['url'],
                'expected_rows': len(rows), 'page': save(kind + '.html', body.encode())}
        if kind == 'mauritius-icta':
            response = {'draw': 1, 'recordsTotal': str(len(rows)), 'recordsFiltered': str(len(rows)), 'data': rows}
            if change == 'filtered': response['recordsTotal'] = str(len(rows) + 1)
            if change == 'partial': response['data'] = rows[:-1]
            data['before'] = save(kind + '-before.json', json.dumps(response).encode())
            if change == 'changed': response['data'] = list(reversed(rows))
            data['after'] = save(kind + '-after.json', json.dumps(response).encode())
        if change == 'hash': data['page']['sha256'] = '0' * 64
        if change == 'count': data['expected_rows'] += 1
        if change == 'url': data['source_url'] = 'https://example.org'
        path = self.root / (kind + '-bundle.json'); path.write_text(json.dumps(data))
        return path

    def test_rows_duplicates_groups_and_field_roles_remain_raw(self):
        ntrc = [['Oracle', 'Radio', 'ONE / TWO', 'FCC ID:HED123'],
                ['Oracle', 'Radio', 'ONE / TWO', 'FCC ID:HED123'],
                ['Amazon', 'Drone', 'F100 Ghost', '']]
        icta = [['TA/0001', 'Radio ', 'Accton', 'Straße_100%'],
                ['', '', 'Amazon Data Services', 'MULTI; MODEL']]
        registers.import_bundle(self.c, self.bundle('ntrc-vc', ntrc))
        registers.import_bundle(self.c, self.bundle('mauritius-icta', icta))
        rows = [dict(r) for r in self.c.db.execute('SELECT * FROM public_equipment_rows ORDER BY source_id,row_number')]
        self.assertEqual(len(rows), 5)
        self.assertEqual(rows[0]['make'], 'Accton'); self.assertEqual(rows[0]['brand'], '')
        self.assertEqual(rows[0]['device_description'], 'Radio ')
        self.assertEqual(rows[2]['brand'], 'Oracle'); self.assertEqual(rows[2]['make'], '')
        self.assertEqual(json.loads(rows[-1]['cells_json']), ntrc[-1])
        for table in ('devices', 'records', 'affiliations', 'identifiers', 'hyperscaler_evidence'):
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ' + table).fetchone()[0], 0)
        result = {s['dataset']: s for s in lookup.run(self.c.db, 'ONE / TWO', 1)['datasets']}
        self.assertEqual(result['public_registers']['matches'], 2)
        self.assertTrue(result['public_registers']['truncated'])
        self.assertEqual(result['catalog']['matches'], 0)
        section = next(s for s in lookup.run(self.c.db, 'STRASSE_100%')['datasets'] if s['dataset'] == 'public_registers')
        self.assertEqual(section['matches'], 1)
        self.assertEqual(section['rows'][0]['raw_cells'], icta[0])

    def test_invalid_captures_fail_before_existing_rows_change(self):
        rows = [['TA/1', 'Radio', 'Accton', 'ONE'], ['TA/2', 'Radio', 'Oracle', 'TWO']]
        registers.import_bundle(self.c, self.bundle('mauritius-icta', rows))
        before = {t: [tuple(r) for r in self.c.db.execute('SELECT * FROM ' + t)]
                  for t in ('sources', 'public_equipment_rows')}
        for change in ['hash', 'count', 'url', 'filtered', 'partial', 'changed', 'unclosed']:
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    registers.import_bundle(self.c, self.bundle('mauritius-icta', rows, change))
                for table, expected in before.items():
                    self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM ' + table)], expected)
        with self.assertRaises(ValueError):
            registers.import_bundle(self.c, self.bundle('ntrc-vc', rows, 'heading'))

    def test_reimport_replaces_only_its_own_raw_rows(self):
        rows = [['Oracle', 'Radio', 'ONE', ''], ['Oracle', 'Radio', 'TWO', '']]
        registers.import_bundle(self.c, self.bundle('ntrc-vc', rows))
        registers.import_bundle(self.c, self.bundle('mauritius-icta', [['TA/1', 'Radio', 'Other', 'THREE']]))
        self.c.source('other', 'https://example.org', 'vendor', 'fixture', 'fixture')
        self.c.add('other', 'kept', 'https://example.org', 'Edge-Core', 'KEPT', 'Kept', 'test', {})
        self.c.finish('other', 1)
        path = self.bundle('ntrc-vc', rows[:1])
        registers.import_bundle(self.c, path); registers.import_bundle(self.c, path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM public_equipment_rows').fetchone()[0], 2)
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT model FROM devices')], ['KEPT'])

    def test_nonstring_cells_and_invalid_counts_are_rejected(self):
        for value in [None, 1, {}, True]:
            with self.assertRaises(ValueError):
                registers.api_rows(json.dumps({'draw': 1, 'recordsTotal': 1, 'recordsFiltered': 1,
                                               'data': [['1', '2', '3', value]]}))
        for value in [True, -1, '1.0', None]:
            with self.assertRaises(ValueError):
                registers.api_rows(json.dumps({'draw': 1, 'recordsTotal': value, 'recordsFiltered': 1,
                                               'data': [['1', '2', '3', '4']]}))

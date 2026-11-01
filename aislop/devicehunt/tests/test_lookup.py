import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import anatel, lookup
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main


class LookupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / 'db.sqlite'
        self.c = Catalog(self.path)
        for sid in ['seller', 'anatel-products', 'icasa-2025', 'bahrain-tra', 'fcc-grantees']:
            self.c.source(sid, 'https://example.org/' + sid, 'marketplace' if sid == 'seller' else 'regulatory',
                          'fixture', 'Fixture scope; not production evidence')
        self.c.add('seller', 'a', 'https://example.org/item', 'Edge-Core', 'Shared-Model', 'Shared-Model',
                   'switch', {}, [('Edge-Core', 'seller_reported_brand')],
                   identifiers=[('ul_document', 'LISTING-REF')], facts=[('power', '100% load')])
        for n, model in enumerate(['Shared-Model', 'Shared-Model-DC-42', 'Shared-Model-DC-42', 'Straße_100%'], 1):
            values = {key: '' for key, _ in anatel.FIELDS}
            values.update(model=model, manufacturer='Different Manufacturer', approval_number='0000123')
            self.c.db.execute('INSERT INTO anatel_equipment VALUES (' + ','.join('?' for _ in range(23)) + ')',
                              ['anatel-products', n] + [values[key] for key, _ in anatel.FIELDS])
        self.c.db.execute('INSERT INTO equipment_register VALUES (?,?,?,?,?,?,?,?,?,?)',
                          ['icasa-2025', 42, 7, None, None, None, None, 'review_required',
                           json.dumps(['Shifted', 'Shared-Model', 'Straße_100%']), 'https://example.org/pdf#page=42'])
        self.c.db.execute('INSERT INTO bahrain_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?)',
                          ['ABC0001', 'Bahrain Company', 'Shared-Model-DC-42', 'Switch', '', '', '', '', '', '', 'bahrain-tra'])
        self.c.db.execute('INSERT INTO fcc_grantees VALUES (?,?,?,?,?,?,?,?,?,?,?)',
                          ['ZZZ', 'Shared-Model Company', '', '', '', '', 'US', '', '', '01/01/2000', 'fcc-grantees'])
        self.c.finish('seller', 1)

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def run_lookup(self, query='Shared-Model', limit=20):
        with sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            return lookup.run(db, query, limit)

    def test_cross_source_matches_keep_units_roles_duplicates_and_raw_rows(self):
        result = self.run_lookup()
        sections = {r['dataset']: r for r in result['datasets']}
        self.assertEqual({k: v['matches'] for k, v in sections.items() if k != 'inspur_support'},
                         {'catalog': 1, 'fcc_sitemaps': 0, 'fcc_company': 0, 'japan': 0, 'public_registers': 0, 'ghana': 0, 'nigeria': 0, 'ised': 0, 'energystar_datacenter': 0, 'energystar_server_archives': 0, 'energystar_servers': 0, 'rra': 0, 'fcc_details': 0, 'fcc_excerpts': 0, 'documents': 0, 'fcc_ids': 0, 'pci': 0, 'ieee': 0, 'ntra': 0, 'ncc': 0, 'bsmi': 0, 'bsmi_product_certificates': 0, 'bsmi_voluntary_certificates': 0, 'bsmi_declarations': 0, 'tec': 0, 'anatel': 3, 'icasa': 1, 'bahrain': 1, 'fcc_grantees': 1})
        self.assertEqual(sections['inspur_support']['matches'], 0)
        self.assertEqual(sections['inspur_support']['state'], 'not_loaded')
        self.assertEqual(sections['catalog']['rows'][0]['evidence'][0]['tier'], 'marketplace')
        self.assertEqual(sections['catalog']['rows'][0]['identity_text_matches'], [{'kind': 'model', 'value': 'Shared-Model'}])
        self.assertIsNone(sections['icasa']['rows'][0]['model'])
        self.assertEqual(sections['icasa']['rows'][0]['parse_status'], 'review_required')
        self.assertIn('Shared-Model', sections['icasa']['rows'][0]['raw_cells'])
        self.assertEqual(sections['fcc_grantees']['unit'], 'historical company registration')
        self.assertTrue(all(r['source_url'] for key in sections if key != 'catalog' for r in sections[key]['rows']))

    def test_limits_report_totals_and_truncation_per_dataset(self):
        for limit, shown in [(0, 0), (1, 1), (-1, 3)]:
            section = next(s for s in self.run_lookup(limit=limit)['datasets'] if s['dataset'] == 'anatel')
            self.assertEqual(section['matches'], 3)
            self.assertEqual(section['shown'], shown)
            self.assertEqual(section['truncated'], shown < 3)
        with self.assertRaises(ValueError): self.run_lookup(limit=-2)
        with self.assertRaises(ValueError): self.run_lookup('  ')

    def test_unicode_casefold_punctuation_and_json_cells_are_literal(self):
        sections = {s['dataset']: s for s in self.run_lookup('STRASSE_100%')['datasets']}
        self.assertEqual(sections['anatel']['matches'], 1)
        self.assertEqual(sections['icasa']['matches'], 1)
        self.assertTrue(all(s['matches'] == 0 for s in self.run_lookup("' OR 1=1 --")['datasets']))
        self.assertTrue(all(s['matches'] == 0 for s in self.run_lookup('"')['datasets']))

    def test_registry_predicate_runs_once_per_row_with_all_limit_modes(self):
        original = lookup.contains
        for query in ['Shared-Model', 'absent-model']:
            for limit in [0, 1, 20, -1]:
                with self.subTest(query=query, limit=limit):
                    evaluated = []

                    def track(value, needle):
                        if 'Different Manufacturer' in value:
                            evaluated.append(value)
                        return original(value, needle)

                    with patch.object(lookup, 'contains', track):
                        result = self.run_lookup(query, limit)
                    self.assertEqual(len(evaluated), 4)
                    section = next(s for s in result['datasets'] if s['dataset'] == 'anatel')
                    expected = 3 if query == 'Shared-Model' else 0
                    self.assertEqual(section['matches'], expected)
                    self.assertEqual(section['shown'], expected if limit == -1 else min(limit, expected))
                    self.assertTrue(all('_lookup_match_count' not in row
                                        for s in result['datasets'] for row in s['rows']))

    def test_searching_notes_or_listing_numbers_is_not_an_identity_match(self):
        for query in ['100% load', 'LISTING-REF']:
            section = self.run_lookup(query)['datasets'][0]
            self.assertEqual(section['matches'], 1)
            self.assertEqual(section['rows'][0]['identity_text_matches'], [])

    def test_catalog_membership_keeps_identity_counts_with_duplicate_stale_or_missing_index_rows(self):
        did=self.c.db.execute('SELECT id FROM devices').fetchone()[0]
        for key in (did,did,'missing-device'):
            self.c.db.execute('INSERT INTO search_index(device_id,text) VALUES (?,?)',(key,'Extra_100%'))
        self.c.db.commit()
        section=self.run_lookup('Extra_100%')['datasets'][0]
        self.assertEqual(section['matches'],1)
        self.assertEqual(len(section['rows']),1)
        self.assertEqual(section['rows'][0]['identity_text_matches'],[])
        self.c.db.execute('DELETE FROM search_index WHERE device_id=?',(did,));self.c.db.commit()
        self.assertEqual(self.run_lookup('Shared-Model')['datasets'][0]['matches'],1)
        self.assertEqual(self.run_lookup('Extra_100%')['datasets'][0]['matches'],0)

    def test_missing_dataset_is_distinguished_from_loaded_zero_matches(self):
        self.c.db.execute('DELETE FROM fcc_grantees')
        self.c.db.execute("DELETE FROM sources WHERE id='fcc-grantees'")
        self.c.db.commit()
        sections = {s['dataset']: s for s in self.run_lookup('no match')['datasets']}
        self.assertEqual(sections['fcc_grantees']['state'], 'not_loaded')
        self.assertEqual(sections['anatel']['state'], 'loaded')
        self.assertEqual(sections['anatel']['matches'], 0)

    def test_cli_reads_existing_database_without_creating_or_changing_files(self):
        before = self.path.read_bytes()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(main(['--db', str(self.path), 'lookup', 'Shared-Model', '--limit', '1', '--format', 'json']), 0)
        self.assertEqual(json.loads(out.getvalue())['limit_per_dataset'], 1)
        self.assertEqual(before, self.path.read_bytes())
        absent = self.path.parent / 'absent.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db', str(absent), 'lookup', 'test']), 1)
        self.assertFalse(absent.exists())

    def test_lookup_does_not_roll_back_callers_pending_work(self):
        self.c.db.execute("UPDATE devices SET title='Pending title'")
        lookup.run(self.c.db, 'Pending title')
        self.assertTrue(self.c.db.in_transaction)
        self.assertEqual(self.c.db.execute('SELECT title FROM devices').fetchone()[0], 'Pending title')
        self.c.db.rollback()

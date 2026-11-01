import unittest
import tempfile
import contextlib
import io
import json
import csv
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import specifications
from devicehunt.__main__ import main

from devicehunt.specifications import product_statements


class SpecificationTests(unittest.TestCase):
    def test_features_scope_preserves_alternatives_and_excludes_neighbors(self):
        body = '<p>Compatible OTHER PSU 1200W</p><script>ONIE</script>'
        body += '<div id="features"><ul><li>Redundant <b>AC</b> or -48V DC PSUs (SKU dependent)</li>'
        body += '<li>Front-to-back and back-to-front airflow SKUs</li><li>1RU rack mount</li>'
        body += '<li>ONIE is pre-loaded; a compatible NOS must be installed separately.</li></ul></div>'
        body += '<div id="compatible"><p>OTHER MODEL 2400W PSU</p></div>'
        facts = product_statements(body)
        self.assertIn(('power_description', 'Redundant AC or -48V DC PSUs (SKU dependent)'), facts)
        self.assertIn(('cooling_description', 'Front-to-back and back-to-front airflow SKUs'), facts)
        self.assertIn(('physical_description', '1RU rack mount'), facts)
        self.assertEqual(len(facts), 4)
        self.assertFalse(any('OTHER' in v for _,v in facts))

    def test_multiple_disjoint_feature_blocks_deduplicate(self):
        body = '<div id="features"><p>Fanless cooling</p><p>ONIE<br/>installer</p></div>'
        body += '<div id="features"><p>ONIE installer</p><script>AC PSU</script></div>'
        facts = product_statements(body)
        self.assertEqual(facts.count(('software_installation_description', 'ONIE installer')), 1)
        self.assertFalse(any(k == 'power_description' for k,v in facts))
        self.assertEqual(product_statements('<p>AC PSU</p>'), [])

    def test_unclosed_or_nested_scope_is_rejected(self):
        for body in ['<div id="features"><p>AC PSU</p>',
                     '<div id="features"><div id="features"></div></div>',
                     '<div id="features"><p>AC ' + 'x'*4000 + '</p></div>']:
            with self.assertRaises(ValueError):
                product_statements(body)


class SpecificationSearchTests(unittest.TestCase):
    def test_search_keeps_record_provenance_and_does_not_derive_numeric_power(self):
        with tempfile.TemporaryDirectory() as directory:
            c = Catalog(Path(directory) / 'db.sqlite')
            try:
                c.source('vendor-page', 'https://example.org/model', 'vendor', 'test', 'test')
                c.add('vendor-page', 'MODEL', 'https://example.org/model', 'Edge-Core', 'MODEL', 'MODEL', 'switch', {},
                      [('Edge-Core', 'brand')], facts=[('power_description', 'Optical transceivers up to 23W; AC or DC chassis options.')])
                c.finish('vendor-page', 1)
                rows = specifications.search(c, 'MODEL', 'Edge-Core', -1)
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0]['source_id'], 'vendor-page')
                self.assertEqual(rows[0]['tier'], 'vendor')
                self.assertIn('Optical transceivers', rows[0]['statement'])
                self.assertNotIn('power_watts', rows[0])
                self.assertEqual(specifications.search(c, 'MODEL', 'Oracle'), [])
                self.assertEqual(specifications.search(c, limit=0), [])
                c.source('accessory', 'https://example.org/accessory', 'vendor', 'test', 'test')
                c.add('accessory', 'OPTIC', 'https://example.org/accessory', 'Edge-Core', 'OPTIC', 'OPTIC', 'optic', {},
                      [('Edge-Core', 'brand')], facts=[('power_description', '3.3V module compatible with MODEL')])
                c.finish('accessory', 1)
                self.assertEqual(len(specifications.search(c, 'MODEL')), 2)
                exact = specifications.search(c, 'model', exact_model=True)
                self.assertEqual([r['model'] for r in exact], ['MODEL'])
                with self.assertRaises(ValueError):
                    specifications.search(c, '', exact_model=True)
            finally:
                c.db.close()


class PartNumberSpecificationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'db.sqlite'
        self.c = Catalog(self.path)
        entries = [
            ('front', 'Celestica', 'part_number', 'R4023-F9102-A0', 'Front configuration'),
            ('back', 'Celestica', 'part_number', 'R4023-F9003-A0', 'Back configuration'),
            ('family', 'Celestica', None, None, 'Family alternatives'),
            ('alias', 'Celestica', 'alias', 'R4023-F9102-A0', 'Alias-only statement'),
            ('other-vendor', 'Oracle', 'part_number', 'R4023-F9102-A0', 'Other association'),
            ('seller', 'Celestica', 'part_number', 'R4023-F9102-A0', 'Seller claim'),
            ('no-specs', 'Celestica', 'part_number', 'EMPTY-100', None),
            ('unicode', 'Celestica', 'part_number', 'Straße_100%', 'Literal Unicode part'),
        ]
        for sid, company, kind, value, statement in entries:
            tier = 'marketplace' if sid == 'seller' else 'vendor'
            self.c.source(sid, 'https://example.org/' + sid, tier, 'fixture', 'fixture')
            self.c.add(sid, sid, 'https://example.org/' + sid, 'Celestica', 'DS4000', 'DS4000', 'switch', {},
                       [(company, 'seller_reported_brand' if sid == 'seller' else 'brand')],
                       [(kind, value)] if kind else [],
                       [('power_description', statement)] if statement else [])
            self.c.finish(sid, 1)

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def test_part_number_vendor_and_statements_are_bound_to_same_record(self):
        rows = specifications.search(self.c, 'r4023-f9102-a0', 'Celestica', exact_part_number=True)
        self.assertEqual({r['source_id'] for r in rows}, {'front', 'seller'})
        self.assertEqual({r['tier'] for r in rows}, {'vendor', 'marketplace'})
        self.assertTrue(all(r['matched_part_numbers'] == ['R4023-F9102-A0'] for r in rows))
        self.assertEqual(len({r['record_id'] for r in rows}), 2)
        # A family-level association must not lend its vendor to a different record.
        other = specifications.search(self.c, 'R4023-F9102-A0', 'Oracle', exact_part_number=True)
        self.assertEqual([r['source_id'] for r in other], ['other-vendor'])
        unfiltered = specifications.search(self.c, 'R4023-F9102-A0', exact_part_number=True)
        self.assertEqual({r['source_id'] for r in unfiltered}, {'front', 'seller', 'other-vendor'})
        family = specifications.search(self.c, 'DS4000', exact_model=True)
        self.assertEqual(len(family), 7)
        self.assertTrue(all('matched_part_numbers' not in r for r in family))

    def test_literal_complete_identifier_does_not_require_search_index_or_fallback(self):
        self.c.db.execute('DELETE FROM search_index'); self.c.db.commit()
        rows = specifications.search(self.c, ' STRASSE_100% ', exact_part_number=True)
        self.assertEqual([r['source_id'] for r in rows], ['unicode'])
        self.assertEqual(rows[0]['matched_part_numbers'], ['Straße_100%'])
        for query in ['R4023-F9102', 'R4023-F9102-A0-EXTRA', 'EMPTY-100', 'DS4000', "' OR 1=1 --"]:
            self.assertEqual(specifications.search(self.c, query, exact_part_number=True), [])

    def test_reused_part_number_preserves_distinct_revision_claims(self):
        self.c.source('revision', 'https://example.org/revision', 'vendor', 'fixture', 'fixture')
        self.c.add('revision', 'B', 'https://example.org/revision', 'Celestica', 'DS4000', 'DS4000', 'switch', {},
                   [('Celestica', 'brand')], [('part_number', 'R4023-F9102-A0')],
                   [('power_description', 'Conflicting revision claim')], revision='B')
        self.c.finish('revision', 1)
        rows = specifications.search(self.c, 'R4023-F9102-A0', 'Celestica', exact_part_number=True)
        self.assertEqual({r['revision'] for r in rows}, {'', 'B'})
        self.assertEqual(len({r['device_id'] for r in rows}), 2)
        self.assertEqual({r['source_id'] for r in rows}, {'front', 'seller', 'revision'})

    def test_modes_limits_and_invalid_inputs(self):
        expected = specifications.search(self.c, 'R4023-F9102-A0', limit=-1, exact_part_number=True)
        for limit in [0, 1, 20]:
            self.assertEqual(specifications.search(self.c, 'R4023-F9102-A0', limit=limit, exact_part_number=True), expected[:limit])
        for kwargs in [dict(query=''), dict(query='  '), dict(query='R4023-F9102-A0', exact_model=True),
                       dict(query='R4023-F9102-A0', target='Unknown'), dict(query='R4023-F9102-A0', limit=-2)]:
            with self.assertRaises(ValueError):
                specifications.search(self.c, exact_part_number=True, **kwargs)

    def test_cli_json_is_read_only_and_rejects_combined_exact_modes(self):
        before = self.path.read_bytes(); output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = main(['--db', str(self.path), 'specs', 'R4023-F9102-A0', '--vendor', 'Celestica',
                           '--exact-part-number', '--format', 'json'])
        self.assertEqual(status, 0)
        self.assertEqual({r['source_id'] for r in json.loads(output.getvalue())}, {'front', 'seller'})
        self.assertEqual(self.path.read_bytes(), before)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as caught:
            main(['--db', str(self.path), 'specs', 'DS4000', '--exact-part-number', '--exact-model'])
        self.assertEqual(caught.exception.code, 2)

    def test_broad_and_exact_model_vendor_filters_use_statement_record(self):
        for exact in [False, True]:
            with self.subTest(exact_model=exact):
                rows = specifications.search(self.c, 'DS4000', 'Celestica', limit=-1, exact_model=exact)
                self.assertEqual({r['source_id'] for r in rows},
                                 {'front', 'back', 'family', 'alias', 'seller', 'unicode'})
                self.assertTrue(all(r['record_id'] and r['revision']=='' for r in rows))
                other = specifications.search(self.c, 'DS4000', 'Oracle', exact_model=exact)
                self.assertEqual([r['source_id'] for r in other], ['other-vendor'])
                for limit in [0, 1, 2]:
                    self.assertEqual(specifications.search(self.c, 'DS4000', 'Celestica', limit,
                                                         exact_model=exact), rows[:limit])
        self.assertEqual({r['source_id'] for r in specifications.search(self.c, target='Oracle')}, {'other-vendor'})
        for exact in [False, True]:
            with self.assertRaises(ValueError):
                specifications.search(self.c, 'DS4000', 'Unknown', exact_model=exact)

    def test_exact_alias_does_not_borrow_configuration_or_vendor(self):
        rows = specifications.search(self.c, 'R4023-F9102-A0', exact_model=True)
        self.assertEqual([r['source_id'] for r in rows], ['alias'])
        self.assertEqual(specifications.search(self.c, 'R4023-F9102-A0', 'Oracle', exact_model=True), [])
        self.c.add('no-specs', 'empty-alias', 'https://example.org/empty-alias', 'Celestica', 'DS4000',
                   'DS4000', 'switch', {}, [('Celestica', 'brand')], [('alias', 'EMPTY-ALIAS')])
        self.c.add('other-vendor', 'alias-collision', 'https://example.org/collision', 'Oracle', 'OTHER-100',
                   'OTHER-100', 'switch', {}, [('Oracle', 'brand')], [('model_search', 'R4023-F9102-A0')],
                   [('power_description', 'Different product, same search alias')], revision='B')
        self.c.finish('other-vendor', 2)
        self.assertEqual(specifications.search(self.c, 'EMPTY-ALIAS', exact_model=True), [])
        rows = specifications.search(self.c, 'R4023-F9102-A0', exact_model=True)
        self.assertEqual({(r['model'],r['source_id'],r['revision']) for r in rows},
                         {('DS4000','alias',''),('OTHER-100','other-vendor','B')})
        oracle = specifications.search(self.c, 'R4023-F9102-A0', 'Oracle', exact_model=True)
        self.assertEqual([r['statement'] for r in oracle], ['Different product, same search alias'])

    def test_exact_model_and_alias_are_literal_without_discovery_index(self):
        self.c.add('unicode', 'literal-model', 'https://example.org/literal', 'Celestica', 'Straße_100%',
                   'literal model', 'switch', {}, [('Celestica', 'brand')],
                   [('manufacturer_model', 'Straße%_ALT')], [('power_description', 'Literal-only model')])
        self.c.finish('unicode', 2)
        self.c.db.execute('DELETE FROM search_index'); self.c.db.commit()
        for query in [' STRASSE_100% ', ' STRASSE%_ALT ']:
            rows = specifications.search(self.c, query, 'Celestica', exact_model=True)
            self.assertEqual([r['statement'] for r in rows], ['Literal-only model'])
        self.assertEqual([r['source_id'] for r in specifications.search(self.c, 'R4023-F9102-A0', exact_model=True)], ['alias'])
        for query in ['STRASSE_100', 'STRASSE_100%-EXTRA', 'STRASSE%_AL', 'Literal-only model', "' OR 1=1 --"]:
            self.assertEqual(specifications.search(self.c, query, exact_model=True), [])

    def test_scoped_model_cli_json_csv_preserve_provenance_and_database(self):
        before = self.path.read_bytes()
        for fmt in ['json', 'csv']:
            for query, vendor, sources in [('DS4000','Oracle',{'other-vendor'}),
                                           ('R4023-F9102-A0','Celestica',{'alias'})]:
                output=io.StringIO()
                with contextlib.redirect_stdout(output):
                    status=main(['--db',str(self.path),'specs',query,'--vendor',vendor,'--exact-model','--format',fmt])
                self.assertEqual(status,0)
                rows=json.loads(output.getvalue()) if fmt=='json' else list(csv.DictReader(io.StringIO(output.getvalue())))
                self.assertEqual({r['source_id'] for r in rows},sources)
                self.assertTrue(all(r['record_id'] and 'revision' in r for r in rows))
        self.assertEqual(self.path.read_bytes(),before)

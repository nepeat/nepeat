import contextlib
import copy
import gc
import io
import json
import sqlite3
import tempfile
import unittest
import warnings
from pathlib import Path

from devicehunt import brand_families as bf
from devicehunt import listings, lookup, model_collisions
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog

SHA = 'a' * 64
EVIDENCE = {'url': 'https://example.org/e', 'local_path': 'data/raw/none/e.pdf', 'sha256': SHA, 'statement': 'fixture statement',
            'as_of': '2025-12-31', 'captured_on': '2026-10-11'}


def member(brand, relations=('subsidiary_brand',), **kw):
    row = {'brand': brand, 'relations': list(relations), 'evidence': [copy.deepcopy(EVIDENCE)], 'date': '2025-12-31',
           'reviewer': None, 'review_status': 'pending', 'note': 'n'}
    row.update(kw)
    return row


def table_data():
    return {'schema_version': 1, 'families': [
        {'id': 'quanta', 'name': 'Quanta', 'group_entity': 'Quanta Computer Inc.', 'members': [
            member('Quanta', ['own_brand']), member('QCT', ['abbreviation', 'subsidiary_brand']),
            member('Quanta Cloud Technology')]},
        {'id': 'oracle', 'name': 'Oracle', 'group_entity': 'Oracle Corporation', 'members': [
            member('Oracle', ['own_brand']), member('Sun Microsystems', ['acquired_company'])]}]}


def table(data=None):
    return bf.parse(json.dumps(data or table_data()))


class ValidationTests(unittest.TestCase):
    def bad(self, mutate, fragment):
        data = table_data()
        mutate(data)
        with self.assertRaises(bf.FamilyFileError) as ctx:
            table(data)
        self.assertIn(fragment, str(ctx.exception))

    def test_valid_table_and_metadata(self):
        t = table()
        meta = t.metadata()
        self.assertEqual((meta['rows'], meta['rows_pending_review'], meta['display_only']), (5, 5, True))
        self.assertEqual(len(meta['sha256']), 64)
        self.assertEqual(meta['families'], ['oracle', 'quanta'])

    def test_schema_and_required_fields(self):
        with self.assertRaises(bf.FamilyFileError):
            bf.parse('{"schema_version": 2, "families": []}')
        with self.assertRaises(bf.FamilyFileError):
            bf.parse('not json')
        self.bad(lambda d: d['families'][0]['members'][0].pop('evidence'), 'evidence')
        self.bad(lambda d: d['families'][0]['members'][0].update(evidence=[]), 'at least one evidence')
        for field in ('url', 'local_path', 'sha256', 'statement', 'captured_on'):
            self.bad(lambda d, f=field: d['families'][0]['members'][0]['evidence'][0].pop(f), field)
        self.bad(lambda d: d['families'][0]['members'][0]['evidence'][0].update(sha256='xyz'), 'sha256')
        self.bad(lambda d: d['families'][0]['members'][0].pop('date'), 'date')
        self.bad(lambda d: d['families'][0]['members'][0].pop('reviewer'), 'reviewer')
        self.bad(lambda d: d['families'][0]['members'][0].update(relations=['manufacturer']), 'unknown relation')
        self.bad(lambda d: d['families'][0]['members'][0].update(review_status='maybe'), 'review_status')

    def test_url_must_be_a_clean_http_url(self):
        self.bad(lambda d: d['families'][0]['members'][0]['evidence'][0].update(url='https://www.qct.io/ (see note)'), 'whitespace')
        self.bad(lambda d: d['families'][0]['members'][0]['evidence'][0].update(url='ftp://example.org/x'), 'http(s)')

    def test_undated_evidence_needs_a_note(self):
        self.bad(lambda d: d['families'][0]['members'][0]['evidence'][0].update(as_of=None), 'date_note')
        data = table_data()
        data['families'][0]['members'][0]['evidence'][0].update(as_of=None, date_note='undated page')
        table(data)

    def test_reviewer_rules(self):
        self.bad(lambda d: d['families'][0]['members'][0].update(reviewer='Someone'), 'pending rows must have reviewer null')
        self.bad(lambda d: d['families'][0]['members'][0].update(review_status='accepted'), 'named reviewer')
        data = table_data()
        data['families'][0]['members'][0].update(review_status='accepted', reviewer='R', reviewed_on='2026-10-12')
        self.assertEqual(table(data).rows_pending, 4)

    def test_brand_cannot_be_in_two_families(self):
        self.bad(lambda d: d['families'][1]['members'].append(member('QCT.')), 'already belongs')
        self.bad(lambda d: d['families'].append(dict(d['families'][0])), 'duplicate family id')

    def test_missing_catalog_brand_is_flagged(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        c = Catalog(Path(tmp.name) / 'c.sqlite')
        c.source('s', 'https://example.org', 'vendor', 'l', 'c')
        c.add('s', 'k', 'u', 'ORACLE', 'X1', 't', 'server', {}, [])
        c.finish('s', 1)
        report = bf.check_catalog(table(), c.db)
        c.db.close()
        self.assertEqual(report['present']['Oracle']['catalog_strings'], ['ORACLE'])
        self.assertEqual(sorted(report['missing']), ['QCT', 'Quanta', 'Quanta Cloud Technology', 'Sun Microsystems'])

    def test_evidence_hash_verification(self):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: [p.unlink() for p in root.rglob('*') if p.is_file()])
        (root / 'data/raw/none').mkdir(parents=True)
        data = table_data()
        for fam in data['families']:
            for m in fam['members']:
                m['evidence'][0]['local_path'] = 'data/raw/none/absent.pdf'
        result = bf.verify_evidence(table(data), root)
        self.assertEqual((result['ok'], result['absent'], result['mismatch']), (0, ['data/raw/none/absent.pdf'], []))
        (root / 'data/raw/none/absent.pdf').write_bytes(b'x')
        self.assertEqual(bf.verify_evidence(table(data), root)['mismatch'], ['data/raw/none/absent.pdf'])


class FamilyOfAndAliasRuleTests(unittest.TestCase):
    def test_family_of_uses_catalog_normalisation(self):
        t = table()
        self.assertEqual([t.family_of(b) for b in ('QCT', 'qct', 'Quanta Cloud Technology', 'ORACLE', 'Sun  Microsystems')],
                         ['quanta', 'quanta', 'quanta', 'oracle', 'oracle'])
        self.assertIsNone(t.family_of('Sun Fire'))
        self.assertIsNone(t.family_of('Quanta Microsystems'))
        self.assertIsNone(t.family_of(''))
        self.assertEqual(t.members('quanta'), ['QCT', 'Quanta', 'Quanta Cloud Technology'])

    def test_rejected_rows_are_ignored(self):
        data = table_data()
        data['families'][0]['members'][1].update(review_status='rejected', reviewer='R')
        self.assertIsNone(table(data).family_of('QCT'))

    def test_module_level_family_of_follows_default_path(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        path = Path(tmp.name) / 'f.json'
        path.write_text(json.dumps(table_data()))
        old = bf.DEFAULT_PATH
        self.addCleanup(setattr, bf, 'DEFAULT_PATH', old)
        bf.DEFAULT_PATH = path
        self.assertEqual(bf.family_of('QCT'), 'quanta')
        bf.DEFAULT_PATH = Path(tmp.name) / 'missing.json'
        self.assertIsNone(bf.family_of('QCT'))
        self.assertIsNone(bf.load_default())

    def test_alias_rule_is_the_model_collisions_rule(self):
        self.assertEqual(bf.alias_key('Oracle', ' Oracle   SPARC S7-2'), ('oracle', 'sparc s7-2'))
        self.assertEqual(bf.alias_key('ORACLE', 'SPARC S7-2'), ('oracle', 'sparc s7-2'))
        self.assertEqual(bf.alias_key('Inspur Corporation', 'Inspur NF1'), ('inspurcorporation', 'nf1'))
        self.assertEqual(bf.alias_key('Inspur', 'Inspurious NF1'), ('inspur', 'inspurious nf1'))
        self.assertEqual(bf.alias_key('Oracle', 'Oracle'), ('oracle', 'oracle'))
        # Own brand only: another family member's name is never stripped.
        self.assertEqual(bf.alias_key('Sun Microsystems', 'Oracle SPARC S7-2'), ('sunmicrosystems', 'oracle sparc s7-2'))
        for brand, model in [('Oracle', 'Oracle SPARC S7-2'), ('Quanta', 'Quanta QuantaGrid D51B'), ('X Y', 'X Y-1')]:
            self.assertEqual(bf.alias_key(brand, model)[1], model_collisions.stripped_key(brand, model)[0])


class DisplayOnlyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.path = self.root / 'db.sqlite'
        c = Catalog(self.path)
        c.source('v', 'https://example.org/v', 'vendor', 'l', 'c')
        c.source('s', 'https://example.org/s', 'certification_body', 'l', 'c')
        rows = [('Quanta', 'QuantaGrid D43K-1U', 'server', 'v', 'Quanta'), ('Quanta Cloud Technology', 'QuantaGrid  d43k-1u', 'server', 'v', 'Quanta'),
                ('QCT', 'QuantaGrid D52B-1U', 'server', 'v', 'Quanta'), ('Quanta', 'QuantaGrid D52B-1U', 'server', 's', 'Quanta'),
                ('Inspur', 'NF5180A7', 'server', 'v', 'Inspur'), ('Inspur Corporation', 'Inspur NF5180A7', 'server', 'v', 'Inspur'),
                ('Oracle', 'SPARC S7-2', 'server', 'v', 'Oracle'), ('Oracle', 'Oracle SPARC S7-2', 'server', 'v', 'Oracle'),
                ('Sun Microsystems', 'SPARC S7-2', 'server', 'v', 'Sun Microsystems'), ('Accton', 'AS7726-32X', 'switch', 'v', 'Accton'),
                # one model string shared by an unmapped, a mapped and another unmapped brand (ordering mutants)
                ('Accton', 'X4470', 'switch', 'v', 'Accton'), ('Oracle', 'X4470', 'server', 'v', 'Oracle'),
                ('Sun Fire', 'X4470', 'server', 'v', 'Sun Microsystems')]
        for i, (brand, model, kind, sid, company) in enumerate(rows):
            c.add(sid, 'k%d' % i, 'https://example.org/%d' % i, brand, model, brand + ' ' + model, kind, {},
                  [(company, 'manufacturer')])
        c.finish('v', 12); c.finish('s', 1)
        c.db.close()
        self.family_path = self.root / 'families.json'
        self.family_path.write_text(json.dumps(table_data()))
        self.table = bf.load(self.family_path)
        self.db = sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True)
        self.db.row_factory = sqlite3.Row
        self.addCleanup(self.db.close)

    def strip(self, value):
        drop = {'brand_family', 'family_members_hit', 'same_family_candidates', 'brand_families', 'family', 'groups_same_family'}
        if isinstance(value, dict):
            return {k: self.strip(v) for k, v in value.items() if k not in drop}
        if isinstance(value, list):
            return [self.strip(v) for v in value]
        return value

    def cli(self, *args, family_file):
        old = bf.DEFAULT_PATH
        bf.DEFAULT_PATH = family_file
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                self.assertEqual(main(['--db', str(self.path), *args]), 0)
        finally:
            bf.DEFAULT_PATH = old
            # __main__ opens `with sqlite3.connect(...)`, which does not close; finalise now so the
            # ResourceWarning cannot surface inside a later test's stderr capture.
            with warnings.catch_warnings():
                warnings.simplefilter('ignore')
                gc.collect()
        return out.getvalue()

    def test_vendor_inventory_is_byte_identical_with_and_without_the_family_file(self):
        missing = self.root / 'absent.json'
        for args in (['vendor-inventory', '--scope', 'all', '--format', 'json', '--limit', '-1'],
                     ['vendor-inventory', 'Quanta', '--scope', 'all', '--format', 'csv', '--limit', '-1'],
                     ['vendor-inventory', 'Oracle', '--scope', 'all'],
                     ['vendor-inventory', '--scope', 'factory', '--format', 'json']):
            with_file = self.cli(*args, family_file=self.family_path)
            without = self.cli(*args, family_file=missing)
            self.assertEqual(with_file.encode(), without.encode(), args)
            self.assertNotIn('brand_famil', with_file)
        data = json.loads(self.cli('vendor-inventory', '--scope', 'all', '--format', 'json', '--limit', '-1', family_file=self.family_path))
        self.assertEqual({r['target'] for r in data['rows']} >= {'Quanta', 'Oracle'}, True)
        # Target resolution is by the catalog's own aliases, never by the family table.
        from devicehunt.catalog import target_for
        self.assertIsNone(target_for('QCT'))
        self.assertIsNone(target_for('Sun Microsystems'))
        source = (Path(bf.__file__).parent / 'vendor_inventory.py').read_text()
        self.assertNotIn('brand_families', source)
        self.assertNotIn('brand_families', (Path(bf.__file__).parent / 'catalog.py').read_text())

    def test_lookup_rows_unchanged_and_family_annotated(self):
        for query in ('QuantaGrid', 'SPARC S7-2', 'NF5180A7', 'AS7726'):
            plain = lookup.run(self.db, query, -1)
            fam = lookup.run(self.db, query, -1, families=self.table)
            self.assertEqual(self.strip(fam), plain)
            self.assertEqual([r['id'] for r in fam['datasets'][0]['rows']], [r['id'] for r in plain['datasets'][0]['rows']])
            self.assertEqual(fam['brand_families']['sha256'], self.table.sha256)
        rows = lookup.run(self.db, 'QuantaGrid', -1, families=self.table)['datasets'][0]['rows']
        self.assertEqual({r['brand_family'] for r in rows}, {'quanta'})
        self.assertEqual(rows[0]['family_members_hit'], ['QCT', 'Quanta', 'Quanta Cloud Technology'])
        rows = {(r['brand'], r['model']): r for r in lookup.run(self.db, 'NF5180A7', -1, families=self.table)['datasets'][0]['rows']}
        self.assertIsNone(rows[('Inspur', 'NF5180A7')]['brand_family'])
        self.assertEqual(rows[('Inspur', 'NF5180A7')]['family_members_hit'], [])
        sparc = lookup.run(self.db, 'SPARC S7-2', -1, families=self.table)['datasets'][0]['rows']
        self.assertEqual({r['brand_family'] for r in sparc}, {'oracle'})
        self.assertEqual(sparc[0]['family_members_hit'], ['Oracle', 'Sun Microsystems'])
        # run_many keeps the same annotations as run.
        self.assertEqual(lookup.run_many(self.db, [('QuantaGrid', 5)], families=self.table),
                         [lookup.run(self.db, 'QuantaGrid', 5, families=self.table)])
        self.assertEqual(lookup.run_many(self.db, [('QuantaGrid', 5)]), [lookup.run(self.db, 'QuantaGrid', 5)])

    def test_mixed_mapped_and_unmapped_brands_keep_the_same_order(self):
        plain = lookup.run(self.db, 'X4470', -1)['datasets'][0]['rows']
        fam = lookup.run(self.db, 'X4470', -1, families=self.table)['datasets'][0]['rows']
        self.assertEqual([r['brand'] for r in plain], ['Accton', 'Oracle', 'Sun Fire'])
        self.assertEqual([r['id'] for r in fam], [r['id'] for r in plain])
        self.assertEqual([r['brand_family'] for r in fam], [None, 'oracle', None])
        for limit in (1, 2):
            self.assertEqual([r['id'] for r in lookup.run(self.db, 'X4470', limit, families=self.table)['datasets'][0]['rows']],
                             [r['id'] for r in lookup.run(self.db, 'X4470', limit)['datasets'][0]['rows']])
        plain = listings.Matcher(self.db).match('lot of X4470 units', -1)
        fam = listings.Matcher(self.db, families=self.table).match('lot of X4470 units', -1)
        self.assertEqual([c['brand'] for c in plain['candidates']], ['Accton', 'Oracle', 'Sun Fire'])
        self.assertEqual([c['device_id'] for c in fam['candidates']], [c['device_id'] for c in plain['candidates']])
        self.assertEqual([c['brand_family'] for c in fam['candidates']], [None, 'oracle', None])
        self.assertEqual([c['same_family_candidates'] for c in fam['candidates']], [0, 0, 0])
        limited = listings.Matcher(self.db, families=self.table).match('lot of X4470 units', 2)
        self.assertEqual([c['device_id'] for c in limited['candidates']], [c['device_id'] for c in plain['candidates']][:2])
        self.assertEqual(self.strip(fam), plain)

    def test_match_listing_candidates_unchanged_and_grouped(self):
        title = 'QCT QuantaGrid D52B-1U and Quanta QuantaGrid D43K-1U, SPARC S7-2'
        plain = listings.Matcher(self.db).match(title, -1)
        fam = listings.Matcher(self.db, families=self.table).match(title, -1)
        self.assertEqual(self.strip(fam), plain)
        self.assertEqual([c['device_id'] for c in fam['candidates']], [c['device_id'] for c in plain['candidates']])
        by = {(c['brand'], c['model']): c for c in fam['candidates']}
        self.assertEqual(by[('QCT', 'QuantaGrid D52B-1U')]['brand_family'], 'quanta')
        self.assertEqual(by[('QCT', 'QuantaGrid D52B-1U')]['same_family_candidates'], len([c for c in fam['candidates'] if c['brand_family'] == 'quanta']) - 1)
        self.assertGreaterEqual(by[('Quanta', 'QuantaGrid D52B-1U')]['same_family_candidates'], 1)
        # The hint counts all candidates, not only the shown ones, and never reorders them.
        limited = listings.Matcher(self.db, families=self.table).match(title, 1)
        self.assertEqual(limited['candidates'][0]['same_family_candidates'], fam['candidates'][0]['same_family_candidates'])
        self.assertEqual(limited['candidates'][0]['device_id'], fam['candidates'][0]['device_id'])
        self.assertEqual(fam['brand_families']['sha256'], self.table.sha256)

    def test_match_listing_flat_rows_only_append_columns(self):
        title = 'QCT QuantaGrid D52B-1U'
        plain_rows = listings.flat_rows(listings.Matcher(self.db).match(title, -1))
        fam_rows = listings.flat_rows(listings.Matcher(self.db, families=self.table).match(title, -1))
        self.assertEqual([list(r)[:len(plain_rows[0])] for r in fam_rows], [list(r) for r in plain_rows])
        self.assertEqual([{k: v for k, v in r.items() if k not in ('brand_family', 'same_family_candidates')} for r in fam_rows], plain_rows)
        self.assertEqual(list(fam_rows[0])[-2:], ['brand_family', 'same_family_candidates'])

    def test_model_collisions_groups_unchanged_and_family_annotated(self):
        plain = model_collisions.run(self.db, limit=-1)
        fam = model_collisions.run(self.db, limit=-1, families=self.table)
        self.assertEqual(self.strip(fam), plain)
        groups = {g['key']: g for g in fam['groups']}
        self.assertTrue(groups['quantagrid d43k-1u']['family']['same_family'])
        self.assertEqual(groups['quantagrid d43k-1u']['family']['ids'], ['quanta'])
        self.assertTrue(groups['sparc s7-2']['family']['same_family'])      # Oracle + Sun Microsystems (+ prefix twin)
        self.assertEqual(groups['nf5180a7']['family'], {'ids': [], 'unmapped_members': 2, 'same_family': False})
        self.assertEqual(fam['groups_same_family'], sum(g['family']['same_family'] for g in fam['groups']))
        self.assertIn('family=quanta same_family', model_collisions.render_table(fam))
        self.assertNotIn('family=', model_collisions.render_table(plain))
        self.assertEqual(model_collisions.run(self.db, 'Quanta', 5, families=self.table)['groups_total'],
                         model_collisions.run(self.db, 'Quanta', 5)['groups_total'])

    def test_cli_outputs_carry_family_hash_and_leave_database_untouched(self):
        before = self.path.read_bytes()
        out = self.cli('lookup', 'QuantaGrid D43K-1U', '--format', 'json', family_file=self.family_path)
        self.assertEqual(json.loads(out)['brand_families']['sha256'], self.table.sha256)
        text = self.cli('lookup', 'QuantaGrid D43K-1U', family_file=self.family_path)
        self.assertIn('brand_family', text)
        self.assertNotIn('family_members_hit', text)
        self.assertIn(self.table.sha256[:12], text)
        out = self.cli('match-listing', '--title', 'QCT QuantaGrid D52B-1U', '--format', 'json', family_file=self.family_path)
        self.assertEqual(json.loads(out)['results'][0]['result']['brand_families']['sha256'], self.table.sha256)
        out = self.cli('model-collisions', '--format', 'json', family_file=self.family_path)
        self.assertEqual(json.loads(out)['brand_families']['sha256'], self.table.sha256)
        plain = self.cli('lookup', 'QuantaGrid D43K-1U', '--format', 'json', family_file=self.root / 'absent.json')
        self.assertNotIn('brand_famil', plain)
        self.assertEqual(before, self.path.read_bytes())

    def test_alias_pairs_are_generated_by_the_rule_and_stay_in_one_brand(self):
        groups = {g['alias_key']: g for g in bf.generate_alias_pairs(self.db)}
        self.assertEqual(sorted(groups), ['sparc s7-2'])
        self.assertEqual(groups['sparc s7-2']['brand_normalised'], 'oracle')
        self.assertEqual([m['model'] for m in groups['sparc s7-2']['members']], ['Oracle SPARC S7-2', 'SPARC S7-2'])
        section = bf.alias_section(self.db, '2026-10-11')
        self.assertEqual(section['pairs'], list(groups.values()))
        self.assertEqual(section['rule']['source'], 'devicehunt.model_collisions.stripped_key')


class RealFileTests(unittest.TestCase):
    def test_shipped_table_validates(self):
        path = bf.ROOT / 'data/brand-families.json'
        if not path.is_file():
            self.skipTest('no shipped table')
        t = bf.load(path)
        self.assertEqual([t.family_of(b) for b in ('QCT', 'Quanta Cloud Technology Inc.', 'Sun Microsystems, Inc.', 'ORACLE')],
                         ['quanta', 'quanta', 'oracle', 'oracle'])
        for brand in ('Sun Fire', 'IEIT SYSTEMS', 'Inspur', 'Quanta Microsystems', 'Edge-Core'):
            self.assertIsNone(t.family_of(brand))
        self.assertEqual(t.data['model_prefix_aliases']['rule']['name'], 'own_brand_leading_token')
        self.assertEqual(t.data['model_prefix_aliases']['groups'], len(t.data['model_prefix_aliases']['pairs']))
        # Captures live in gitignored data/raw; when present they must match their recorded hashes.
        self.assertEqual(bf.verify_evidence(t)['mismatch'], [])


if __name__ == '__main__':
    unittest.main()

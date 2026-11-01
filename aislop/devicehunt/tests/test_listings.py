import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import listings
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog


class ListingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.path = self.root / 'db.sqlite'
        self.c = Catalog(self.path)
        for sid, tier in [('vendor', 'vendor'), ('seller', 'marketplace')]:
            self.c.source(sid, 'https://example.org/' + sid, tier, 'fixture', 'Test fixture')
        self.c.add('vendor', 'base', 'https://example.org/base', 'Edge-Core', 'AS7772-32X', 'switch', 'switch', {},
                   [('Edge-Core', 'brand')], [('alias', 'Commercial-7000')], [('compatible_accessory', 'Unrelated-1234')])
        self.c.add('vendor', 'variant', 'https://example.org/variant', 'Edge-Core', 'AS7772-32X-12V', 'switch', 'switch', {}, [('Edge-Core', 'brand')])
        self.c.add('seller', 'order', 'https://example.org/item', 'Edge-Core', '7760-32X-A-AC-F', 'switch', 'switch', {}, [('Edge-Core', 'seller_reported_brand')])
        self.c.add('vendor', 'other', 'https://example.org/other', 'Celestica', 'Commercial-7000', 'switch', 'switch', {}, [('Celestica', 'brand')])
        self.c.add('vendor', 'code', 'https://example.org/code', 'Accton', 'Minipack', 'switch', 'switch', {}, [('Accton', 'brand')])
        self.c.finish('vendor', 4); self.c.finish('seller', 1)

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def matcher(self, vendor=None):
        return listings.Matcher(self.c.db, vendor)

    def test_suffixes_and_unknown_extensions_never_become_exact_matches(self):
        matcher = self.matcher()
        got = matcher.match('Edgecore AS7772-32X-12V tested')
        self.assertEqual([(c['model'], c['best_match']) for c in got['candidates']],
                         [('AS7772-32X-12V', 'model_mention'), ('AS7772-32X', 'variant_lead')])
        got = matcher.match('AS7772-32X-UNKNOWN')
        self.assertEqual(got['status'], 'variant_leads_only')
        self.assertEqual([c['best_match'] for c in got['candidates']], ['variant_lead'])
        self.assertEqual(got['candidates'][0]['matches'][0]['related_title_tokens'], ['as7772-32x-unknown'])
        self.assertEqual(matcher.match('XAS7772-32X')['candidates'], [])
        self.assertEqual(matcher.match('AS7772-32XZ')['candidates'], [])
        self.assertEqual(matcher.match('AS7772-32X_suffix')['candidates'], [])

    def test_alias_collision_stays_ambiguous_and_vendor_filter_is_evidence_scoped(self):
        got = self.matcher().match('Commercial-7000 switch')
        self.assertEqual([(r['brand'], r['best_match']) for r in got['candidates']],
                         [('Celestica', 'model_mention'), ('Edge-Core', 'identifier_mention')])
        self.assertEqual(len(self.matcher('Edgecore').match('Commercial-7000')['candidates']), 1)
        with self.assertRaises(ValueError): self.matcher('not a target')

    def test_plus_suffix_stays_distinct_and_cannot_borrow_specs(self):
        for model, slots in [('SV7220G3-V', 'two x16 slots'), ('SV7220G3-V+', 'one x16 and two x8 slots')]:
            self.c.add('vendor', model, 'https://example.org/variants', 'Wiwynn', model,
                       model, 'server node', {}, [('Wiwynn', 'brand')], [],
                       [('physical_description', slots)])
        self.c.finish('vendor', 6)
        matcher = listings.Matcher(self.c.db, 'Wiwynn', include_specifications=True)
        before = self.path.read_bytes()
        for title, exact, lead in [('SV7220G3-V+', 'SV7220G3-V+', 'SV7220G3-V'),
                                   ('SV7220G3-V', 'SV7220G3-V', 'SV7220G3-V+')]:
            got = matcher.match('Wiwynn (' + title + ') untested')
            self.assertEqual([(c['model'], c['best_match']) for c in got['candidates']],
                             [(exact, 'model_mention'), (lead, 'variant_lead')])
            self.assertTrue(got['candidates'][0]['matches'][0]['specifications'])
            self.assertEqual(got['candidates'][1]['matches'][0]['specifications'], [])
        for title in ['SV7220G3-V++', 'SV7220G3-V+-UNKNOWN', 'SV7220G3-V+EXTRA']:
            got = matcher.match(title)
            self.assertEqual(got['status'], 'variant_leads_only', title)
            self.assertTrue(all(not m['specifications'] for c in got['candidates'] for m in c['matches']))
        self.assertEqual(matcher.match('XSV7220G3-V+')['matches'], 0)
        self.assertEqual(self.path.read_bytes(), before)

    def test_plus_suffix_on_phrase_identifiers_keeps_full_phrase(self):
        for model in ['Example Server 1000', 'Example Server 1000+']:
            self.c.add('vendor', model, 'https://example.org/phrase', 'Wiwynn', model,
                       model, 'server', {}, [('Wiwynn', 'brand')])
        self.c.finish('vendor', 6)
        for title, exact in [('Example Server 1000+', 'Example Server 1000+'),
                              ('Example Server 1000', 'Example Server 1000')]:
            result = self.matcher('Wiwynn').match(title)
            self.assertEqual(result['candidates'][0]['model'], exact)
            self.assertEqual(result['candidate_counts'], dict(model_mention=1, identifier_mention=0, variant_lead=1))
        self.assertEqual(self.matcher('Wiwynn').match('Example Server 1000++')['status'], 'variant_leads_only')
        self.assertEqual(self.matcher('Wiwynn').match('Other Server 1000+')['matches'], 0)

    def test_phrase_plus_alias_boundaries_context_and_maximum_title(self):
        self.c.add('seller', 'phrase-plus', 'https://example.org/phrase-plus', 'Wiwynn',
                   'BOARD-1000', 'board', 'server component', {},
                   [('Wiwynn', 'seller_reported_brand')], [('alias', 'Example Server 1000+')],
                   [('physical_description', 'Seller configuration claim')])
        self.c.finish('seller', 2)
        matcher = listings.Matcher(self.c.db, 'Wiwynn', include_specifications=True)
        exact = matcher.match('Replacement PSU for Example Server 1000+ (untested)')
        evidence = exact['candidates'][0]['matches'][0]
        self.assertEqual(evidence['match_type'], 'identifier_mention')
        self.assertEqual(evidence['tier'], 'marketplace')
        self.assertEqual(evidence['affiliations'][0]['role'], 'seller_reported_brand')
        self.assertEqual(exact['context_terms'], ['untested', 'replacement', 'psu'])
        self.assertEqual(len(evidence['specifications']), 1)
        prefix = 'Example Server 1000+'
        for suffix in ['+' * (4096 - len(prefix)), '-A+B-C', '+EXTRA']:
            title = prefix + suffix
            result = matcher.match(title)
            self.assertEqual(result['status'], 'variant_leads_only')
            lead = result['candidates'][0]['matches'][0]
            self.assertEqual(lead['related_title_tokens'], [title.lower()])
            self.assertEqual(lead['specifications'], [])
        for title in ['XExample Server 1000+', 'Example Server 1000_extra',
                      'Example / Server 1000+', 'Other Server 1000+']:
            self.assertEqual(matcher.match(title)['matches'], 0, title)
        with self.assertRaises(ValueError):
            matcher.match(prefix + '+' * (4097 - len(prefix)))

    def test_seller_evidence_stays_weak_and_listing_context_is_visible(self):
        got = self.matcher().match('Replacement power supply for 7760-32X-A-AC-F - UNTESTED')
        self.assertEqual(got['context_terms'], ['untested', 'replacement', 'power supply'])
        evidence = got['candidates'][0]['matches'][0]
        self.assertEqual(evidence['tier'], 'marketplace')
        self.assertEqual(evidence['affiliations'][0]['role'], 'seller_reported_brand')
        self.assertIn('not identification', got['scope'])
        self.assertEqual(listings.flat_rows(got)[0]['evidence_tiers'], 'marketplace')

    def test_only_identity_fields_match_and_codenames_and_dash_typography_work(self):
        matcher = self.matcher()
        self.assertEqual(matcher.match('Unrelated-1234')['status'], 'no_catalog_mention')
        self.assertEqual(matcher.match('minipack switch')['candidates'][0]['model'], 'Minipack')
        got = matcher.match('as7772\u201132x / lots')
        self.assertEqual(got['candidates'][0]['best_match'], 'model_mention')
        self.assertEqual(matcher.match('AS7772-32X OR 1=1')['candidates'][0]['model'], 'AS7772-32X')

    def test_limits_counts_and_invalid_inputs(self):
        matcher = self.matcher()
        for limit in [0, 1, -1]:
            got = matcher.match('Commercial-7000', limit)
            self.assertEqual(got['matches'], 2)
            self.assertEqual(got['shown'], 2 if limit == -1 else limit)
            self.assertEqual(got['truncated'], limit != -1)
        for title in ['', ' ', 'x' * 4097]:
            with self.assertRaises(ValueError): matcher.match(title)
        with self.assertRaises(ValueError): matcher.match('AS7772-32X', -2)

    def test_short_alphanumeric_models_are_explicit_and_keep_boundaries(self):
        for model in ['AF1', 'IBM', '123', 'A1']:
            self.c.add('vendor', model, 'https://example.org/' + model, 'Amazon Data Services', model,
                       model, 'appliance', {}, [('Amazon Data Services', 'manufacturer')])
        self.c.finish('vendor', 8)
        self.assertEqual(self.matcher().match('AWS Snowcone AF1')['status'], 'no_catalog_mention')
        matcher = listings.Matcher(self.c.db, 'Amazon Data Services', include_short_models=True)
        result = matcher.match('AF1 IBM 123 A1')
        self.assertTrue(result['short_identifiers_included'])
        self.assertEqual([r['model'] for r in result['candidates']], ['AF1'])
        for title in ['XAF1', 'AF10', 'AF1_extra']:
            self.assertEqual(matcher.match(title)['candidates'], [])
        self.assertEqual(matcher.match('AF1-X')['status'], 'variant_leads_only')
        self.assertEqual(listings.Matcher(self.c.db, 'Oracle', include_short_models=True).match('AF1')['candidates'], [])
        before = self.path.read_bytes()
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(main(['--db', str(self.path), 'match-listing', '--title', 'AWS Snowcone AF1',
                                   '--include-short-models', '--format', 'json']), 0)
        self.assertEqual(json.loads(out.getvalue())['results'][0]['result']['candidates'][0]['model'], 'AF1')
        self.assertEqual(before, self.path.read_bytes())

    def test_csv_retains_row_context_and_rejects_bad_shape(self):
        p = self.root / 'input.csv'
        p.write_text('\ufefftitle,url\n"Minipack, tested",https://example.org/lot\nunknown,\n')
        rows = listings.read_csv(p)
        self.assertEqual(rows[0]['title'], 'Minipack, tested')
        self.assertEqual(rows[0]['input']['url'], 'https://example.org/lot')
        self.assertEqual(rows[1]['input_record'], 2)
        for text in ['title,title\na,b\n', 'title,url\njust a title\n', 'title\nx,y\n', 'title\n', 'other\nfoo\n', 'title\n" "\n']:
            p.write_text(text)
            with self.assertRaises(ValueError): listings.read_csv(p)

    def test_numeric_opt_in_preserves_boundaries_zeros_and_suffixes(self):
        models = ['7103791', '07103791', '371-4325-02', '2026', '12345', '1' * 21, '１２３４５６', 'AF1']
        for model in models:
            self.c.add('vendor', model, 'https://example.org/numeric', 'Oracle', model, model, 'component', {},
                       [('Oracle', 'documentation_vendor')])
        self.c.finish('vendor', 4 + len(models))
        self.assertEqual(self.matcher('Oracle').match('7103791 371-4325-02')['matches'], 0)
        matcher = listings.Matcher(self.c.db, 'Oracle', include_numeric_identifiers=True)
        result = matcher.match('7103791 2026 12345 ' + '1' * 21 + ' １２３４５６ AF1')
        self.assertEqual([r['model'] for r in result['candidates']], ['7103791'])
        self.assertTrue(result['numeric_identifiers_included'])
        self.assertFalse(result['short_identifiers_included'])
        self.assertEqual(matcher.match('07103791')['candidates'][0]['model'], '07103791')
        self.assertEqual(matcher.match('371\u20114325\u201102')['candidates'][0]['model'], '371-4325-02')
        for title in ['X7103791', '71037910', '7103791x', '7103791_extra', '371432502']:
            self.assertEqual(matcher.match(title)['matches'], 0, title)
        result = matcher.match('7103791-REV2')
        self.assertEqual(result['status'], 'variant_leads_only')
        self.assertEqual(result['candidates'][0]['matches'][0]['related_title_tokens'], ['7103791-rev2'])
        self.assertEqual(matcher.match('371-4325')['status'], 'variant_leads_only')
        combined = listings.Matcher(self.c.db, 'Oracle', include_short_models=True, include_numeric_identifiers=True)
        self.assertEqual(combined.match('AF1 7103791')['matches'], 2)

    def test_numeric_aliases_keep_evidence_scope_and_do_not_promote_mappings(self):
        self.c.add('seller', 'numeric', 'https://example.org/seller', 'Edge-Core', 'AS7772-32X', 'switch',
                   'switch', {}, [('Oracle', 'seller_reported_brand')], [('part_number', '1234567')])
        self.c.add('vendor', 'numeric-other', 'https://example.org/other', 'Celestica', '1234567', 'component',
                   'component', {}, [('Celestica', 'brand')], [('oracle_mfg_part_number', '7654321')],
                   [('compatible_part', '9876543')])
        self.c.finish('vendor', 5); self.c.finish('seller', 2)
        all_matches = listings.Matcher(self.c.db, include_numeric_identifiers=True).match('1234567')
        self.assertEqual(all_matches['matches'], 2)
        oracle = listings.Matcher(self.c.db, 'Oracle', include_numeric_identifiers=True)
        result = oracle.match('1234567')
        self.assertEqual(result['matches'], 1)
        self.assertEqual(result['candidates'][0]['best_match'], 'identifier_mention')
        self.assertEqual({r['tier'] for r in result['candidates'][0]['matches']}, {'marketplace'})
        self.assertEqual(listings.Matcher(self.c.db, 'Edge-Core', include_numeric_identifiers=True).match('1234567')['matches'], 0)
        for title in ['7654321', '9876543']:
            self.assertEqual(listings.Matcher(self.c.db, include_numeric_identifiers=True).match(title)['matches'], 0)

    def test_numeric_cli_title_and_csv_are_readonly_and_report_options(self):
        self.c.add('vendor', 'numeric-cli', 'https://example.org/numeric-cli', 'Oracle', '7103791', 'component',
                   'component', {}, [('Oracle', 'documentation_vendor')])
        self.c.finish('vendor', 5)
        p = self.root / 'numeric.csv'; p.write_text('title\nOracle 7103791 for parts\nUnknownxyz\n')
        before = self.path.read_bytes()
        for inputs in [['--title', 'Oracle 7103791'], ['--csv', str(p)]]:
            with contextlib.redirect_stdout(io.StringIO()) as out:
                code = main(['--db', str(self.path), 'match-listing', *inputs, '--vendor', 'Oracle',
                             '--include-numeric-identifiers', '--format', 'json'])
            self.assertEqual(code, 0)
            data = json.loads(out.getvalue()); result = data['results'][0]['result']
            self.assertEqual(result['candidates'][0]['model'], '7103791')
            self.assertTrue(result['numeric_identifiers_included'])
            flat = listings.flat_rows(result)[0]
            self.assertTrue(flat['numeric_identifiers_included']); self.assertFalse(flat['short_identifiers_included'])
            if '--csv' in inputs:
                unmatched = listings.flat_rows(data['results'][1]['result'])[0]
                self.assertTrue(unmatched['numeric_identifiers_included'])
                self.assertEqual(unmatched['match_type'], 'no_catalog_mention')
        self.assertEqual(before, self.path.read_bytes())

    def test_cli_batch_is_readonly_offline_and_preserves_unmatched_rows(self):
        p = self.root / 'input.csv'; p.write_text('title\nMinipack\nUnknownxyz\n')
        before = self.path.read_bytes()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = main(['--db', str(self.path), 'match-listing', '--csv', str(p), '--format', 'json'])
        self.assertEqual(code, 0)
        data = json.loads(out.getvalue())
        self.assertEqual(data['listing_count'], 2)
        self.assertEqual(data['results'][1]['result']['status'], 'no_catalog_mention')
        self.assertEqual(before, self.path.read_bytes())
        missing = self.root / 'absent.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db', str(missing), 'match-listing', '--title', 'Minipack']), 1)
        self.assertFalse(missing.exists())

    def test_matcher_keeps_callers_transaction(self):
        self.c.db.execute("UPDATE devices SET model='Changed-1111' WHERE model='Minipack'")
        matcher = self.matcher()
        self.assertTrue(self.c.db.in_transaction)
        self.assertEqual(matcher.match('Changed-1111')['matches'], 1)
        self.c.db.rollback()

    def test_matching_alias_cannot_borrow_vendor_or_tier_from_another_record(self):
        self.c.add('seller', 'alias', 'https://example.org/seller-alias', 'Edge-Core', 'AS7772-32X',
                   'switch', 'switch', {}, [('Oracle', 'seller_reported_brand')], [('alias', 'Seller-1234')])
        self.c.finish('seller', 2)
        got = self.matcher().match('Seller-1234')
        self.assertEqual(got['candidates'][0]['best_match'], 'identifier_mention')
        self.assertEqual({e['source_id'] for e in got['candidates'][0]['matches']}, {'seller'})
        self.assertEqual(self.matcher('Edge-Core').match('Seller-1234')['matches'], 0)
        self.assertEqual(listings.flat_rows(got)[0]['evidence_tiers'], 'marketplace')

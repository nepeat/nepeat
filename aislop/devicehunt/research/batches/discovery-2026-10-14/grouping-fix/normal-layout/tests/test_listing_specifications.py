"""Auction specifications retain record/configuration boundaries."""
import contextlib
import csv
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt.listings import Matcher, flat_rows
from devicehunt.__main__ import main


class ListingSpecificationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'catalog.sqlite'
        self.c = Catalog(self.path)
        self.c.source('manual', 'https://example.org/manual', 'vendor', 'fixture', 'fixture')
        self.c.source('seller', 'https://example.org/seller', 'marketplace', 'fixture', 'fixture')
        self.ids = {}
        for key, source, company, ids, statement, revision in [
            ('front', 'manual', 'Celestica', [('part_number', 'R4023-F9102-A0')], 'AC; front-to-back airflow', ''),
            ('back', 'manual', 'Celestica', [('part_number', 'R4023-F9003-A0')], 'DC; back-to-front airflow', ''),
            ('family', 'manual', 'Celestica', [], 'Family alternatives, SKU dependent', ''),
            ('alias', 'manual', 'Celestica', [('alias', 'ALIAS-4000')], None, ''),
            ('empty', 'manual', 'Celestica', [('part_number', 'EMPTY-4000')], None, ''),
            ('other', 'manual', 'Oracle', [('part_number', 'R4023-F9102-A0')], 'Other company claim', ''),
            ('seller', 'seller', 'Celestica', [('part_number', 'R4023-F9102-A0')], 'Seller conflicting DC claim', ''),
            ('revision', 'manual', 'Celestica', [('part_number', 'R4023-F9102-A0')], 'Revision B claim', 'B'),
        ]:
            self.ids[key] = self.c.add(source, key, 'https://example.org/' + key, 'Celestica', 'DS4000',
                       'DS4000', 'switch', {}, [(company, 'seller_reported_brand' if source=='seller' else 'brand')],
                       ids, [('power_description', statement), ('irrelevant_fact', 'not a specification')] if statement else [], revision=revision)
        self.c.finish('manual', 7); self.c.finish('seller', 1)

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def matcher(self, vendor='Celestica'):
        return Matcher(self.c.db, vendor=vendor, include_specifications=True)

    @staticmethod
    def evidence(result):
        return [m for row in result['candidates'] for m in row['matches']]

    def test_ordering_code_keeps_same_record_vendor_revision_and_conflicts(self):
        result = self.matcher().match('Celestica R4023-F9102-A0 tested')
        evidence = self.evidence(result)
        self.assertEqual(len(evidence), 3)
        self.assertEqual({m['source_url'] for m in evidence}, {'https://example.org/front', 'https://example.org/seller', 'https://example.org/revision'})
        self.assertEqual({row['revision'] for row in result['candidates']}, {'', 'B'})
        for match in evidence:
            self.assertEqual(match['record_part_numbers'], ['R4023-F9102-A0'])
            self.assertEqual(len(match['specifications']), 1)
            self.assertEqual(match['specifications'][0]['category'], 'power_description')
        seller = next(m for m in evidence if m['source_id']=='seller')
        self.assertEqual(seller['tier'], 'marketplace')
        self.assertEqual(seller['specifications'][0]['statement'], 'Seller conflicting DC claim')
        other = self.evidence(self.matcher('Oracle').match('R4023-F9102-A0'))
        self.assertEqual([m['source_url'] for m in other], ['https://example.org/other'])

    def test_variants_and_records_without_specs_do_not_borrow_family_facts(self):
        for title in ['R4023-F9102-A0-UNKNOWN', 'R4023-F9102', 'DS4000-UNKNOWN', 'ALIAS-4000', 'EMPTY-4000']:
            result = self.matcher().match(title)
            self.assertTrue(result['candidates'], title)
            self.assertTrue(all(m['specifications']==[] for m in self.evidence(result)), title)
            self.assertTrue(all(json.loads(r['specifications_json'])==[] for r in flat_rows(result)), title)
        result = self.matcher().match('R4023-F9102-A0 / R4023-F9003-A0-UNKNOWN')
        for match in self.evidence(result):
            if match['identifier']=='R4023-F9003-A0':
                self.assertEqual(match['match_type'], 'variant_lead')
                self.assertEqual(match['specifications'], [])

    def test_spaced_model_suffixes_keep_phrase_boundaries_and_specification_scope(self):
        for model in ['Wedge 100', 'Wedge 100-32X', 'WEDGE100-32X']:
            self.c.add('manual', model, 'https://example.org/' + model, 'Accton', model,
                       model, 'switch', {}, [('Accton', 'manufacturer')],
                       facts=[('power_description', 'Configuration for ' + model)])
        self.c.finish('manual', 10)
        matcher = self.matcher('Accton')
        exact = matcher.match('wedge  100\u201132x empty chassis', limit=-1)
        self.assertEqual([(r['model'], r['best_match']) for r in exact['candidates']],
                         [('Wedge 100-32X', 'model_mention'), ('Wedge 100', 'variant_lead')])
        for candidate in exact['candidates']:
            evidence = candidate['matches'][0]
            self.assertEqual(len(evidence['specifications']), int(candidate['model']=='Wedge 100-32X'))
        for title in ['Wedge 100-32X-UNKNOWN', 'Wedge 100-UNKNOWN']:
            result = matcher.match(title, limit=-1)
            self.assertEqual(result['status'], 'variant_leads_only')
            self.assertTrue(result['candidates'])
            for match in self.evidence(result):
                self.assertEqual(match['specifications'], [])
                self.assertEqual(match['related_title_tokens'], [title.lower()])
        base = matcher.match('Wedge 100', limit=-1)
        variant = next(r for r in base['candidates'] if r['model']=='Wedge 100-32X')
        self.assertEqual(variant['matches'][0]['related_title_tokens'], ['wedge 100'])
        self.assertEqual(variant['matches'][0]['specifications'], [])
        for title in ['XWedge 100-32X', 'Wedge 100-32XZ', 'Wedge 100_extra',
                      'Wedge 1000', 'Wedge 100-32X_extra', 'Wedge / 100-32X']:
            result = matcher.match(title, limit=-1)
            self.assertFalse(any(m['specifications'] for m in self.evidence(result)), title)
            self.assertFalse(any(r['best_match']!='variant_lead' for r in result['candidates']), title)
        self.assertEqual([r['model'] for r in matcher.match('WEDGE100-32X')['candidates']], ['WEDGE100-32X'])

    def test_spaced_aliases_and_multiple_suffixes_remain_source_scoped(self):
        self.c.add('manual', 'spaced-alias', 'https://example.org/spaced', 'Celestica', 'BOARD-1000',
                   'board', 'switch', {}, [('Celestica', 'manufacturer')],
                   [('alias', 'Commercial Board 100-A-B')], [('power_description', 'Alias-record configuration')])
        self.c.finish('manual', 8)
        matcher = self.matcher()
        for title in ['Commercial Board 100', 'Commercial Board 100-A', 'Commercial Board 100-A-B-C']:
            evidence = self.evidence(matcher.match(title, limit=-1))
            self.assertEqual(len(evidence), 1)
            self.assertEqual(evidence[0]['match_type'], 'variant_lead')
            self.assertEqual(evidence[0]['related_title_tokens'], [title.lower()])
            self.assertEqual(evidence[0]['specifications'], [])
        exact = self.evidence(matcher.match('Commercial Board 100-A-B'))
        self.assertEqual(exact[0]['match_type'], 'identifier_mention')
        self.assertEqual(len(exact[0]['specifications']), 1)
        self.assertEqual(self.matcher('Oracle').match('Commercial Board 100-A-B-C')['candidates'], [])
        for title in ['Commercial Board', 'Board 100-A-B', 'Commercial Board 100A', 'XCommercial Board 100-A-B-C']:
            self.assertEqual(matcher.match(title)['candidates'], [], title)

    def test_family_mentions_keep_all_configuration_labels_and_flat_rows_deduplicate(self):
        result = self.matcher().match('DS4000 / R4023-F9102-A0')
        specs = [record for row in flat_rows(result) for record in json.loads(row['specifications_json'])]
        self.assertEqual(len(specs), 5)  # front, back, family, seller, revision
        front = next(r for r in specs if r['source_url']=='https://example.org/front')
        self.assertEqual(front['record_part_numbers'], ['R4023-F9102-A0'])
        self.assertEqual({r['identifier'] for r in front['matched_identifiers']}, {'DS4000','R4023-F9102-A0'})
        back = next(r for r in specs if r['source_url']=='https://example.org/back')
        self.assertEqual(back['record_part_numbers'], ['R4023-F9003-A0'])
        self.assertEqual(back['specifications'][0]['statement'], 'DC; back-to-front airflow')
        self.assertIn('family name', result['specification_scope'])
        plain = Matcher(self.c.db).match('DS4000')
        self.assertNotIn('specifications_included', plain)
        self.assertTrue(all('specifications' not in m for m in self.evidence(plain)))
        self.assertNotIn('specifications_json', flat_rows(plain)[0])

    def test_counts_missing_results_and_cached_matcher_are_consistent(self):
        matcher = self.matcher()
        result = matcher.match('R4023-F9102-A0', limit=0)
        self.assertEqual(result['matches'], 2)
        self.assertEqual(result['candidates'], [])
        self.assertEqual(json.loads(flat_rows(result)[0]['specifications_json']), [])
        unknown = matcher.match('unknown-device-999')
        self.assertEqual(unknown['status'], 'no_catalog_mention')
        self.assertEqual(json.loads(flat_rows(unknown)[0]['specifications_json']), [])
        # Results cannot mutate the matcher cache for subsequent CSV rows.
        first = matcher.match('R4023-F9102-A0')
        first['candidates'][0]['matches'][0]['specifications'][0]['statement'] = 'tampered'
        first['candidates'][0]['matches'][0]['record_part_numbers'].append('FAKE')
        second = matcher.match('R4023-F9102-A0')
        self.assertNotIn('tampered', json.dumps(second)); self.assertNotIn('FAKE', json.dumps(second))
        # Caller transaction remains open; the matcher retains its initial snapshot.
        self.c.db.execute("UPDATE facts SET value='uncommitted' WHERE name='power_description'")
        new = self.matcher(); self.assertTrue(self.c.db.in_transaction)
        self.c.db.rollback()
        self.assertTrue(all(m['specifications'][0]['statement']=='uncommitted' for m in self.evidence(new.match('R4023-F9102-A0'))))
        self.assertNotIn('uncommitted', json.dumps(matcher.match('R4023-F9102-A0')))

    def test_cli_json_and_csv_batch_preserve_evidence_and_readonly_database(self):
        before = self.path.read_bytes()
        p = Path(self.tmp.name)/'listings.csv'
        p.write_text('title,url\nR4023-F9102-A0,https://example.org/lot\nunknown-device-999,\n')
        with contextlib.redirect_stdout(io.StringIO()) as output:
            code = main(['--db',str(self.path),'match-listing','--csv',str(p),'--vendor','Celestica','--with-specs','--format','json'])
        self.assertEqual(code,0); result=json.loads(output.getvalue())
        self.assertEqual(result['results'][0]['input']['url'],'https://example.org/lot')
        self.assertTrue(result['results'][0]['result']['specifications_included'])
        self.assertEqual(len(self.evidence(result['results'][0]['result'])),3)
        with contextlib.redirect_stdout(io.StringIO()) as output:
            code = main(['--db',str(self.path),'match-listing','--csv',str(p),'--vendor','Celestica','--with-specs','--format','csv'])
        self.assertEqual(code,0); rows=list(csv.DictReader(io.StringIO(output.getvalue())))
        self.assertEqual(len(rows),3)  # two revisions and an unmatched title
        self.assertEqual({row['revision'] for row in rows[:2]}, {'', 'B'})
        self.assertEqual(rows[-1]['match_type'],'no_catalog_mention')
        self.assertEqual(json.loads(rows[-1]['specifications_json']),[])
        specs=[s for row in rows for s in json.loads(row['specifications_json'])]
        self.assertEqual(len(specs),3)
        self.assertTrue(all(s['record_part_numbers']==['R4023-F9102-A0'] for s in specs))
        self.assertEqual(before,self.path.read_bytes())

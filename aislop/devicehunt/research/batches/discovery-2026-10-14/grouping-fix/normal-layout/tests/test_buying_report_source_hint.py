import contextlib
import io
import tempfile
import unittest
from pathlib import Path

from devicehunt import buying, specifications
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main


class SourceFactHintTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / 'catalog.sqlite'
        self.c = Catalog(self.path)

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def add(self, source, model='HINT-1', vendor='Wiwynn', facts=(), tier='vendor', ids=()):
        capture = self.path.parent / (source + '.txt')
        capture.write_bytes(b'capture')
        self.c.source(source, 'https://example.org/' + source, tier, 'test', 'source scope', path=capture)
        self.c.add(source, source, 'https://example.org/' + source, vendor, model, model, 'server', {},
                   [(vendor, 'brand')], identifiers=ids, facts=facts)
        self.c.finish(source, 1)

    def test_hint_added_without_changing_status_or_statements(self):
        self.add('a', facts=[('spec_power_supply_quantity_and_rating_w', '1 x 1300'), ('spec_form_factor', '2U'),
                             ('spec_power_supply_details', 'n/a'), ('sonic_psu_count', '2')])
        r = buying.run(self.c.db, 'hint-1')
        power = r['categories']['power']
        self.assertEqual((power['status'], power['statements']), ('not_collected', []))
        self.assertEqual(power['source_facts_available']['count'], 1)
        self.assertEqual(power['source_facts_available']['fact_names'], ['spec_power_supply_quantity_and_rating_w'])
        self.assertIn('specs hint-1 --exact-model --include-source-facts', power['source_facts_available']['see'])
        self.assertEqual(r['categories']['chassis_physical']['source_facts_available']['fact_names'], ['spec_form_factor'])
        self.assertNotIn('source_facts_available', r['categories']['cooling'])
        self.assertEqual(r['categories']['power']['status'], 'not_collected')
        self.assertEqual(list(power)[:3], ['status', 'status_scope', 'statements'])

    def test_no_hint_without_source_facts_or_when_collected(self):
        self.add('a', facts=[('power', 'AC option'), ('spec_power_supply_details', '1 x 650')])
        r = buying.run(self.c.db, 'HINT-1')
        self.assertEqual(r['categories']['power']['status'], 'collected_source_statements')
        self.assertNotIn('source_facts_available', r['categories']['power'])
        self.add('b', model='PLAIN', facts=[('cooling_description', 'fans')])
        self.assertFalse(any('source_facts_available' in c for c in buying.run(self.c.db, 'PLAIN')['categories'].values()))
        self.assertNotIn('source_facts_available', buying.render_table(buying.run(self.c.db, 'PLAIN')))

    def test_scope_follows_matched_records_and_vendor_filter(self):
        self.add('a', facts=[('netbox_u_height', '2')])
        self.add('b', model='OTHER-1', facts=[('spec_power_supply_details', '2 x 800')])
        r = buying.run(self.c.db, 'HINT-1')
        self.assertNotIn('source_facts_available', r['categories']['power'])
        self.assertEqual(r['categories']['chassis_physical']['source_facts_available']['count'], 1)

    def spec_rows(self, report, category, **kw):
        """Run the report's `see` command's equivalent specs search; return rows of the category."""
        rows = specifications.search(self.c, kw['query'], target=kw.get('vendor'), limit=-1, **kw['flags'],
                                     include_source_facts=True)
        return [r for r in rows if r['category'] == category and r['origin'] == 'source fact']

    def test_see_part_number_spanning_two_models(self):
        self.add('a', model='M1', facts=[('spec_form_factor', '2U')], ids=[('part_number', 'PN-1')])
        self.add('b', model='M2', facts=[('netbox_u_height', '1')], ids=[('part_number', 'PN-1')])
        self.add('c', model='M1', facts=[('netbox_u_height', '4')], ids=[('part_number', 'PN-OTHER')])
        r = buying.run(self.c.db, 'pn-1')
        hint = r['categories']['chassis_physical']['source_facts_available']
        self.assertEqual(hint['count'], 2)
        self.assertEqual(hint['see'], 'python3 -m devicehunt specs pn-1 --exact-part-number --include-source-facts')
        rows = self.spec_rows(r, 'physical_description', query='pn-1', flags={'exact_part_number': True})
        self.assertEqual(len(rows), hint['count'])
        self.assertEqual(self.spec_rows(r, 'physical_description', query='pn-1', flags={'exact_model': True}), [])

    def test_see_alias_uses_query_and_exact_model(self):
        self.add('a', model='M1', facts=[('spec_form_factor', '2U')], ids=[('alias', 'AL-1')])
        self.add('b', model='M1', facts=[('netbox_u_height', '4')])
        hint = buying.run(self.c.db, 'AL-1')['categories']['chassis_physical']['source_facts_available']
        self.assertEqual(hint['see'], 'python3 -m devicehunt specs AL-1 --exact-model --include-source-facts')
        rows = self.spec_rows(None, 'physical_description', query='AL-1', flags={'exact_model': True})
        self.assertEqual((hint['count'], len(rows)), (1, 1))

    def test_see_carries_vendor_and_quotes(self):
        self.add('a', model='M 1', vendor='Accton', facts=[('spec_form_factor', '2U')])
        self.add('b', model='M 1', vendor='Edge-Core', facts=[('netbox_u_height', '4')])
        r = buying.run(self.c.db, 'M 1', 'accton')
        hint = r['categories']['chassis_physical']['source_facts_available']
        self.assertEqual(hint['see'], "python3 -m devicehunt specs 'M 1' --exact-model --include-source-facts --vendor Accton")
        rows = self.spec_rows(r, 'physical_description', query='M 1', vendor='Accton', flags={'exact_model': True})
        self.assertEqual((hint['count'], len(rows)), (1, 1))
        unfiltered = buying.run(self.c.db, 'M 1')['categories']['chassis_physical']['source_facts_available']
        self.assertEqual(unfiltered['count'], 2)
        self.assertNotIn('--vendor', unfiltered['see'])

    def test_see_mixed_part_number_and_model_matches_lists_both(self):
        self.add('a', model='PN-9', facts=[('spec_form_factor', '2U')])
        self.add('b', model='M2', facts=[('netbox_u_height', '1')], ids=[('part_number', 'PN-9')])
        hint = buying.run(self.c.db, 'PN-9')['categories']['chassis_physical']['source_facts_available']
        self.assertEqual(hint['count'], 2)
        self.assertEqual(hint['see'].count('python3 -m devicehunt specs'), 2)
        self.assertIn('--exact-model', hint['see'])
        self.assertIn('--exact-part-number', hint['see'])

    def test_table_line_and_cli(self):
        self.add('a', facts=[('spec_power_supply_details', '1 x 650')])
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(main(['--db', str(self.path), 'buying-report', 'HINT-1']), 0)
        text = out.getvalue()
        self.assertIn('power: not_collected\n  source_facts_available: 1 (names: spec_power_supply_details; see: python3 -m devicehunt specs HINT-1', text)


if __name__ == '__main__':
    unittest.main()

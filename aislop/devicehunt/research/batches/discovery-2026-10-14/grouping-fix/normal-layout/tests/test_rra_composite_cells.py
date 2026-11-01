"""Certificate-scoped composite manufacturer cell evidence (SUN-6000, Kladno-only)."""
import hashlib
import json
import shutil
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import rra, rra_company_names, rra_manufacturer_lists
from devicehunt.campaign_sources import RraSavedAdapter
from devicehunt.catalog import Catalog, target_for
from devicehunt.pipeline import import_source

ROOT = Path(__file__).resolve().parents[1]
CEL = ROOT / 'data/raw/rra-celestica-official'
WRAPPER = 'rra-composite-manufacturer-cells-2026-10-11.json'
BUNDLE = 'bundle-composite-cells.json'
CELL = 'Celestica de Monterrey S.A. de C.V.,Celestica Kladno s.r.o. (Oracle America, Inc.)'
KEYS = {'SUN-6000:' + m for m in ('6000', 'Netra 6000 AC', 'Netra 6000 Chassis',
                                  'Oracle Sun Blade 6000 Chassis M2', 'Sun Blade 6000 Chassis')}


def sha(data):
    return hashlib.sha256(data).hexdigest()


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix='composite-'))
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def copy(self, edit=None):
        """Throwaway bundle directory; `edit(wrapper_dict)` changes the wrapper and re-declares its hash."""
        dest = Path(tempfile.mkdtemp(dir=self.tmp))
        shutil.copytree(CEL, dest, dirs_exist_ok=True)
        if edit:
            wrapper = json.loads((dest / WRAPPER).read_text())
            edit(wrapper)
            (dest / WRAPPER).write_text(json.dumps(wrapper, indent=2, ensure_ascii=False))
            bundle = json.loads((dest / BUNDLE).read_text())
            bundle['company_evidence']['sha256'] = sha((dest / WRAPPER).read_bytes())
            (dest / BUNDLE).write_text(json.dumps(bundle, indent=2, ensure_ascii=False))
        return dest / BUNDLE


class SavedEvidenceTests(Base):
    def test_sibling_bundle_differs_from_bundle_only_in_company_evidence(self):
        old, new = json.loads((CEL / 'bundle.json').read_text()), json.loads((CEL / BUNDLE).read_text())
        self.assertEqual({k: v for k, v in old.items() if k != 'company_evidence'},
                         {k: v for k, v in new.items() if k != 'company_evidence'})
        self.assertEqual(new['company_evidence']['sha256'], sha((CEL / WRAPPER).read_bytes()))

    def test_entry_hashes_match_the_saved_page(self):
        entry = json.loads((CEL / WRAPPER).read_text())['entries'][0]
        page = (CEL / 'detail-090.html').read_bytes()
        raw = entry['cell']['raw_html_cell_bytes']
        self.assertEqual(sha(page), entry['detail_sha256'])
        self.assertEqual(sha(page[raw['offset']:raw['offset'] + raw['length']]), raw['sha256'])
        self.assertEqual(sha(entry['cell']['text'].encode()), entry['cell']['text_sha256'])
        self.assertEqual(entry['cell']['text'], CELL)

    def test_claims_and_adapter(self):
        data, ready = rra.validate(CEL / BUNDLE)
        claims = rra.composite_claims(CEL, data, ready)
        self.assertEqual(list(claims), ['SUN-6000'])
        out = RraSavedAdapter().records(CEL / BUNDLE, 'data/raw/rra-celestica-official/' + BUNDLE)
        self.assertEqual((out['stats']['eligible_manufacturer_rows'], out['stats']['model_assertions']), (106, 154))
        sun = next(o for o in out['occurrences'] if o['certificate'] == 'SUN-6000')
        self.assertTrue(sun['eligible_manufacturer'])
        self.assertEqual(set(sun['projected_source_keys']), KEYS)

    def test_unchanged_inputs_keep_their_old_behaviour(self):
        old = RraSavedAdapter().records(CEL / 'bundle.json', 'x/bundle.json')
        self.assertEqual((old['stats']['eligible_manufacturer_rows'], old['stats']['model_assertions']), (105, 149))
        oracle = RraSavedAdapter().records(ROOT / 'data/raw/rra-oracle-official/bundle.json', 'x/bundle.json')
        sun = next(o for o in oracle['occurrences'] if o['certificate'] == 'SUN-6000')
        self.assertFalse(sun['eligible_manufacturer'])
        self.assertEqual(sun['projected_source_keys'], [])
        self.assertEqual(rra_manufacturer_lists.prepare(ROOT / 'data/raw/rra-manufacturer-lists.json')[2]['model_assertions'], 45)

    def test_parenthetical_would_map_globally_so_it_must_not_be_an_affiliation(self):
        self.assertEqual(target_for('(Oracle America, Inc.)'), 'Oracle')

    def test_import_makes_five_records_with_kladno_only(self):
        c = Catalog(self.tmp / 'c.sqlite')
        import_source(c, 'rra', CEL / BUNDLE)
        sid = 'rra-official-query-41e4ca912b33'
        keys = {r[0] for r in c.db.execute("SELECT source_key FROM records WHERE source_id=? AND source_key LIKE 'SUN-6000:%'", (sid,))}
        self.assertEqual(keys, KEYS)
        rows = c.db.execute("SELECT a.company,a.target,a.role,r.source_key FROM affiliations a JOIN records r ON r.id=a.record_id "
                            "WHERE r.source_id=? AND r.source_key LIKE 'SUN-6000:%'", (sid,)).fetchall()
        self.assertEqual({(r[0], r[1], r[2]) for r in rows},
                         {('Celestica Kladno s.r.o.', 'Celestica', 'registry_manufacturer'),
                          ('ORACLE AMERICA. INC', 'Oracle', 'regulatory_applicant')})
        self.assertEqual(len(rows), 10)
        facts = dict(c.db.execute("SELECT f.name,f.value FROM facts f JOIN records r ON r.id=f.record_id "
                                  "WHERE r.source_key='SUN-6000:6000'").fetchall())
        self.assertEqual(facts['manufacturer_cell_unlabelled_text'], '(Oracle America, Inc.)')
        self.assertEqual(facts['rra_manufacturer_as_printed'], CELL)
        self.assertIn('unmapped_context', json.dumps(
            [v for (v,) in c.db.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id "
                                        "WHERE r.source_key='SUN-6000:6000' AND f.name='manufacturer_cell_part'")]))
        coverage = c.db.execute('SELECT coverage FROM sources WHERE id=?', (sid,)).fetchone()[0]
        self.assertIn('Exception to unsplit composite cells', coverage)
        # re-import replaces rather than duplicates
        import_source(c, 'rra', CEL / BUNDLE)
        self.assertEqual(c.db.execute("SELECT count(*) FROM records WHERE source_key LIKE 'SUN-6000:%'").fetchone()[0], 5)
        c.db.close()

    def test_old_bundle_import_has_no_sun6000_records_and_unchanged_coverage_wording(self):
        c = Catalog(self.tmp / 'o.sqlite')
        import_source(c, 'rra', CEL / 'bundle.json')
        self.assertEqual(c.db.execute("SELECT count(*) FROM records WHERE source_key LIKE 'SUN-6000:%'").fetchone()[0], 0)
        self.assertNotIn('Exception to unsplit', c.db.execute('SELECT coverage FROM sources').fetchone()[0])
        c.db.close()


class RefusalTests(Base):
    def refused(self, edit, text=None):
        path = self.copy(edit)
        with self.assertRaises(ValueError) as caught:
            rra.validate(path)
        if text:
            self.assertIn(text, str(caught.exception))

    def entry(self, w):
        return w['entries'][0]

    def test_parts_must_reproduce_the_cell(self):
        self.refused(lambda w: self.entry(w)['parts'][1].update(text='Celestica Kladno s.r.o'), 'reproduce')

    def test_cell_hash_must_match(self):
        self.refused(lambda w: self.entry(w)['cell'].update(text_sha256='0' * 64), 'hash')

    def test_mapped_part_requirements(self):
        self.refused(lambda w: self.entry(w)['parts'][1].update(role='brand'), 'backed')
        self.refused(lambda w: self.entry(w)['parts'][1].update(target='Oracle'), 'backed')
        self.refused(lambda w: self.entry(w)['parts'][1].update(name_evidence='unmapped_context'), 'backed')

    def test_context_part_must_be_unmapped_and_not_already_mappable(self):
        self.refused(lambda w: self.entry(w)['parts'][0].update(target='Celestica'), 'context part')
        self.refused(lambda w: self.entry(w)['parts'][0].update(name_evidence='base'), 'context part')

    def test_entry_must_map_a_part(self):
        def drop(w):
            e = self.entry(w)
            e['parts'][1].update(role=None, target=None, name_evidence='unmapped_context')
        # Kladno is mappable by the base, so a context declaration for it is refused first.
        self.refused(drop)

    def test_duplicate_and_empty_entries(self):
        self.refused(lambda w: w['entries'].append(json.loads(json.dumps(self.entry(w)))), 'Duplicate')
        self.refused(lambda w: w.update(entries=[]), 'Unrecognized')

    def test_wrong_base(self):
        self.refused(lambda w: w.update(base=dict(path='celestica-subsidiaries-2025.json',
                                                  sha256=sha((CEL / 'celestica-subsidiaries-2025.json').read_bytes()))), 'base')

    def test_query_maker_scope(self):
        self.refused(lambda w: self.entry(w).update(query_maker='Oracle'), 'does not match')

    def test_detail_hash_must_match(self):
        self.refused(lambda w: self.entry(w).update(detail_sha256='0' * 64), 'does not match')

    def test_raw_cell_bytes_are_verified(self):
        self.refused(lambda w: self.entry(w)['cell']['raw_html_cell_bytes'].update(offset=3898), 'raw bytes')

    def test_entry_matching_no_row_is_refused(self):
        self.refused(lambda w: self.entry(w).update(certificate_number='SUN-T6300'), 'matched no row')

    def test_wrapper_under_the_oracle_query_is_refused(self):
        dest = self.tmp / 'o'
        shutil.copytree(ROOT / 'data/raw/rra-oracle-official', dest)
        for name in (WRAPPER, 'celestica-historical-companies.json', 'celestica-subsidiaries-2025.json',
                     'celestica-sec-8.1-reviewed.json', 'celestica-archived-21.1-reviewed.json'):
            shutil.copy(CEL / name, dest / name)
        bundle = json.loads((dest / 'bundle.json').read_text())
        bundle['company_evidence'] = json.loads((CEL / BUNDLE).read_text())['company_evidence']
        (dest / 'bundle.json').write_text(json.dumps(bundle))
        with self.assertRaises(ValueError):
            rra.validate(dest / 'bundle.json')

    def test_claim_target_uses_the_first_mapped_part(self):
        claims = {'SUN-6000': json.loads((CEL / WRAPPER).read_text())['entries'][0]}
        self.assertIsNone(claims['SUN-6000']['parts'][0]['target'])
        row = dict(certificate_number='SUN-6000', manufacturer=CELL)
        self.assertEqual(rra.claim_target(row, {}, claims), 'Celestica')
        self.assertIsNone(rra.claim_target(row, {}, {}))


if __name__ == '__main__':
    unittest.main()

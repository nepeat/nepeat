import copy
import json
from pathlib import Path
import tempfile
import unittest

from review_build import ROOT, AuditHTML, build, parse_source


TABLE = b'<html><h2>Program table</h2><table id="t"><tr><th>Partner</th><th>System</th><th>Tested NVIDIA GPUs</th></tr><tr><td>QCT</td><td>D75E-4U+</td><td></td></tr><tr><td>QCT</td><td>D75E-4U+</td><td>T4</td></tr></table></html>'


class IndependentReviewTests(unittest.TestCase):
    def test_complete_accepted_data_is_reproducible_from_copied_captures(self):
        selection = json.loads((ROOT / 'selection.json').read_text())
        candidate, rows = build(selection, ROOT / 'raw')
        self.assertEqual(candidate, json.loads((ROOT / 'candidate.json').read_text()))
        self.assertEqual(rows, json.loads((ROOT / 'accepted-rows.json').read_text()))
        self.assertEqual(len(candidate['sources']), 5)
        self.assertEqual(len(candidate['devices']), 214)
        self.assertEqual(len({(r['brand'],r['model']) for r in candidate['devices']}), 154)

    def test_duplicates_suffixes_and_blank_cells_remain_distinct_rows(self):
        rows, _ = parse_source(TABLE)
        self.assertEqual([r['model'] for r in rows], ['D75E-4U+', 'D75E-4U+'])
        self.assertNotEqual(rows[0]['locator'], rows[1]['locator'])
        self.assertEqual(rows[0]['row']['cells'][2]['text'], '')
        self.assertEqual(rows[1]['row']['cells'][2]['text'], 'T4')
        self.assertEqual(rows[0]['headers'][2], 'Tested NVIDIA GPUs')

    def test_blank_partner_is_not_forward_filled(self):
        rows, _ = parse_source(TABLE.replace(b'<td>QCT</td><td>D75E-4U+</td><td>T4', b'<td></td><td>D75E-4U+</td><td>T4'))
        self.assertEqual(len(rows), 1)

    def test_spans_identity_footnotes_and_missing_cells_rejected(self):
        for before, after in [(b'<td>QCT</td>', b'<td rowspan="2">QCT</td>'),
                              (b'<td>D75E-4U+</td>', b'<td>D75E-4U+<sup>1</sup></td>'),
                              (b'<td>T4</td>', b'')]:
            with self.subTest(before=before):
                with self.assertRaises((ValueError, AssertionError)):
                    parse_source(TABLE.replace(before, after))

    def test_nonidentity_marker_retained_without_becoming_an_identifier(self):
        rows, _ = parse_source(TABLE.replace(b'<td>T4</td>', b'<td>T4<sup>*</sup></td>'))
        self.assertEqual(rows[1]['row']['cells'][2]['text'], 'T4*')
        self.assertTrue(rows[1]['row']['cells'][2]['has_superscript'])

    def test_truncated_and_nested_tables_rejected(self):
        for raw in [TABLE.replace(b'</table>', b''), TABLE.replace(b'<td>T4</td>', b'<td><table></table></td>')]:
            with self.assertRaises(ValueError):
                parse_source(raw)

    def test_copied_capture_hash_mismatch_fails_before_build(self):
        selection = json.loads((ROOT / 'selection.json').read_text())
        with tempfile.TemporaryDirectory(dir=ROOT / 'tmp') as temp:
            root = Path(temp)
            first = selection['sources'][0]
            (root / first['body_path']).write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                build(selection, root)

    def test_reference_model_or_source_row_omission_fails(self):
        selection = json.loads((ROOT / 'selection.json').read_text())
        changed = copy.deepcopy(selection)
        changed['references'][0]['model'] += '-invented'
        with self.assertRaisesRegex(ValueError, 'disagrees'):
            build(changed, ROOT / 'raw')
        changed = copy.deepcopy(selection)
        changed['references'].pop(0)
        with self.assertRaisesRegex(ValueError, 'coverage changed'):
            build(changed, ROOT / 'raw')

    def test_card_per_node_units_and_absent_fields_preserved(self):
        rows, _ = parse_source((ROOT / 'raw/mitac-multi.html').read_bytes(), True)
        first = rows[0]['card']
        self.assertEqual(first['family'], 'C2810Z5 (2)')
        self.assertEqual([(f['label']['text'],f['value']['text']) for f in first['fields']],
                         [('CPUs','1 / per node'),('CPU Type','AMD EPYC™ 9005 Series Processor'),('Form Factor','2OU'),('Storage Bays','14 / per node')])
        missing = [r['model'] for r in rows if len(r['card']['fields']) == 3]
        self.assertEqual(missing, ['D50DNP1MHCPAC', 'D50DNP1MHCPLC', 'D50DNP1MHEVAC', 'B8016H68D5'])

    def test_candidate_roles_whole_cells_and_no_manufacturer_or_aliases(self):
        candidate = json.loads((ROOT / 'candidate.json').read_text())
        models = {r['model'] for r in candidate['devices']}
        self.assertIn('Miramar - ICEMan', models)
        self.assertIn('Miramar -ICEMan', models)
        self.assertIn('X-wing-Flyboy (Tacoma)', models)
        self.assertTrue(all(not r['identifiers'] and not r['hyperscaler'] for r in candidate['devices']))
        self.assertEqual({role for r in candidate['devices'] for _,role in r['affiliations']},
                         {'program_partner','listing_holder','documentation_vendor'})
        self.assertFalse(any('amd' in s['id'] for s in candidate['sources']))


if __name__ == '__main__':
    unittest.main(verbosity=2)

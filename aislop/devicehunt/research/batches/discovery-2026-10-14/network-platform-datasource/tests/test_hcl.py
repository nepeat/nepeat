import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("network_hcl", ROOT / "hcl.py")
hcl = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hcl)
URL = "https://www.bisdn.de/en/linux/compatibility/"


class MatrixTests(unittest.TestCase):
    def setUp(self):
        self.raw = (ROOT / "tests/fixtures/bisdn-complete-table.html").read_bytes()

    def test_complete_fixture_preserves_every_qualifier_and_download(self):
        matrix = hcl.parse_bisdn(self.raw, URL)
        self.assertEqual(matrix["row_count"], 11)
        self.assertEqual([r["cells"]["Vendor"] for r in matrix["rows"]],
                         ["Celestica", "Delta", "Delta"] + ["Edgecore"] * 8)
        self.assertEqual([r["cells"]["Platform"] for r in matrix["rows"]], [
            "Questone 2 D3030 (Questone 2A)", "AG5648", "AG7648", "AS4610-30T",
            "AS4610-30P", "AS4610-54T", "AS4610-54P", "EPS201 (AS4630-54TE)",
            "EPS202 (AS4630-54PE)", "DCS201 (AS5835-54X)", "AS7726-32X"])
        self.assertTrue(all(r["cells"]["Release"] == "6.0.0" for r in matrix["rows"]))
        self.assertEqual(matrix["rows"][1]["cells"]["ASIC"], "Broadcom Tomahawk (+)")
        self.assertEqual(matrix, hcl.parse_bisdn(self.raw, URL))

    def test_real_capture_has_exact_fixture_rows(self):
        receipt, raw = hcl.verified_capture(ROOT, "raw/05-bisdn-hcl.json")
        actual = hcl.parse_bisdn(raw, receipt["url"])
        expected = hcl.parse_bisdn(self.raw, URL)
        self.assertEqual([r["cells"] for r in actual["rows"]], [r["cells"] for r in expected["rows"]])

    def test_duplicate_rows_are_retained(self):
        text = self.raw.decode()
        start = text.index('<tr data-hcl-vendor=Delta')
        end = text.index('</tr>', start) + len('</tr>')
        text = text.replace('</tbody>', text[start:end] + '</tbody>')
        matrix = hcl.parse_bisdn(text.encode(), URL)
        self.assertEqual(matrix["row_count"], 12)
        self.assertEqual(matrix["rows"][1]["cells"], matrix["rows"][-1]["cells"])
        candidate = hcl.candidate(matrix, {"body_path": "fixture", "finished_utc": "test"})
        self.assertEqual(len({d["key"] for d in candidate["devices"]}), 12)

    def test_current_retired_context_is_literal_per_table(self):
        tables = hcl.MatrixHTML(URL).finish('<h1>Example HCL</h1><h2>Current platforms</h2><table><tr><td>A</td></tr></table><h2>Retired platforms</h2><table><tr><td>B</td></tr></table>')
        self.assertEqual(tables[0]["headings"], {1: "Example HCL", 2: "Current platforms"})
        self.assertEqual(tables[1]["headings"], {1: "Example HCL", 2: "Retired platforms"})

    def test_inline_text_entities_and_links_are_retained(self):
        table = hcl.MatrixHTML(URL).finish('<table><tr><td> A<b>+B</b>&amp; C<br>D<a href="/x?m=a&amp;b=c"> go</a></td></tr></table>')[0]
        cell = table["rows"][0]["cells"][0]
        self.assertEqual(cell["text"], 'A+B& C D go')
        self.assertEqual(cell["links"][0]["url"], 'https://www.bisdn.de/x?m=a&b=c')

    def test_schema_and_structural_changes_fail_closed(self):
        for before, after in [(b'>ASIC<', b'>Chip<'), (b'<td>Delta</td>', b'<td></td>'),
                              (b'data-hcl-platform=AG5648', b'data-hcl-platform=WRONG'),
                              (b'platform=AG5648', b'platform=WRONG'),
                              (b'<td>Delta', b'<td rowspan=2>Delta'),
                              (b'</table>', b'')]:
            with self.subTest(before=before):
                with self.assertRaises(ValueError):
                    hcl.parse_bisdn(self.raw.replace(before, after), URL)
        with self.assertRaises(ValueError):
            hcl.parse_bisdn(b'<html><body>Vue shell</body></html>', URL)

    def test_capture_hash_mismatch_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'body').write_bytes(self.raw)
            (root / 'receipt').write_text(json.dumps({"status": 200, "body_path": "body", "body_bytes": len(self.raw), "body_sha256": "incorrect"}))
            with self.assertRaisesRegex(ValueError, 'hash/length'):
                hcl.verified_capture(root, 'receipt')

    def test_candidate_is_source_scoped_with_no_alias_expansion_or_roles(self):
        matrix = hcl.parse_bisdn(self.raw, URL)
        candidate = hcl.candidate(matrix, {"body_path": "fixture", "finished_utc": "test"})
        self.assertEqual(len(candidate["sources"]), 1)
        self.assertEqual(len(candidate["devices"]), 11)
        self.assertEqual(sum(len(d["facts"]) for d in candidate["devices"]), 77)
        self.assertTrue(all(d["brand"] == 'BISDN Linux HCL' and not d["affiliations"] for d in candidate["devices"]))
        self.assertEqual(candidate["devices"][0]["model"], 'Questone 2 D3030 (Questone 2A)')
        self.assertEqual(candidate["devices"][0]["identifiers"], [['bisdn_hcl_platform', 'Questone 2 D3030 (Questone 2A)']])

    def test_novelty_distinguishes_exact_case_and_substring_without_writing(self):
        with tempfile.TemporaryDirectory() as tmp:
            db = Path(tmp) / 'catalog.sqlite'
            with sqlite3.connect(db) as c:
                c.executescript('CREATE TABLE devices(id TEXT,brand TEXT,model TEXT); CREATE TABLE records(id TEXT,device_id TEXT); CREATE TABLE identifiers(record_id TEXT,kind TEXT,value TEXT);')
                c.executemany('INSERT INTO devices VALUES(?,?,?)', [('a','hardware','AG5648'), ('b','hardware','as7726-32x'), ('c','hardware','prefix AG7648 suffix')])
                c.execute("INSERT INTO records VALUES('r','a')")
                c.execute("INSERT INTO identifiers VALUES('r','other','AS4610-30T')")
            before = db.read_bytes()
            result = hcl.novelty(hcl.parse_bisdn(self.raw, URL), db)
            self.assertEqual(db.read_bytes(), before)
            self.assertFalse(result['rows'][1]['absent_as_exact_model_or_identifier'])
            self.assertTrue(result['rows'][2]['absent_as_exact_model_or_identifier'])
            self.assertFalse(result['rows'][3]['absent_as_exact_model_or_identifier'])
            self.assertTrue(result['rows'][-1]['absent_as_exact_model_or_identifier'])
            self.assertEqual(result['rows'][-1]['case_insensitive_only_model_leads'][0][0], 'b')
            self.assertEqual(result['database_sha256_before'], hashlib.sha256(before).hexdigest())
            with self.assertRaises(FileNotFoundError):
                hcl.novelty(hcl.parse_bisdn(self.raw, URL), Path(tmp) / 'absent.sqlite')


if __name__ == '__main__':
    unittest.main()

import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import lookup
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main


class MatchContextTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / 'db.sqlite'
        c = Catalog(self.path)
        c.source('s', 'https://example.org/s', 'project', 'fixture', 'Fixture scope')
        c.add('s', 'a', 'https://example.org/a', 'Meta OpenBMC platform', 'minipack', 'Meta OpenBMC platform minipack',
              'BMC firmware platform definition (software reference)', {}, [],
              identifiers=[], facts=[('psu_part_string', 'PFE1100-12-054NA, other'), ('zz_note', 'pfe1100-12-054na'),
                                     ('unrelated', 'nothing')])
        c.add('s', 'b', 'https://example.org/b', 'Acme', 'PFE1100-12-054NA', 'Acme PFE1100-12-054NA',
              'server', {}, [], identifiers=[], facts=[(f'f{n}', 'Pfe1100-12-054NA') for n in range(8)])
        c.finish('s', 2)
        c.db.close()

    def tearDown(self):
        self.temp.cleanup()

    def rows(self, query='PFE1100-12-054NA'):
        with sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            result = lookup.run(db, query, 20)
        return {r['model']: r for r in result['datasets'][0]['rows']}

    def test_fact_names_only_for_software_row(self):
        row = self.rows()['minipack']
        self.assertEqual(row['matched_in'], 'facts[2]: psu_part_string, zz_note')
        self.assertEqual(row['matched_in_detail']['fact_names'], ['psu_part_string', 'zz_note'])
        self.assertFalse(row['identity_text_matches'])

    def test_identity_and_capped_sorted_fact_names(self):
        row = self.rows()['PFE1100-12-054NA']
        self.assertEqual(row['matched_in'], 'identity; facts[8]: f0, f1, f2, f3, f4 (+3 more)')
        self.assertEqual(row['matched_in_detail']['fact_name_count'], 8)

    def test_unicode_casefold_and_no_value_leak(self):
        self.assertIn('psu_part_string', self.rows('pfe1100-12-054NA')['minipack']['matched_in'])
        self.assertNotIn('other', self.rows()['minipack']['matched_in'])

    def test_table_appends_matched_in_last_and_json_keeps_detail(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            main(['--db', str(self.path), 'lookup', 'PFE1100-12-054NA'])
        header = next(l for l in out.getvalue().splitlines() if l.startswith('id\tbrand'))
        cols = header.split('\t')
        self.assertEqual(cols[-1], 'matched_in')
        self.assertEqual(cols[-2], 'identity_text_match')
        self.assertNotIn('matched_in_detail', cols)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            main(['--db', str(self.path), 'lookup', 'PFE1100-12-054NA', '--format', 'json'])
        self.assertIn('matched_in_detail', json.loads(out.getvalue())['datasets'][0]['rows'][0])

    def test_scope_note_mentions_software_references(self):
        with sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            scope = lookup.run(db, 'minipack')['datasets'][0]['scope']
        self.assertIn('not fitment evidence', scope)


class CitationTests(unittest.TestCase):
    def test_strip_citations_rule(self):
        strip = lookup.strip_citations
        for v in ('D52B-1U (C621)', 'FB_temp(0x4C)', 'Length(m):1/2/2.5', 'PCI (8086:1521)', 'Rack (2U, 4:3)',
                  'x (a/b c)', 'x (INSTALL:2)', 'ratio 3:1', 'v 1.2:3',
                  'x (TX/RX:1,2)', 'x (Rack A/B:3)', 'Ethernet0 (Eth1/1:4)', 'port Eth1/1:4', 'x (RJ45/SFP:1-4)',
                  'speed 10G/25G:2', 'x (Slot A/B:2)', 'x (e1/1:2)'):
            self.assertEqual(strip(v), v, v)
        for v, left in (('NF5280M6 (DMI/Server/Inspur/NF5280/NF5280M6/48E15DC00C22:40)', 'NF5280M6'),
                        ('a (src/x/board_info.txt:5)', 'a'), ('a (fani.c:103,135)', 'a'),
                        ('a (x.dts:4, x.dts:9-12); b', 'a  ; b'), ('a (first entry k.json:4)', 'a'),
                        ('a (L866)', 'a'), ('a (line 12)', 'a'), ('a (p/q.c:1; r.c:2-3)', 'a'),
                        ('a foo/bar.c:12', 'a'), ('a (Server/Hyve/C4/C4I/09324DBA0E18:39)', 'a'),
                        ('a (rootconf/sysroot-lib-onie/gen-config-platform:7)', 'a'), ('a (src/mainboard/x/Kconfig:3)', 'a'), ('a foo.c:12-14', 'a')):
            self.assertEqual(strip(v).split(), left.split(), v)

    def test_citation_only_labelled_separately(self):
        with tempfile.TemporaryDirectory() as t:
            path = Path(t) / 'db.sqlite'
            c = Catalog(path)
            c.source('s', 'https://example.org/s', 'project', 'fixture', 'Fixture scope')
            c.add('s', 'a', 'https://example.org/a', 'Acme', 'ZX-9000', 'Acme ZX-9000', 'server', {}, [], identifiers=[],
                  facts=[('prod', 'ZX-9000 (DMI/Server/Acme/ZX-9000/AB12CD34EF56:3)'), ('c1', 'Acme (DMI/Server/Acme/ZX-9000/AB12CD34EF56:4)'),
                         ('both', 'x (src/zx-9000.c:1)'), ('both', 'ZX-9000 plain'),
                         ('port', 'ZX-9000/TX/RX:1,2'), ('port2', 'Ethernet0 (ZX-9000/Eth1/1:4)'),
                         ('c2', 'x (src/zx-9000.c:7)'), ('paren', 'ZX-9000 (C621)')])
            c.finish('s', 1)
            c.db.close()
            with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as db:
                db.row_factory = sqlite3.Row
                row = lookup.run(db, 'ZX-9000', 20)['datasets'][0]['rows'][0]
        self.assertEqual(row['matched_in'], 'identity; facts[5]: both, paren, port, port2, prod; cited-path only: 2 fact names')
        self.assertEqual(row['matched_in_detail']['facts_citation_only'], ['c1', 'c2'])
        self.assertEqual(row['matched_in_detail']['fact_name_count'], 5)
        self.assertNotIn('both', row['matched_in_detail']['facts_citation_only'])


if __name__ == '__main__':
    unittest.main()

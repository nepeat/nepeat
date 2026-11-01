import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_index as fcc, lookup
from devicehunt.catalog import Catalog, digest


def fixture(code='HED', count=1, page=1, total=1, offset=0, streamed=False):
    cards = []
    for n in range(offset, offset + count):
        fid = code + '-DEVICE-' + str(n)
        purpose = '<template id="P:1"></template>' if streamed else '<p>Original Equipment</p>'
        cards.append('<article><div><div><a href="/fcc/' + fid + '">' + fid + '</a>'
                     '<p>Fixture product, not a model alias</p></div><span>Jan 01, 2020</span></div>'
                     '<a href="/company/' + code + '">' + fcc.GRANTEES[code] + '</a>'
                     '<div><div><p>Equipment Class</p><a href="/equipment-class/dts">DTS</a></div>'
                     '<div><p>Application Purpose</p>' + purpose + '</div></div></article>')
    pages = max(1, (total + 24) // 25)
    text = ('<h1>' + fcc.GRANTEES[code] + '</h1><dl><div><dt>Grantee Code</dt><dd>' + code + '</dd></div>'
            '<div><dt>Total Applications</dt><dd>' + str(total) + '</dd></div></dl>' + ''.join(cards)
            + '<p>Page ' + str(page) + ' of ' + str(pages) + '</p>')
    if streamed:
        text += '<div hidden id="S:1"><p>Original Equipment</p></div><script>$RS("S:1","P:1")</script>'
    return text.encode()


class FccIndexTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root/'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def bundle(self, hed_pages=None):
        data = {'schema_version': 1, 'mirror': fcc.BASE, 'companies': {}}
        for code in fcc.GRANTEES:
            bodies = hed_pages if code == 'HED' and hed_pages is not None else [fixture(code=code)]
            if code in ('MRF', 'NVN'):
                bodies = [b'<title>Company Not Found | FCC ID Database</title>']
            data['companies'][code] = []
            for n, body in enumerate(bodies, 1):
                path = self.root/(code + '-' + str(n) + '.html'); path.write_bytes(body)
                data['companies'][code].append({'path': path.name, 'url': fcc.page_url(code, n),
                                               'sha256': digest(body)})
        path = self.root/'bundle.json'; path.write_text(json.dumps(data)); return path

    def test_streamed_fragment_reconstruction_does_not_execute_scripts(self):
        plain = fcc.parse_page(fixture(), 'HED', 1)
        text = fixture(streamed=True) + b'<script>throw new Error("must not execute");</script>'
        self.assertEqual(fcc.parse_page(text, 'HED', 1), plain)
        with self.assertRaisesRegex(ValueError, 'Unresolved'):
            fcc.parse_page(fixture(streamed=True).replace(b'$RS("S:1","P:1")', b'void(0)'), 'HED', 1)

    def test_company_mismatch_bad_date_or_missing_field_rejected(self):
        for before, after in [(b'<h1>Accton Technology Corp', b'<h1>Someone else'),
                              (b'Jan 01, 2020', b'Feb 31, 2020'),
                              (b'<p>Original Equipment</p>', b'<p></p>'),
                              (b'/fcc/HED-DEVICE-0', b'/fcc/OTHER-DEVICE-0')]:
            with self.assertRaises(ValueError): fcc.parse_page(fixture().replace(before, after), 'HED', 1)

    def test_pagination_requires_all_unique_cards_and_correct_page_numbers(self):
        full = [fixture(count=25, total=26), fixture(count=1, page=2, total=26, offset=25)]
        result = fcc.import_bundle(self.c, self.bundle(full))
        self.assertEqual(result['HED']['rows'], 26)
        for pages in [[full[0]], [full[0], fixture(count=1, page=2, total=26)],
                      [full[0], fixture(count=1, page=1, total=26, offset=25)],
                      [fixture(count=24, total=26), full[1]]]:
            with self.assertRaises(ValueError): fcc.import_bundle(self.c, self.bundle(pages))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_id_index WHERE grantee_code="HED"').fetchone()[0], 26)

    def test_import_preserves_mirror_fields_without_grants_or_model_aliases(self):
        for _ in range(2): result = fcc.import_bundle(self.c, self.bundle())
        self.assertEqual(result['MRF']['state'], 'not_found_in_mirror')
        self.assertIsNone(result['MRF']['pages'])
        self.assertEqual(self.c.search(), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0], 0)
        row = self.c.db.execute('SELECT * FROM fcc_id_index WHERE grantee_code="HED"').fetchone()
        self.assertEqual(row['listed_date'], 'Jan 01, 2020')
        section = next(s for s in lookup.run(self.c.db, 'HED-DEVICE')['datasets'] if s['dataset'] == 'fcc_ids')
        self.assertEqual(section['matches'], 1)
        self.assertEqual(len(section['sources']), 6)  # Include mirror-not-found states too.
        self.assertIn('not_found_in_mirror', next(s['coverage'] for s in section['sources'] if s['id']=='fcc-index-mrf'))

    def test_all_files_validated_before_any_source_replaced(self):
        path = self.bundle(); fcc.import_bundle(self.c, path)
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')]
        data = json.loads(path.read_text()); data['companies']['NVN'][0]['sha256'] = '0' * 64
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'checksum'): fcc.import_bundle(self.c, path)
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')])
        data['companies']['NVN'][0]['path'] = '../outside.html'; path.write_text(json.dumps(data))
        with self.assertRaises(ValueError): fcc.import_bundle(self.c, path)

    def test_missing_grantee_or_notfound_conflicting_with_rows_rejected(self):
        path = self.bundle(); data = json.loads(path.read_text()); del data['companies']['NVN']
        path.write_text(json.dumps(data))
        with self.assertRaises(ValueError): fcc.import_bundle(self.c, path)
        with self.assertRaises(ValueError): fcc.parse_page(b'<title>Company Not Found</title>' + fixture(), 'HED', 1)
        with self.assertRaises(ValueError): fcc.parse_page(b'<title>Service unavailable</title>', 'HED', 1)

    def test_duplicate_or_cyclic_stream_fragments_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            fcc.document('<div id="x"></div><div id="x"></div>')
        with self.assertRaisesRegex(ValueError, 'Cyclic'):
            fcc.document('<div id="S:1"><template id="P:1"></template></div><script>$RS("S:1","P:1")</script>')

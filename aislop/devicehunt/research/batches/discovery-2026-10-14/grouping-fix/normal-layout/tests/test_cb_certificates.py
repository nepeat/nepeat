import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import cb_certificates as cb
from devicehunt.catalog import Catalog, digest


def pages():
    result = []
    for cid, n in cb.REVIEWED:
        dc = n == 1
        rating = 'NCS1004: -48V, 44A NCS1014: -48V, 60A' if dc else 'NCS1004 100-240 Vac, 15A Additional Information on page 2'
        front = f'''1/2 Ref. Certif. No. {cid}
CB TEST CERTIFICATE Product Network Convergence System
Name and address of the applicant CISCO SYSTEMS INC Test address
Name and address of the manufacturer CISCO SYSTEMS INC Test address
Name and address of the factory
Note: When more than one factory, please report on page 2
CELESTICA (THAILAND) LIMITED Test factory address Additional Information on page 2
Ratings and principal characteristics {rating}
Trademark / Brand (if any) Cisco
Model / Type Ref. NCS1004, NCS1014, may be followed by additional alphanumeric characters or blank
Additional Information on page 2
Additional information (if necessary may also be reported on page 2) Test fixture
Date: 2023 -08-31 Original Issue Date: 2021 -01-28'''
        extra = '' if dc else 'Additional Ratings: NCS1014 100-240V~, 16A '
        back = f'''2/2 Ref. Certif. No. {cid} Factory(ies): FABRINET CO LTD Test second address
{extra}Summary of Modifications: Added NCS1014
Date: 2023 -08-31 Original Issue Date: 2021 -01-28'''
        result.extend([{'page': n, 'text': front}, {'page': n+1, 'text': back}])
    return result + [{'page': 5, 'text': 'Older certificate not selected'}, {'page': 6, 'text': 'Older continuation not selected'}]


class CBCertificateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def bundle(self, content=None):
        pdf = self.root / 'source.pdf'; pdf.write_bytes(b'%PDF-1.4 Test fixture')
        txt = self.root / 'text.json'
        txt.write_text(json.dumps({'pdf_sha256': digest(pdf.read_bytes()), 'extraction_method': 'pypdf embedded text; no OCR', 'pages': pages() if content is None else content}))
        def ref(p): return {'path': p.name, 'sha256': digest(p.read_bytes())}
        b = self.root / 'bundle.json'
        b.write_text(json.dumps({'schema_version': 1, 'url': cb.URL,
                                'reviewed_certificates': [c[0] for c in cb.REVIEWED],
                                'visually_reviewed_pages': [1, 2, 3, 4], 'pdf': ref(pdf), 'text': ref(txt)}))
        return b

    def test_cross_brand_factory_and_distinct_power_certificates(self):
        p = self.bundle()
        for _ in range(2):
            self.assertEqual(cb.import_bundle(self.c, p), {'certificates': 2, 'model_assertions': 4})
        rows = self.c.search(target='Celestica', role='listed_factory')
        self.assertEqual({(r['brand'], r['model']) for r in rows}, {('Cisco', 'NCS1004'), ('Cisco', 'NCS1014')})
        self.assertEqual(self.c.search(target='Celestica', role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 4)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM identifiers WHERE kind='alias'").fetchone()[0], 0)
        rows = [r[0] for r in self.c.db.execute("SELECT value FROM device_facts WHERE model='NCS1014' AND name='power_description' ORDER BY value")]
        self.assertEqual(len(rows), 2)
        self.assertTrue(any('60A' in r and 'US-37447-M3-UL' in r for r in rows))
        self.assertTrue(any('16A' in r and 'US-37446-M3-UL' in r for r in rows))
        self.assertFalse(any('44A' in r or '15A' in r for r in rows))

    def test_wrong_continuation_roles_date_and_model_ratings_rejected(self):
        for page, before, after in [(1, 'US-37447-M3-UL', 'US-37446-M3-UL'),
                                    (0, 'CISCO SYSTEMS INC', 'Other Manufacturer'),
                                    (0, 'CELESTICA (THAILAND) LIMITED', 'Other Factory'),
                                    (0, '2023 -08-31', '2024 -08-31'),
                                    (3, 'NCS1014 100-240V~', 'NCS9999 100-240V~'),
                                    (0, 'NCS1004, NCS1014,', 'NCS1004, NCS1004,')]:
            content = pages(); content[page]['text'] = content[page]['text'].replace(before, after)
            with self.assertRaises(ValueError): cb.import_bundle(self.c, self.bundle(content))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 0)

    def test_corrupt_refresh_preserves_imported_evidence(self):
        p = self.bundle(); cb.import_bundle(self.c, p)
        before = [tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        (self.root / 'source.pdf').write_bytes(b'%PDF- changed')
        with self.assertRaisesRegex(ValueError, 'checksum'): cb.import_bundle(self.c, p)
        self.assertEqual(before, [tuple(r) for r in self.c.db.execute('SELECT * FROM records')])
        p = self.bundle(); data = json.loads(p.read_text()); data['text']['path'] = '../outside.json'; p.write_text(json.dumps(data))
        with self.assertRaises(ValueError): cb.import_bundle(self.c, p)

    def test_missing_page_and_swapped_pdf_association_rejected(self):
        p = self.bundle(pages()[:-1])
        with self.assertRaisesRegex(ValueError, 'page count'): cb.import_bundle(self.c, p)
        p = self.bundle(); txt = self.root / 'text.json'
        data = json.loads(txt.read_text()); data['pdf_sha256'] = '0' * 64; txt.write_text(json.dumps(data))
        bundle = json.loads(p.read_text()); bundle['text']['sha256'] = digest(txt.read_bytes()); p.write_text(json.dumps(bundle))
        with self.assertRaisesRegex(ValueError, 'association'): cb.import_bundle(self.c, p)

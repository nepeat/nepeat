import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt.catalog import Catalog
from devicehunt import lineage


class LineageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.c = Catalog(self.root / 'catalog.sqlite')
        self.c.source('fixture', 'https://example.invalid/fixture', 'vendor', 'fixture', 'synthetic')
        db = self.c.db
        db.execute("INSERT INTO devices VALUES ('dev','Test','Model','','fixture','')")
        db.execute("INSERT INTO records VALUES ('r','dev','fixture','key','https://example.invalid','','{}')")
        db.execute("INSERT INTO facts VALUES ('r','width','435 mm')")
        db.execute("INSERT INTO facts VALUES ('r','width','17.3 inch')")
        db.commit()
        self.original = self.root / 'original.pdf'
        self.original.write_bytes(b'%PDF-fixture')
        self.sha = lineage.checksum(self.original)
        self.text = self.root / 'text.json'
        self.text.write_text(json.dumps({'pdf_sha256': self.sha,
            'extraction_method': 'pypdf embedded text; no OCR',
            'pages': [{'page': 1, 'text': '435 mm (17.3 inch)'}]}))
        self.text_sha = lineage.checksum(self.text)
        self.data = {'schema_version': 1,
            'artifacts': [{'sha256': lineage.checksum(p), 'byte_count': p.stat().st_size,
                           'paths': [p.name]} for p in (self.original, self.text)],
            'receipts': [{'id': 'saved', 'kind': 'saved-derivation', 'created_at': None,
                'payload': {'tool_version': None}, 'artifact_refs': [self.sha, self.text_sha],
                'receipt_refs': []}], 'statements': [], 'reviews': [], 'links': []}
        for ordinal, value in enumerate(('435 mm', '17.3 inch')):
            s = dict(claim_id='width-' + str(ordinal), source_id='fixture', source_key='key',
                     predicate='width', raw_value=value, normalized_value=None,
                     role='vendor_specification', projection_kind='fact',
                     projection={'name': 'width', 'value': value})
            s['statement_id'] = lineage.identity(s)
            s['accepted_review_id'] = 'review-' + str(ordinal)
            self.data['statements'].append(s)
            self.data['reviews'].append(dict(review_id=s['accepted_review_id'],
                statement_id=s['statement_id'], decision='accepted', reviewer='fixture',
                reviewed_at=None, reason='fixture only'))
            self.data['links'].append(dict(link_id='link-' + str(ordinal),
                statement_id=s['statement_id'], artifact_sha256=self.sha, receipt_id='saved',
                locator={'kind': 'pdf_page', 'page': 1, 'text_sha256': self.text_sha}, excerpt=value))
        self.path = self.root / 'release.json'
        self.save()

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def save(self):
        self.data.pop('release_id', None)
        self.data['release_id'] = lineage.identity(self.data)
        self.path.write_text(json.dumps(self.data))

    def test_offline_idempotence_conflicts_and_stable_source_replacement(self):
        with patch('urllib.request.urlopen', side_effect=AssertionError('no network')):
            lineage.import_bundle(self.c, self.path)
            lineage.import_bundle(self.c, self.path)
        self.assertEqual(self.c.db.execute('select count(*) from evidence_statements').fetchone()[0], 2)
        old = [r[0] for r in self.c.db.execute('select statement_id from evidence_statements')]
        self.c.source('fixture', 'https://example.invalid/fixture', 'vendor', 'fixture', 'replacement')
        result = lineage.show(self.c.db, 'key', self.root)
        self.assertEqual(len(result['statements']), 2)
        self.assertTrue(all(not s['projection_present'] for s in result['statements']))
        self.assertEqual(old, [r[0] for r in self.c.db.execute('select statement_id from evidence_statements')])

    def test_bad_artifact_or_unreviewed_statement_does_not_mutate(self):
        self.data['reviews'][0]['decision'] = 'rejected'
        self.save()
        with self.assertRaisesRegex(ValueError, 'explicitly accepted'):
            lineage.import_bundle(self.c, self.path)
        self.assertEqual(self.c.db.execute('select count(*) from archive_releases').fetchone()[0], 0)
        self.data['reviews'][0]['decision'] = 'accepted'
        self.save()
        self.original.write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            lineage.import_bundle(self.c, self.path)
        self.assertEqual(self.c.db.execute('select count(*) from facts').fetchone()[0], 2)

    def test_traversal_symlink_and_missing_files_rejected(self):
        self.data['artifacts'][0]['paths'] = ['../escape']
        self.save()
        with self.assertRaisesRegex(ValueError, 'path'):
            lineage.prepare(self.path)
        self.data['artifacts'][0]['paths'] = ['link.pdf']
        (self.root / 'link.pdf').symlink_to(self.original)
        self.save()
        with self.assertRaisesRegex(ValueError, 'symlink'):
            lineage.prepare(self.path)
        self.data['artifacts'][0]['paths'] = ['missing.pdf']
        self.save()
        with self.assertRaises(FileNotFoundError):
            lineage.prepare(self.path)

    def test_show_checks_text_ancestors_and_is_readonly(self):
        lineage.import_bundle(self.c, self.path)
        self.c.db.commit()
        before = (self.root / 'catalog.sqlite').read_bytes()
        with sqlite3.connect((self.root / 'catalog.sqlite').as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            self.assertEqual(len(lineage.show(db, 'key', self.root)['statements']), 2)
            self.text.write_text('corrupt text')
            with self.assertRaisesRegex(ValueError, 'checksum'):
                lineage.show(db, 'key', self.root)
        self.assertEqual((self.root / 'catalog.sqlite').read_bytes(), before)

    def test_invalid_graph_and_selector_rejected(self):
        self.data['receipts'][0]['receipt_refs'] = ['saved']
        self.save()
        with self.assertRaisesRegex(ValueError, 'Cyclic'):
            lineage.prepare(self.path)
        self.data['receipts'][0]['receipt_refs'] = []
        self.data['links'][0]['locator']['page'] = 2
        self.save()
        with self.assertRaisesRegex(ValueError, 'ordinal'):
            lineage.prepare(self.path)

    def test_collision_rolls_back_whole_release(self):
        lineage.import_bundle(self.c, self.path)
        self.c.db.commit()
        count = self.c.db.execute('select count(*) from archive_releases').fetchone()[0]
        self.data['receipts'][0]['payload']['tool_version'] = 'new conflicting payload'
        self.save()
        with self.assertRaisesRegex(ValueError, 'collision'):
            lineage.import_bundle(self.c, self.path)
        self.assertEqual(self.c.db.execute('select count(*) from archive_releases').fetchone()[0], count)

    def test_link_cannot_borrow_unrelated_receipt(self):
        self.data['receipts'][0]['artifact_refs'] = []
        self.save()
        with self.assertRaisesRegex(ValueError, 'unrelated'):
            lineage.prepare(self.path)


if __name__ == '__main__':
    unittest.main()

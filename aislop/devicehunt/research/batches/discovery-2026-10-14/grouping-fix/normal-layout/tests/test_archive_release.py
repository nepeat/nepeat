import json
import tempfile
import unittest
from pathlib import Path

from devicehunt.artifacts import Archive, canonical, digest
from devicehunt.archive import freeze, register_reference
from devicehunt import lineage
from devicehunt.documents import PYPDF_METHOD, POPPLER_METHOD
from tests.test_pdf_extraction import poppler_capture


class ArchiveReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.a = Archive(self.root / 'archive', workspace=self.root, create=True)

    def tearDown(self):
        self.a.close()
        self.tmp.cleanup()

    def test_frozen_release_independent_of_later_registry(self):
        sha = self.a.store_bytes(b'original')
        rid = self.a.receipt('legacy-capture', {'artifact_sha256': sha, 'bundle_sha256': sha})
        dest = self.root / 'release.json'
        result = freeze(self.a, dest)
        self.a.store_bytes(b'later pending evidence')
        self.assertEqual(lineage.prepare(dest), result)
        self.assertEqual(len(result['artifacts']), 1)
        self.assertIn(rid, [r['id'] for r in result['receipts']])
        with self.assertRaisesRegex(ValueError, 'already exists'):
            freeze(self.a, dest)

    def test_missing_registry_row_cannot_drop_durable_dependency(self):
        self.a.store_bytes(b'original')
        self.a.db.execute('delete from artifacts')
        self.a.db.commit()
        with self.assertRaisesRegex(ValueError, 'core receipt artifact'):
            freeze(self.a, self.root / 'release.json')
        self.assertFalse((self.root / 'release.json').exists())

    def test_complete_reference_registration_idempotent_and_unknown(self):
        pdf = self.root / 'fixture.pdf'
        pdf.write_bytes(b'%PDF-fixture')
        sha = lineage.checksum(pdf)
        text = self.root / 'fixture.json'
        text.write_text(json.dumps({'pdf_sha256': sha,
            'extraction_method': 'pypdf embedded text; no OCR',
            'pages': [{'page': 1, 'text': ''}]}))
        entry = dict(id='fixture', url='https://example.invalid/fixture', title='synthetic',
                     category='fixture', scope='synthetic only', tier='vendor', page_count=1,
                     pdf={'path': pdf.name, 'sha256': sha},
                     text={'path': text.name, 'sha256': lineage.checksum(text)})
        bundle = self.root / 'bundle.json'
        bundle.write_text(json.dumps({'schema_version': 1, 'collection': 'public-device-references-v1',
                                     'documents': [entry]}))
        def pin(p):
            return {'path': p.name, 'sha256': lineage.checksum(p), 'byte_count': p.stat().st_size}
        inventory = {'pins': {'complete_bundle': pin(bundle)}, 'dependencies': [pin(pdf), pin(text)],
                     'supporting_provenance': [], 'documents': [{'entry': entry}],
                     'special_lineage': {'document_id': 'not-present'}}
        path = self.root / 'inventory.json'
        path.write_text(json.dumps(inventory))
        first = register_reference(self.a, path)
        second = register_reference(self.a, path)
        self.assertEqual(first, second)
        capture = self.a.receipts('legacy-capture')[0]['payload']
        self.assertIsNone(capture['started_at'])
        self.assertIsNone(capture['collector_version'])
        self.assertEqual(self.a.receipts('reference-document')[0]['payload']['entry'], entry)
        expected_derivation = dict(
            inputs=[{'role': 'pdf', 'sha256': sha, 'order': 0}],
            outputs=[{'role': 'saved-output', 'sha256': lineage.checksum(text)}],
            method=PYPDF_METHOD, recipe_id=None, tool_version=None, code_sha256=None,
            configuration=None, extracted_at=None, historical_status=None,
            output_schema_version=None, metadata_status='unknown',
            locator={'bundle_sha256': lineage.checksum(bundle), 'pointer': '/documents/0'},
        )
        derivation = self.a.receipts('saved-derivation')[0]
        self.assertEqual(derivation['payload'], expected_derivation)
        self.assertEqual(derivation['id'], 'saved-derivation-' + digest(canonical(expected_derivation)))
        before = len(self.a.receipts())
        text.write_text('bad')
        with self.assertRaisesRegex(ValueError, 'Pinned inventory changed'):
            register_reference(self.a, path)
        self.assertEqual(len(self.a.receipts()), before)

    def test_mixed_method_registration_uses_sidecar_not_inventory_label(self):
        entries, dependencies = [], []
        def pin(p):
            return {'path': p.name, 'sha256': lineage.checksum(p), 'byte_count': p.stat().st_size}
        for name, method in [('one', PYPDF_METHOD), ('two', POPPLER_METHOD)]:
            pdf = self.root / (name + '.pdf')
            pdf.write_bytes(b'%PDF-fixture ' + name.encode())
            text = self.root / (name + '-text.json')
            capture = poppler_capture(lineage.checksum(pdf))
            if method == PYPDF_METHOD:
                capture = {'pdf_sha256': capture['pdf_sha256'], 'extraction_method': method,
                           'pages': capture['pages']}
            text.write_text(json.dumps(capture))
            # The pinned bundle is preserved verbatim; this untrusted field
            # must not become saved-derivation provenance.
            entries.append(dict(id=name, url='https://example.invalid/' + name,
                title='fixture', category='fixture', scope='synthetic only', tier='vendor',
                page_count=2, extraction_method='untrusted inventory method',
                pdf={'path': pdf.name, 'sha256': lineage.checksum(pdf)},
                text={'path': text.name, 'sha256': lineage.checksum(text)}))
            dependencies.extend([pin(pdf), pin(text)])
        bundle = self.root / 'mixed.json'
        bundle.write_text(json.dumps({'schema_version': 1, 'collection': 'public-device-references-v1',
                                     'documents': entries}))
        inventory = dict(pins={'complete_bundle': pin(bundle)}, dependencies=dependencies,
                         supporting_provenance=[], documents=[{'entry': e} for e in entries],
                         special_lineage={'document_id': 'not-present'})
        path = self.root / 'mixed-inventory.json'
        path.write_text(json.dumps(inventory))
        first = register_reference(self.a, path)
        self.assertEqual(first, register_reference(self.a, path))
        self.assertEqual({r['payload']['method'] for r in self.a.receipts('saved-derivation')},
                         {PYPDF_METHOD, POPPLER_METHOD})
        self.assertEqual(sorted((r['payload']['entry'] for r in self.a.receipts('reference-document')),
                                key=lambda e: e['id']), entries)
        before = len(self.a.receipts())
        inventory['documents'].reverse()
        path.write_text(json.dumps(inventory))
        with self.assertRaisesRegex(ValueError, 'complete selected bundle'):
            register_reference(self.a, path)
        self.assertEqual(len(self.a.receipts()), before)


if __name__ == '__main__':
    unittest.main()

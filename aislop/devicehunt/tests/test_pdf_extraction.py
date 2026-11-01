import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from devicehunt import documents
from scripts import extract_pdf_pages as utility


def poppler_capture(pdf_hash='a' * 64, pages=None):
    if pages is None:
        pages = [{'page': 1, 'text': 'MODEL-12-REV'}, {'page': 2, 'text': '  \n'}]
    return {
        'pdf_sha256': pdf_hash, 'extraction_method': documents.POPPLER_METHOD,
        'extractor_version': '26.06.0', 'pdfinfo_version': '26.06.0',
        'pdf_page_count': len(pages), 'extraction_options': dict(documents.POPPLER_OPTIONS),
        'pages': pages,
    }


class CaptureValidationTests(unittest.TestCase):
    def test_legacy_and_poppler_preserve_blank_whitespace(self):
        capture = poppler_capture()
        pages, meta = documents.read_capture(json.dumps(capture), capture['pdf_sha256'])
        self.assertEqual(pages, capture['pages'])
        self.assertEqual(meta['extraction_method'], documents.POPPLER_METHOD)
        legacy = {'pdf_sha256': capture['pdf_sha256'],
                  'extraction_method': documents.PYPDF_METHOD, 'pages': pages}
        self.assertEqual(documents.read_pages(json.dumps(legacy), capture['pdf_sha256']), pages)

    def test_reject_malformed_roots_pages_and_false_provenance(self):
        original = poppler_capture()
        mutations = [
            lambda d: d.update(pdf_sha256='b' * 64),
            lambda d: d.update(extraction_method='OCR'),
            lambda d: d.pop('extractor_version'),
            lambda d: d.update(extractor_version=True),
            lambda d: d.update(extractor_version='26.06.0 spoofed'),
            lambda d: d.update(pdfinfo_version='26.05.0'),
            lambda d: d.pop('extraction_options'),
            lambda d: d['extraction_options'].update(remove_hyphens='all'),
            lambda d: d['extraction_options'].update(layout=1),
            lambda d: d['extraction_options'].update(encoding='ASCII'),
            lambda d: d['extraction_options'].update(eol='dos'),
            lambda d: d['extraction_options'].update(page_selection='all'),
            lambda d: d['extraction_options'].update(page_breaks=True),
            lambda d: d['extraction_options'].update(extra=True),
            lambda d: d.pop('pdf_page_count'),
            lambda d: d.update(pdf_page_count=True),
            lambda d: d.update(pdf_page_count=3),
            lambda d: d.update(pages=[]),
            lambda d: d['pages'][0].update(page=True),
            lambda d: d['pages'][0].update(page=1.0),
            lambda d: d['pages'][1].update(page=1),
            lambda d: d['pages'].reverse(),
            lambda d: d['pages'][0].update(text=None),
            lambda d: d['pages'].__setitem__(0, 'not an object'),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                capture = copy.deepcopy(original)
                mutation(capture)
                with self.assertRaises(ValueError):
                    documents.read_pages(json.dumps(capture), original['pdf_sha256'])
        for root in [None, [], True, 1]:
            with self.subTest(root=root), self.assertRaises(ValueError):
                documents.read_pages(json.dumps(root), original['pdf_sha256'])
        legacy = dict(original, extraction_method=documents.PYPDF_METHOD,
                      pages=[{'page': True, 'text': ''}])
        with self.assertRaises(ValueError):
            documents.read_pages(json.dumps(legacy), original['pdf_sha256'])


class ExtractionUtilityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.pdf = self.root / 'model.pdf'
        self.pdf.write_bytes(b'%PDF-1.7 fixture')
        self.output = self.root / 'new.json'
        self.tools = self.root / 'tools'
        self.tools.mkdir()
        for name in ('pdfinfo', 'pdftotext'):
            tool = self.tools / name
            tool.touch()
            tool.chmod(0o700)

    def result(self, stdout=b'', stderr=b'', code=0):
        return subprocess.CompletedProcess([], code, stdout, stderr)

    def results(self, info=b'Pages: 2\nEncrypted: no\n'):
        return [
            self.result(stderr=b'pdftotext version 26.06.0\n'),
            self.result(stderr=b'pdfinfo version 26.06.0\n'),
            self.result(info),
            self.result(b'MODEL-12-REV\n  exact spacing\n', b'warning retained\n'),
            self.result(b''),
        ]

    def assert_no_capture(self):
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('.pdf-capture-*')), [])

    def test_explicit_arguments_preserve_identifiers_spacing_and_blank_page(self):
        with patch.object(utility.subprocess, 'run', side_effect=self.results()) as run:
            capture = utility.extract(self.pdf, self.output, self.tools)
        self.assertEqual(capture['pages'], [
            {'page': 1, 'text': 'MODEL-12-REV\n  exact spacing\n'},
            {'page': 2, 'text': ''},
        ])
        self.assertEqual(documents.read_pages(self.output.read_bytes(), capture['pdf_sha256']),
                         capture['pages'])
        self.assertEqual(capture['provenance']['commands'][3]['stderr'], 'warning retained\n')
        self.assertEqual(run.call_count, 5)
        for ordinal, call in enumerate(run.call_args_list[3:], 1):
            self.assertEqual(call.args[0], [
                str(self.tools / 'pdftotext'), '-f', str(ordinal), '-l', str(ordinal),
                '-layout', '-enc', 'UTF-8', '-eol', 'unix', '-remove-hyphens',
                'none', '-nopgbrk', str(self.pdf), '-',
            ])
            self.assertIs(call.kwargs['shell'], False)
            self.assertEqual(call.kwargs['timeout'], 30)
            self.assertEqual(call.kwargs['env']['LC_ALL'], 'C')

    def test_failed_or_timeout_page_does_not_publish(self):
        for failure in [self.result(stderr=b'failed page', code=1),
                        subprocess.TimeoutExpired('mocked', 30)]:
            with self.subTest(failure=failure):
                results = self.results()
                results[-1] = failure
                with patch.object(utility.subprocess, 'run', side_effect=results):
                    with self.assertRaises((ValueError, subprocess.TimeoutExpired)):
                        utility.extract(self.pdf, self.output, self.tools)
                self.assert_no_capture()

    def test_version_count_encryption_and_delimiter_rejection(self):
        variants = [
            (1, self.result(stderr=b'pdfinfo version 26.05.0')),
            (2, self.result(b'Pages: 0\nEncrypted: no\n')),
            (2, self.result(b'Pages: 2\nEncrypted: yes (print:yes)\n')),
            (2, self.result(b'Pages: 2\n')),
            (3, self.result(b'text\f')),
            (3, self.result(b'\xff')),
        ]
        for index, result in variants:
            with self.subTest(index=index, result=result):
                results = self.results()
                results[index] = result
                with patch.object(utility.subprocess, 'run', side_effect=results):
                    with self.assertRaises(ValueError):
                        utility.extract(self.pdf, self.output, self.tools)
                self.assert_no_capture()

    def test_changed_source_does_not_publish(self):
        results = iter(self.results())
        calls = 0
        def run(*args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 5:
                self.pdf.write_bytes(b'%PDF-1.7 changed')
            return next(results)
        with patch.object(utility.subprocess, 'run', side_effect=run):
            with self.assertRaisesRegex(ValueError, 'changed'):
                utility.extract(self.pdf, self.output, self.tools)
        self.assert_no_capture()

    def test_existing_output_and_publish_race_never_overwrite(self):
        self.output.write_bytes(b'keep original')
        with patch.object(utility.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'already exists'):
                utility.extract(self.pdf, self.output, self.tools)
            run.assert_not_called()
        self.assertEqual(self.output.read_bytes(), b'keep original')
        self.output.unlink()
        def race(*args):
            self.output.write_bytes(b'created concurrently')
            raise FileExistsError('race')
        with patch.object(utility.subprocess, 'run', side_effect=self.results()):
            with patch.object(utility.os, 'link', side_effect=race):
                with self.assertRaises(FileExistsError):
                    utility.extract(self.pdf, self.output, self.tools)
        self.assertEqual(self.output.read_bytes(), b'created concurrently')
        self.assertEqual(list(self.root.glob('.pdf-capture-*')), [])

    def test_invalid_pdf_header_is_rejected_before_tool_calls(self):
        self.pdf.write_bytes(b'HTML error page')
        with patch.object(utility.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'not a PDF'):
                utility.extract(self.pdf, self.output, self.tools)
            run.assert_not_called()
        self.assert_no_capture()

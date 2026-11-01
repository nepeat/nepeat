import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt.rebuild import file_sha256, rebuild


class RebuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.input = self.root / 'source.json'
        self.input.write_text(json.dumps({
            'sources': [{'id': 'fixture', 'url': 'https://example.org/device',
                         'tier': 'vendor', 'license': 'test', 'coverage': 'test'}],
            'devices': [{'source': 'fixture', 'key': 'E5', 'brand': 'Oracle',
                         'model': 'E5-2C', 'title': 'Oracle E5-2C', 'kind': 'server',
                         'affiliations': [['Oracle', 'brand']]}]}))
        self.manifest = self.root / 'manifest.json'
        self.data = {'schema_version': 1,
                     'imports': [{'source': 'curated', 'path': 'source.json'}],
                     'files': {'source.json': file_sha256(self.input)},
                     'expected': {'devices': 1, 'records': 1, 'sources': {'fixture': 1}}}
        self.save()
        self.output = self.root / 'result.sqlite'

    def tearDown(self):
        self.temp.cleanup()

    def save(self):
        self.manifest.write_text(json.dumps(self.data))

    def test_offline_rebuild_and_exclusive_destination(self):
        with patch('urllib.request.urlopen', side_effect=AssertionError('Network forbidden')):
            result = rebuild(self.manifest, self.output)
        self.assertEqual(result['records'], 1)
        with sqlite3.connect(self.output) as db:
            self.assertEqual(db.execute('SELECT model FROM devices').fetchone()[0], 'E5-2C')
        before = self.output.read_bytes()
        with self.assertRaisesRegex(ValueError, 'already exists'):
            rebuild(self.manifest, self.output)
        self.assertEqual(self.output.read_bytes(), before)

    def test_missing_or_changed_snapshot_never_publishes(self):
        self.input.write_text('modified')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            rebuild(self.manifest, self.output)
        self.assertFalse(self.output.exists())
        self.input.unlink()
        with self.assertRaises(OSError):
            rebuild(self.manifest, self.output)
        self.assertFalse(self.output.exists())

    def test_count_mismatch_and_parser_failure_remove_staging(self):
        self.data['expected']['records'] = 2
        self.save()
        with self.assertRaisesRegex(ValueError, 'counts differ'):
            rebuild(self.manifest, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('.devicehunt-build-*')), [])
        # Failure after an earlier importer committed still publishes nothing.
        bad = self.root / 'bad.html'
        bad.write_text('not an Oracle source index')
        self.data['files']['bad.html'] = file_sha256(bad)
        self.data['imports'].append({'source': 'oracle-oss', 'path': 'bad.html'})
        self.save()
        with self.assertRaises(ValueError):
            rebuild(self.manifest, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('.devicehunt-build-*')), [])

    def test_outside_paths_and_duplicate_jobs_rejected(self):
        self.data['files'] = {'../outside': 'a' * 64}
        self.save()
        with self.assertRaisesRegex(ValueError, 'escapes'):
            rebuild(self.manifest, self.output)
        self.data['files'] = {'source.json': file_sha256(self.input)}
        self.data['imports'] *= 2
        self.save()
        with self.assertRaisesRegex(ValueError, 'Duplicate import'):
            rebuild(self.manifest, self.output)

    def test_change_during_import_detected(self):
        def changed(_):
            self.input.write_text('changed while importing')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            rebuild(self.manifest, self.output, progress=changed)
        self.assertFalse(self.output.exists())

    def test_manifest_change_during_import_detected(self):
        def changed(_):
            self.data['description'] = 'different manifest'
            self.save()
        with self.assertRaisesRegex(ValueError, 'manifest changed'):
            rebuild(self.manifest, self.output, progress=changed)
        self.assertFalse(self.output.exists())

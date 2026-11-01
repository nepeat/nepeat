import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from devicehunt import storage


class StorageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'root'
        self.root.mkdir()
        self.archive = Path(self.temp.name) / 'archive'

    def source(self, name='data/old.sqlite', content=b'SQLite fixture\x00' * 1000):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def test_duplicate_files_chunks_restore_exact_bytes(self):
        raw = b'x' * storage.CHUNK_SIZE * 2 + b'end'
        self.source(content=raw)
        self.source('history/same.sqlite', raw)
        manifest = storage.archive(self.root, ['data/old.sqlite', 'history/same.sqlite'], self.archive)
        self.assertEqual(len(list(self.archive.rglob('*.zlib'))), 2)
        self.assertEqual(manifest['files'][0]['sha256'], hashlib.sha256(raw).hexdigest())
        target = Path(self.temp.name) / 'restored'
        storage.restore(self.archive, target)
        self.assertEqual((target / 'data/old.sqlite').read_bytes(), raw)
        self.assertEqual((target / 'history/same.sqlite').read_bytes(), raw)
        self.assertTrue((self.root / 'data/old.sqlite').exists())
        with self.assertRaises(FileExistsError):
            storage.restore(self.archive, target)

    def test_inventory_is_readonly(self):
        path = self.source()
        before = path.stat()
        result = storage.inventory(self.root)
        self.assertEqual(result['logical_bytes'], before.st_size)
        self.assertEqual(path.stat().st_mtime_ns, before.st_mtime_ns)
        self.assertEqual(list(self.root.rglob('*')), [self.root / 'data', path])

    def test_protected_paths_and_keep(self):
        for name, keep in [('data/devices.sqlite', []), ('sources/old.sqlite', []),
                           ('data/old.sqlite', ['data/old.sqlite']), ('data/import-manifest.json', [])]:
            with self.assertRaises(storage.StorageError):
                storage.archive(self.root, [name], self.archive, keep=keep, remove_originals=True)

    def test_sidecars_prevent_archive_and_remove(self):
        path = self.source()
        for suffix in ('-wal', '-shm', '-journal'):
            sidecar = Path(str(path) + suffix)
            sidecar.touch()
            with self.assertRaises(storage.StorageError):
                storage.archive(self.root, ['data/old.sqlite'], self.archive)
            sidecar.unlink()
        storage.archive(self.root, ['data/old.sqlite'], self.archive)
        Path(str(path) + '-wal').touch()
        with self.assertRaises(storage.StorageError):
            storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        self.assertTrue(path.exists())

    def test_corrupted_or_missing_object_never_removes(self):
        path = self.source()
        storage.archive(self.root, ['data/old.sqlite'], self.archive)
        obj = next(self.archive.rglob('*.zlib'))
        obj.write_bytes(b'broken')
        with self.assertRaises(Exception):
            storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        self.assertTrue(path.exists())
        obj.unlink()
        with self.assertRaises(FileNotFoundError):
            storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        self.assertTrue(path.exists())

    def test_changed_source_preflight_preserves_all(self):
        first = self.source()
        second = self.source('data/second.sqlite')
        paths = ['data/old.sqlite', 'data/second.sqlite']
        storage.archive(self.root, paths, self.archive)
        second.write_bytes(b'changed')
        with self.assertRaises(storage.StorageError):
            storage.archive(self.root, paths, self.archive, remove_originals=True)
        self.assertTrue(first.exists())
        self.assertTrue(second.exists())

    def test_crash_before_manifest_retains_original_and_resumes(self):
        source = self.source()
        original = storage._atomic_new
        def fail_manifest(path, raw):
            if path.name == 'manifest.json':
                raise OSError('simulated crash')
            return original(path, raw)
        with patch.object(storage, '_atomic_new', side_effect=fail_manifest):
            with self.assertRaises(OSError):
                storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        self.assertTrue(source.exists())
        self.assertFalse((self.archive / 'manifest.json').exists())
        storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        self.assertFalse(source.exists())
        storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        storage.verify(self.archive)

    def test_traversal_symlink_manifest_and_file_hash(self):
        self.source()
        for name in ('../old.sqlite', '/tmp/old.sqlite', 'data/./old.sqlite'):
            with self.assertRaises(storage.StorageError):
                storage.archive(self.root, [name], self.archive)
        alias = self.root / 'alias'
        alias.symlink_to(self.root / 'data', target_is_directory=True)
        with self.assertRaises(storage.StorageError):
            storage.archive(self.root, ['alias/old.sqlite'], self.archive)
        storage.archive(self.root, ['data/old.sqlite'], self.archive)
        manifest_path = self.archive / 'manifest.json'
        manifest = json.loads(manifest_path.read_text())
        manifest['files'][0]['sha256'] = '0' * 64
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaises(storage.StorageError):
            storage.verify(self.archive)
        manifest['files'][0]['path'] = '../escape.sqlite'
        manifest_path.write_text(json.dumps(manifest))
        target = Path(self.temp.name) / 'restored'
        with self.assertRaises(storage.StorageError):
            storage.restore(self.archive, target)
        self.assertFalse(target.exists())

    def test_empty_file_and_immutable_selection(self):
        self.source(content=b'')
        storage.archive(self.root, ['data/old.sqlite'], self.archive)
        storage.verify(self.archive)
        with self.assertRaises(storage.StorageError):
            storage.archive(self.root, ['data/other.sqlite'], self.archive)

    def test_crash_during_removal_is_resumable(self):
        self.source()
        self.source('data/second.sqlite')
        paths = ['data/old.sqlite', 'data/second.sqlite']
        original = Path.unlink
        def crash(path, *args, **kwargs):
            if path.name == 'second.sqlite':
                raise OSError('simulated unlink failure')
            return original(path, *args, **kwargs)
        with patch.object(Path, 'unlink', crash):
            with self.assertRaises(OSError):
                storage.archive(self.root, paths, self.archive, remove_originals=True)
        self.assertFalse((self.root / paths[0]).exists())
        self.assertTrue((self.root / paths[1]).exists())
        storage.verify(self.archive)
        storage.archive(self.root, paths, self.archive, remove_originals=True)
        restored = Path(self.temp.name) / 'recovery'
        storage.restore(self.archive, restored)
        self.assertTrue((restored / paths[0]).exists())
        self.assertTrue((restored / paths[1]).exists())

    def test_archive_object_and_restore_destination_symlinks_rejected(self):
        self.source()
        storage.archive(self.root, ['data/old.sqlite'], self.archive)
        destination = Path(self.temp.name) / 'link'
        destination.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(storage.StorageError):
            storage.restore(self.archive, destination)
        obj = next(self.archive.rglob('*.zlib'))
        replacement = Path(self.temp.name) / 'object'
        replacement.write_bytes(obj.read_bytes())
        obj.unlink()
        obj.symlink_to(replacement)
        with self.assertRaises(storage.StorageError):
            storage.archive(self.root, ['data/old.sqlite'], self.archive, remove_originals=True)
        self.assertTrue((self.root / 'data/old.sqlite').exists())

    def test_live_basename_protected_with_changed_root(self):
        self.source('data/devices.sqlite')
        with self.assertRaises(storage.StorageError):
            storage.archive(self.root / 'data', ['devices.sqlite'], self.archive)

    def test_selected_restore_still_verifies_whole_archive(self):
        self.source()
        self.source('data/second.sqlite', b'other content')
        manifest = storage.archive(self.root, ['data/old.sqlite', 'data/second.sqlite'], self.archive)
        target = Path(self.temp.name) / 'selected'
        storage.restore(self.archive, target, paths=['data/old.sqlite'])
        self.assertTrue((target / 'data/old.sqlite').exists())
        self.assertFalse((target / 'data/second.sqlite').exists())
        with self.assertRaises(storage.StorageError):
            storage.restore(self.archive, Path(self.temp.name) / 'unknown', paths=['absent.sqlite'])
        other = manifest['files'][1]['chunks'][0]['sha256']
        storage._object(self.archive, other).write_bytes(b'broken')
        with self.assertRaises(Exception):
            storage.restore(self.archive, Path(self.temp.name) / 'failed', paths=['data/old.sqlite'])
        self.assertFalse((Path(self.temp.name) / 'failed').exists())

    def test_sources_protected_with_changed_root_and_nested_path(self):
        self.source('sources/old.sqlite')
        self.source('nested/sources/old.sqlite')
        for root, name in ((self.root / 'sources', 'old.sqlite'),
                           (self.root, 'nested/sources/old.sqlite')):
            with self.assertRaises(storage.StorageError):
                storage.archive(root, [name], self.archive, remove_originals=True)

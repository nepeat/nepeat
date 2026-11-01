import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch
from devicehunt.artifacts import Archive, ArchiveRecoveryError, digest


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.workspace = Path(self.tmp.name)
        self.archive = Archive(self.workspace/'archive',workspace=self.workspace,create=True)

    def tearDown(self):
        self.archive.close()
        self.tmp.cleanup()

    def test_registration_and_copies(self):
        a = self.workspace/'a'
        a.write_bytes(b'original')
        sha = self.archive.register_file(a)
        self.assertEqual(sha,self.archive.register_file(a))
        self.assertEqual(len(self.archive.receipts()),1)
        self.assertFalse(self.archive.object_path(sha).exists())
        b = self.workspace/'b'
        b.write_bytes(b'original')
        self.assertEqual(sha,self.archive.register_file(b))
        self.assertEqual(len(self.archive.receipts()),2)
        self.assertEqual(sha,self.archive.store_bytes(b'original'))
        self.assertNotEqual(a.stat().st_ino,self.archive.object_path(sha).stat().st_ino)
        a.write_bytes(b'changed')
        self.assertEqual(self.archive.resolve(sha).read_bytes(),b'original')
        self.assertFalse(self.archive.audit()['ok'])

    def test_missing_corruption_and_validation(self):
        sha = self.archive.store_bytes(b'one')
        p = self.archive.resolve(sha)
        p.chmod(0o644)
        p.write_bytes(b'two')
        with self.assertRaises(ValueError): self.archive.resolve(sha)
        p.unlink()
        with self.assertRaises(ValueError): self.archive.resolve(sha)
        with self.assertRaises(ValueError): self.archive.resolve('../x')
        file = self.workspace/'raw'
        file.write_bytes(b'one')
        with self.assertRaises(ValueError): self.archive.register_file(file,expected_sha256='a'*64)
        with self.assertRaises(ValueError): self.archive.register_file(self.workspace/'../outside')
        with self.assertRaises(ValueError): self.archive.register_file('/etc/passwd')
        symlink = self.workspace/'link'
        symlink.symlink_to(file)
        with self.assertRaises(ValueError): self.archive.register_file(symlink)

    def test_receipts_freeze_and_distinct_attempts(self):
        first = self.archive.receipt('capture',{'url':'https://fixture','sha256':None},'stable')
        original = self.archive.get_receipt(first)
        self.assertEqual(first,self.archive.receipt('capture',original['payload'],'stable'))
        self.assertEqual(original,self.archive.get_receipt(first))
        self.assertNotEqual(first,self.archive.receipt('capture',original['payload']))
        with self.assertRaises(ValueError): self.archive.receipt('capture',{'x':1},'stable')

    def test_interrupt_and_orphan(self):
        with patch('devicehunt.artifacts.os.link',side_effect=OSError('interrupted')):
            with self.assertRaises(OSError): self.archive.store_bytes(b'partial')
        self.assertEqual(list((self.archive.root/'objects').rglob('.pending-*')),[])
        sha = digest(b'orphan')
        p = self.archive.object_path(sha)
        p.parent.mkdir(parents=True,exist_ok=True)
        p.write_bytes(b'orphan')
        self.assertEqual(self.archive.audit()['orphan_objects'],[p.relative_to(self.archive.root).as_posix()])

    def test_reconstruct_and_restore(self):
        source = self.workspace/'raw'
        source.write_bytes(b'legacy')
        legacy = self.archive.register_file(source)
        sha = self.archive.store_bytes(b'new')
        original = self.archive.receipts()
        self.archive.close()
        (self.workspace/'archive'/'index.sqlite').unlink()
        with self.assertRaises(ArchiveRecoveryError):
            Archive(self.workspace/'archive',workspace=self.workspace,create=True)
        Archive.rebuild(self.workspace/'archive',self.workspace,quiesced=True,decision='fixture explicit rebuild')
        self.archive = Archive(self.workspace/'archive',workspace=self.workspace)
        self.assertEqual(original,self.archive.receipts())
        self.assertEqual(self.archive.resolve(sha).read_bytes(),b'new')
        restore = self.workspace/'restored'
        shutil.copytree(self.workspace/'archive',restore/'archive')
        shutil.copy2(source,restore/'raw')
        copied = Archive(restore/'archive',workspace=restore)
        self.assertEqual(copied.resolve(legacy).read_bytes(),b'legacy')
        self.assertTrue(copied.audit()['ok'])
        copied.close()

    def test_readonly_and_absent(self):
        sha = self.archive.store_bytes(b'unchanged')
        before = {p.relative_to(self.archive.root).as_posix():p.read_bytes() for p in self.archive.root.rglob('*') if p.is_file()}
        other = Archive(self.archive.root,workspace=self.workspace,readonly=True)
        self.assertEqual(other.resolve(sha).read_bytes(),b'unchanged')
        other.audit()
        with self.assertRaises(PermissionError): other.store_bytes(b'bad')
        other.close()
        after = {p.relative_to(self.archive.root).as_posix():p.read_bytes() for p in self.archive.root.rglob('*') if p.is_file()}
        self.assertEqual(before,after)
        with self.assertRaises(FileNotFoundError): Archive(self.workspace/'absent')
        self.assertFalse((self.workspace/'absent').exists())

    def test_invalid_receipt_reindex_leaves_index(self):
        sha = self.archive.store_bytes(b'preserved')
        bad = self.archive.root/'receipts'/'bad.json'
        bad.write_text(json.dumps({'id':'bad','kind':'artifact-registration','created_at':'unknown','payload':{'sha256':sha,'size':9,'scope':'workspace','path':'../escape'}}))
        with self.assertRaises(ValueError): self.archive.reindex(quiesced=True,decision='fixture index audit')
        self.assertEqual(self.archive.resolve(sha).read_bytes(),b'preserved')
        bad.unlink()

    def test_existing_stable_wrapper_keeps_original_bytes_and_sequence(self):
        identifier=self.archive.receipt('attempt',{'a':1},'same')
        path=self.archive.root/'receipts'/(identifier+'.json')
        original=path.read_bytes()
        before=self.archive.require_ready()['sequence']
        self.assertEqual(identifier,self.archive.receipt('attempt',{'a':1},'same'))
        self.assertEqual(path.read_bytes(),original)
        self.assertEqual(self.archive.require_ready()['sequence'],before)
        # Real competing writers and rebuild exclusion are exercised in
        # test_archive_index.py; no test bypasses the public publication protocol.

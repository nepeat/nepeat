"""Lossless, offline, chunk-deduplicated archival of explicit SQLite snapshots.

The archive is a byte backup, not a SQLite logical dump. No SQLite connections
are opened. Originals are retained unless removal is explicitly requested.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import tempfile
import zlib

VERSION = 1
CHUNK_SIZE = 1024 * 1024  # multiple of every supported SQLite page size


class StorageError(ValueError):
    pass


def _relative(value):
    if not isinstance(value, str) or not value or '\\' in value:
        raise StorageError('invalid relative path')
    p = PurePosixPath(value)
    if p.is_absolute() or any(x in ('', '.', '..') for x in value.split('/')):
        raise StorageError('unsafe relative path: ' + value)
    return p


def _safe(path):
    path = Path(path).absolute()
    for part in (path, *path.parents):
        if part.is_symlink():
            raise StorageError('symlink prohibited: ' + str(part))
    return path


def _file(root, relative):
    return _safe(root / str(_relative(relative)))


def _hash(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(CHUNK_SIZE), b''):
            digest.update(chunk)
    return digest.hexdigest()


def _regular(path):
    if not stat.S_ISREG(path.stat().st_mode):
        raise StorageError('not a regular file: ' + str(path))


def _sidecars(path):
    for suffix in ('-wal', '-shm', '-journal'):
        if Path(str(path) + suffix).exists() or Path(str(path) + suffix).is_symlink():
            raise StorageError('SQLite sidecar exists: ' + str(path) + suffix)


def _atomic_new(path, data):
    """Publish immutable bytes, never replace an existing path."""
    _safe(path.parent).mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix='.pending-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.link(name, path)  # exclusive publication; failure preserves existing bytes
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        os.unlink(name)


def inventory(root):
    """Read metadata only; no hashes, connections or file mutation."""
    root = _safe(root)
    entries = []
    for path in sorted(root.rglob('*.sqlite')):
        _safe(path)
        _regular(path)
        st = path.stat()
        entries.append({'path': path.relative_to(root).as_posix(),
                        'size': st.st_size, 'allocated': st.st_blocks * 512})
    return {'version': VERSION, 'files': entries,
            'logical_bytes': sum(e['size'] for e in entries),
            'allocated_bytes': sum(e['allocated'] for e in entries)}


def _object(archive_dir, digest):
    if not isinstance(digest, str) or len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):
        raise StorageError('invalid SHA-256')
    return _file(archive_dir, 'objects/' + digest[:2] + '/' + digest + '.zlib')


def _read_object(archive_dir, digest, size):
    path = _object(archive_dir, digest)
    _regular(path)
    # Limit output to declared length, rejecting bombs and trailing bytes.
    if path.stat().st_size > CHUNK_SIZE + 65536:
        raise StorageError('oversized compressed object: ' + digest)
    decoder = zlib.decompressobj()
    raw = decoder.decompress(path.read_bytes(), size + 1)
    if len(raw) != size or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise StorageError('invalid compressed object: ' + digest)
    if hashlib.sha256(raw).hexdigest() != digest:
        raise StorageError('object hash mismatch: ' + digest)
    return raw


def _manifest(archive_dir):
    archive_dir = _safe(archive_dir)
    manifest = json.loads(_file(archive_dir, 'manifest.json').read_text())
    if manifest.get('version') != VERSION or manifest.get('chunk_size') != CHUNK_SIZE:
        raise StorageError('unsupported archive format')
    if not isinstance(manifest.get('files'), list):
        raise StorageError('invalid files')
    seen = set()
    for entry in manifest['files']:
        name = str(_relative(entry['path']))
        if name in seen:
            raise StorageError('duplicate path')
        seen.add(name)
        _object(archive_dir, entry['sha256'])  # validate digest syntax
        if type(entry['size']) is not int or entry['size'] < 0:
            raise StorageError('invalid size')
        total = 0
        for chunk in entry['chunks']:
            _object(archive_dir, chunk['sha256'])
            if type(chunk['size']) is not int or not 0 < chunk['size'] <= CHUNK_SIZE:
                raise StorageError('invalid chunk size')
            total += chunk['size']
        if total != entry['size']:
            raise StorageError('file length mismatch')
    return manifest


def verify(archive_dir):
    archive_dir = _safe(archive_dir)
    manifest = _manifest(archive_dir)
    for entry in manifest['files']:
        digest = hashlib.sha256()
        for chunk in entry['chunks']:
            digest.update(_read_object(archive_dir, chunk['sha256'], chunk['size']))
        if digest.hexdigest() != entry['sha256']:
            raise StorageError('reconstructed file hash mismatch: ' + entry['path'])
    return manifest


def _selection(paths, keep):
    keep = {str(_relative(p)) for p in keep}
    names = sorted(str(_relative(p)) for p in paths)
    if not names or len(names) != len(set(names)):
        raise StorageError('provide distinct explicit snapshot paths')
    for name in names:
        if PurePosixPath(name).name == 'devices.sqlite' or name.startswith('sources/') or name in keep or not name.endswith('.sqlite'):
            raise StorageError('protected or non-snapshot path: ' + name)
    return names


def archive(root, paths, archive_dir, *, keep=(), remove_originals=False):
    root, archive_dir = _safe(root), _safe(archive_dir)
    names = _selection(paths, keep)
    # Archives must not contain their own input files.
    for name in names:
        source = _file(root, name)
        if 'sources' in source.parts:
            raise StorageError('immutable sources path: ' + str(source))
        if source == archive_dir or archive_dir in source.parents:
            raise StorageError('source is inside archive')
    manifest_path = _file(archive_dir, 'manifest.json')
    if manifest_path.exists():
        manifest = verify(archive_dir)
        if [e['path'] for e in manifest['files']] != names:
            raise StorageError('existing immutable manifest has different selection')
    else:
        entries = []
        for name in names:
            source = _file(root, name)
            _regular(source)
            _sidecars(source)
            before = source.stat()
            digest, chunks = hashlib.sha256(), []
            with source.open('rb') as stream:
                for raw in iter(lambda: stream.read(CHUNK_SIZE), b''):
                    digest.update(raw)
                    sha = hashlib.sha256(raw).hexdigest()
                    target = _object(archive_dir, sha)
                    if target.exists():
                        _read_object(archive_dir, sha, len(raw))
                    else:
                        try:
                            _atomic_new(target, zlib.compress(raw, 6))
                        except FileExistsError:
                            _read_object(archive_dir, sha, len(raw))
                    chunks.append({'sha256': sha, 'size': len(raw)})
            after = source.stat()
            if (before.st_size, before.st_mtime_ns, before.st_ino) != (after.st_size, after.st_mtime_ns, after.st_ino):
                raise StorageError('source changed during archival: ' + name)
            entries.append({'path': name, 'size': before.st_size,
                            'sha256': digest.hexdigest(), 'chunks': chunks})
        manifest = {'version': VERSION, 'chunk_size': CHUNK_SIZE, 'files': entries}
        _atomic_new(manifest_path, (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode())
    verify(archive_dir)  # every object AND every reconstructed file, before removal
    # Preflight every remaining original before deleting any; absent files allow resume.
    for entry in manifest['files']:
        source = _file(root, entry['path'])
        _sidecars(source)
        if source.exists():
            _regular(source)
            if source.stat().st_size != entry['size'] or _hash(source) != entry['sha256']:
                raise StorageError('source hash changed: ' + entry['path'])
        elif not remove_originals:
            raise StorageError('original missing: ' + entry['path'])
    if remove_originals:
        for entry in manifest['files']:
            source = _file(root, entry['path'])
            _sidecars(source)
            if source.exists():
                # Recheck directly before unlink, including after earlier removals.
                if _hash(source) != entry['sha256']:
                    raise StorageError('source changed before removal: ' + entry['path'])
                source.unlink()
    return manifest


def restore(archive_dir, destination, *, paths=None):
    """Verify first, then restore into a newly created directory only."""
    archive_dir, destination = _safe(archive_dir), _safe(destination)
    manifest = verify(archive_dir)
    selected = None if paths is None else {str(_relative(p)) for p in paths}
    if selected is not None and (not selected or not selected <= {e['path'] for e in manifest['files']}):
        raise StorageError('restore selection contains unknown paths')
    destination.mkdir(parents=True, exist_ok=False)
    for entry in manifest['files']:
        if selected is not None and entry['path'] not in selected:
            continue
        target = _file(destination, entry['path'])
        target.parent.mkdir(parents=True, exist_ok=True)
        # Each file is published only after its complete bytes pass the file hash.
        fd, pending = tempfile.mkstemp(prefix='.restore-', dir=target.parent)
        try:
            digest = hashlib.sha256()
            with os.fdopen(fd, 'wb') as stream:
                for chunk in entry['chunks']:
                    raw = _read_object(archive_dir, chunk['sha256'], chunk['size'])
                    digest.update(raw)
                    stream.write(raw)
                stream.flush()
                os.fsync(stream.fileno())
            if digest.hexdigest() != entry['sha256']:
                raise StorageError('restore hash mismatch')
            os.link(pending, target)
        finally:
            os.unlink(pending)
    return manifest


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    inv = commands.add_parser('inventory')
    inv.add_argument('root', type=Path)
    arc = commands.add_parser('archive')
    arc.add_argument('root', type=Path)
    arc.add_argument('archive_dir', type=Path)
    arc.add_argument('paths', nargs='+')
    arc.add_argument('--keep', action='append', default=[])
    arc.add_argument('--remove-originals', action='store_true')
    ver = commands.add_parser('verify')
    ver.add_argument('archive_dir', type=Path)
    res = commands.add_parser('restore')
    res.add_argument('archive_dir', type=Path)
    res.add_argument('destination', type=Path)
    res.add_argument('--path', dest='paths', action='append')
    args = vars(parser.parse_args(argv))
    command = args.pop('command')
    try:
        result = globals()[command](**args)
    except (StorageError, OSError, KeyError, TypeError, json.JSONDecodeError, zlib.error) as exc:
        parser.exit(1, f'storage: {exc}\n')
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == '__main__':
    main()

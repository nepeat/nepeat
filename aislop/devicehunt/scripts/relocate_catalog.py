"""Relocate current source-cache paths after copying this entire workspace.

Run from the destination with PYTHONPATH=. Only sources.cache_path is changed;
hashes, claims and historical rebuilt databases are left untouched. Every cached
source is verified against its recorded hash before committing any update.
"""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def relocate(old, new, audit):
    old, new = Path(old).resolve(), Path(new).resolve()
    if old == new:
        raise ValueError('Source and destination must differ')
    path = new / 'data/devices.sqlite'
    if Path(audit).is_absolute():
        raise ValueError('Audit path must be workspace-relative')
    audit = (new / audit).resolve()
    if new not in audit.parents or not audit.parent.is_dir():
        raise ValueError('Audit path must name a new file in an existing workspace directory')
    if audit.exists():
        raise ValueError('Audit file already exists; earlier relocation records are kept')
    before = sha(path)
    db = sqlite3.connect(path.as_uri() + '?mode=rw', uri=True)
    try:
        updates = []
        for sid, value, expected in db.execute('SELECT id,cache_path,sha256 FROM sources'):
            if not value:
                continue
            cached = Path(value)
            if cached.is_absolute():
                if old in cached.parents:
                    relative = cached.relative_to(old)
                elif new in cached.parents:
                    relative = cached.relative_to(new)
                else:
                    raise ValueError('Unmapped cache path for ' + sid)
            else:
                relative = cached
            target = (new / relative).resolve()
            if new not in target.parents or not expected or sha(target) != expected:
                raise ValueError('Missing or changed copied source: ' + sid)
            updates.append((str(target), sid))
        with db:
            db.executemany('UPDATE sources SET cache_path=? WHERE id=?', updates)
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not db.execute('PRAGMA foreign_key_check').fetchall()
    finally:
        db.close()
    result = dict(source_root=str(old), destination_root=str(new),
                  database_before_sha256=before, database_after_sha256=sha(path),
                  verified_cache_paths=len(updates), modified_columns=['sources.cache_path'],
                  historical_databases_modified=False)
    with audit.open('x') as stream:
        stream.write(json.dumps(result, indent=2) + '\n')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--from-root', required=True)
    parser.add_argument('--to-root', default='.')
    parser.add_argument('--audit', required=True,
                        help='new workspace-relative audit path, e.g. data/exports/relocation-audit-DATE.json')
    args = parser.parse_args()
    print(json.dumps(relocate(args.from_root, args.to_root, args.audit), indent=2))

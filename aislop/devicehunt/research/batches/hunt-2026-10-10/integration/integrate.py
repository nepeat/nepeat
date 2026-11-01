"""Integrator-only staging for the 2026-10-10 hunt batch. Never promotes by itself.

stage   copy the live catalog to a NEW candidate path, import the accepted curated inputs
        there, audit that every live row survives unchanged, and write a candidate import
        manifest (new jobs appended, every snapshot hashed, expected counts from the candidate)

Promotion, the clean rebuild and its comparison are separate, explicit commands.
"""
import argparse
import hashlib
import json
import sqlite3
import sys
from contextlib import closing
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))

from devicehunt.catalog import Catalog  # noqa: E402
from devicehunt.pipeline import import_source  # noqa: E402

DATA = ROOT / 'data'
OUT = Path(__file__).resolve().parent
# Tables whose rows must all survive unchanged (FTS shadow tables are rebuilt, compared by count).
SKIP_PREFIXES = ('search_index', 'document_search', 'sqlite_')


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def inputs_for(curated):
    """The curated JSON plus every snapshot it names, as data-relative paths."""
    path = (DATA / curated).resolve()
    data = json.loads(path.read_text())
    files = {curated: sha(path)}
    for source in data['sources']:
        if source.get('snapshot'):
            snapshot = (path.parent / source['snapshot']).resolve()
            if DATA not in snapshot.parents or not snapshot.is_file():
                raise ValueError('snapshot outside data/ or missing: ' + source['snapshot'])
            files[snapshot.relative_to(DATA).as_posix()] = sha(snapshot)
    return files


def stage(args):
    candidate = DATA / args.candidate
    if candidate.exists():
        raise SystemExit('candidate path exists; choose a new one')
    live_sha = sha(DATA / 'devices.sqlite')
    with closing(sqlite3.connect((DATA / 'devices.sqlite').as_uri() + '?mode=ro', uri=True)) as src, \
            closing(sqlite3.connect(candidate)) as dst:
        src.backup(dst)
    c = Catalog(candidate)
    results = {}
    for curated in args.curated:
        results[curated] = import_source(c, 'curated', DATA / curated)
    c.db.commit()
    c.db.close()
    audit = {}
    with closing(sqlite3.connect(candidate.as_uri(), uri=True)) as db:
        db.execute('ATTACH DATABASE ? AS live', ((DATA / 'devices.sqlite').as_uri() + '?mode=ro',))
        tables = [r[0] for r in db.execute("SELECT name FROM live.sqlite_master WHERE type='table'")]
        for table in tables:
            if table.startswith(SKIP_PREFIXES):
                continue
            missing = db.execute('SELECT COUNT(*) FROM (SELECT * FROM live."%s" EXCEPT SELECT * FROM main."%s")'
                                 % (table, table)).fetchone()[0]
            live_n = db.execute('SELECT COUNT(*) FROM live."%s"' % table).fetchone()[0]
            cand_n = db.execute('SELECT COUNT(*) FROM main."%s"' % table).fetchone()[0]
            audit[table] = dict(live=live_n, candidate=cand_n, live_rows_missing=missing)
        integrity = db.execute('PRAGMA integrity_check').fetchone()[0]
        fk = db.execute('PRAGMA foreign_key_check').fetchall()
        counts = dict(devices=db.execute('SELECT COUNT(*) FROM devices').fetchone()[0],
                      records=db.execute('SELECT COUNT(*) FROM records').fetchone()[0],
                      sources=dict(db.execute('SELECT id,records_seen FROM sources')))
    lost = {t: a for t, a in audit.items() if a['live_rows_missing']}
    manifest = json.loads((DATA / 'import-manifest.json').read_text())
    jobs = {(j['source'], j['path']) for j in manifest['imports']}
    for curated in args.curated:
        if ('curated', curated) in jobs:
            raise SystemExit('job already in the accepted manifest: ' + curated)
        manifest['imports'].append(dict(source='curated', path=curated))
        for name, value in inputs_for(curated).items():
            if manifest['files'].get(name, value) != value:
                raise SystemExit('conflicting hash for existing manifest input: ' + name)
            manifest['files'][name] = value
    manifest['expected'].update(devices=counts['devices'], records=counts['records'], sources=counts['sources'])
    out = DATA / args.manifest
    if out.exists():
        raise SystemExit('candidate manifest path exists')
    out.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    report = dict(live_sha256=live_sha, candidate=str(candidate.relative_to(ROOT)), candidate_manifest=str(out.relative_to(ROOT)),
                  imported=results, counts=dict(devices=counts['devices'], records=counts['records'],
                                                sources=len(counts['sources'])),
                  integrity=integrity, foreign_key_violations=len(fk), tables_with_lost_live_rows=lost,
                  growth={t: a['candidate'] - a['live'] for t, a in audit.items() if a['candidate'] != a['live']})
    path = OUT / ('stage-' + Path(args.candidate).stem + '.json')
    with path.open('x') as stream:
        json.dump(report, stream, indent=2, sort_keys=True)
        stream.write('\n')
    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if lost or integrity != 'ok' or fk else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    s = commands.add_parser('stage')
    s.add_argument('--candidate', required=True, help='new data-relative candidate catalog path')
    s.add_argument('--manifest', required=True, help='new data-relative candidate manifest path')
    s.add_argument('curated', nargs='+', help='accepted data-relative curated JSON inputs')
    args = parser.parse_args()
    raise SystemExit(stage(args))


if __name__ == '__main__':
    main()

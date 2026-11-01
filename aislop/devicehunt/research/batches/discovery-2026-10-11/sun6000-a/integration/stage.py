"""Coordinator staging for SUN-6000 option A (Kladno-only). Never promotes by itself.

Builds data/import-manifest-candidate-sun6000-2026-10-11.json from the live manifest (Celestica RRA job moved to the
sibling bundle; every release input hashed; campaign_gate -> the new release + coverage manifest) and
data/devices-candidate-sun6000-2026-10-11.sqlite (live catalog copy + re-import of the Celestica RRA job), then audits
that every live row survives and reports counts.
"""
import hashlib, json, sqlite3, sys
from contextlib import closing
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT))
from devicehunt.catalog import Catalog  # noqa: E402
from devicehunt.pipeline import import_source  # noqa: E402
from devicehunt import review_queue  # noqa: E402

DATA = ROOT / 'data'
RELEASE = 'campaign-releases/campaign-release-1d45317e1bfc9aee8767129e4885e48c4393bc300f514a020dc82775294986a6.json'
COVERAGE = 'coverage/rra-ised-saved-2026-10-04/coverage-b8ee5df9984de206d250cf144da8d40ac60535b6caaee2d84fc07506dfdcbacd.json'
OLD_JOB, NEW_JOB = 'raw/rra-celestica-official/bundle.json', 'raw/rra-celestica-official/bundle-composite-cells.json'
CAND_DB, CAND_MANIFEST = DATA / 'devices-candidate-sun6000-2026-10-11.sqlite', DATA / 'import-manifest-candidate-sun6000-2026-10-11.json'
OUT = Path(__file__).resolve().parent


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def main():
    for p in (CAND_DB, CAND_MANIFEST):
        if p.exists():
            raise SystemExit('exists: ' + str(p))
    manifest = json.loads((DATA / 'import-manifest.json').read_text())
    jobs = [j for j in manifest['imports'] if j['source'] == 'rra' and j['path'] == OLD_JOB]
    assert len(jobs) == 1, jobs
    jobs[0]['path'] = NEW_JOB
    release = json.loads((DATA / RELEASE).read_text())
    for e in release['entries']:
        for i in e['inputs'] or []:
            have = manifest['files'].get(i['path'])
            if have is not None and have != i['sha256']:
                raise SystemExit('hash conflict for existing input ' + i['path'])
            assert sha(DATA / i['path']) == i['sha256'], i['path']
            manifest['files'][i['path']] = i['sha256']
    for rel in (RELEASE, COVERAGE):
        manifest['files'][rel] = sha(DATA / rel)
    manifest['campaign_gate'] = {'release': RELEASE, 'coverage_manifest': COVERAGE}
    review_queue.gate_inputs(DATA, manifest)
    with closing(sqlite3.connect((DATA / 'devices.sqlite').as_uri() + '?mode=ro', uri=True)) as src, \
            closing(sqlite3.connect(CAND_DB)) as dst:
        src.backup(dst)
    c = Catalog(CAND_DB)
    result = import_source(c, 'rra', DATA / NEW_JOB)
    c.db.commit()
    c.db.close()
    audit = {}
    with closing(sqlite3.connect(CAND_DB.as_uri(), uri=True)) as db:
        db.execute('ATTACH DATABASE ? AS live', ((DATA / 'devices.sqlite').as_uri() + '?mode=ro',))
        for (t,) in db.execute("SELECT name FROM live.sqlite_master WHERE type='table'").fetchall():
            if t.startswith(('search_index', 'document_search', 'sqlite_')):
                continue
            missing = db.execute('SELECT COUNT(*) FROM (SELECT * FROM live."%s" EXCEPT SELECT * FROM main."%s")' % (t, t)).fetchone()[0]
            if missing:
                audit[t] = missing
        counts = dict(devices=db.execute('SELECT COUNT(*) FROM devices').fetchone()[0],
                      records=db.execute('SELECT COUNT(*) FROM records').fetchone()[0],
                      sources=dict(db.execute('SELECT id,records_seen FROM sources')))
        integrity = db.execute('PRAGMA integrity_check').fetchone()[0]
        fk = db.execute('PRAGMA foreign_key_check').fetchall()
        review_queue.gate_projections(db, release)
    manifest['expected'].update(devices=counts['devices'], records=counts['records'], sources=counts['sources'])
    CAND_MANIFEST.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    report = dict(import_result=result, devices=counts['devices'], records=counts['records'], integrity=integrity,
                  foreign_key_violations=len(fk), live_rows_missing=audit, gate_projections='ok',
                  candidate_manifest_sha256=sha(CAND_MANIFEST), candidate_sha256=sha(CAND_DB))
    (OUT / 'stage-report.json').write_text(json.dumps(report, indent=2, sort_keys=True, default=str) + '\n')
    print(json.dumps(report, indent=2, sort_keys=True, default=str))
    return 1 if audit or integrity != 'ok' or fk else 0


if __name__ == '__main__':
    raise SystemExit(main())

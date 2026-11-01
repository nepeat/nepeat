"""Pure preparation of accepted and staged curated jobs; no Catalog or SQL writes."""
from collections import Counter
import datetime as dt
import hashlib
import json
from pathlib import Path
import sys
import time
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(ROOT))
from devicehunt.importers import prepare_curated


def sha(body):
    return hashlib.sha256(body).hexdigest()


def main():
    started = time.monotonic()
    held = {}
    jobs = {}
    for name in ('data/import-manifest.json', 'data/import-manifest-candidate-wave3-2026-10-12.json'):
        body = (ROOT / name).read_bytes()
        manifest = json.loads(body)
        held[name] = (body, manifest)
        for job in manifest['imports']:
            if job['source'] == 'curated':
                path = job['path']
                expected = manifest['files'][path]
                if path in jobs:
                    assert jobs[path]['sha256'] == expected
                else:
                    jobs[path] = {'sha256': expected}
    reads = Counter()
    original = Path.read_bytes

    def counted(path):
        reads[str(path)] += 1
        return original(path)

    with patch.object(Path, 'read_bytes', counted):
        for name, entry in jobs.items():
            path = ROOT / 'data' / name
            assert sha(path.read_bytes()) == entry['sha256'], name
            prepared = prepare_curated(path)
            entry.update(source_occurrences=len(prepared), row_occurrences=sum(len(p[3]) for p in prepared),
                         snapshots=[{'source': source['id'], 'path': metadata.cache_path,
                                     'sha256': metadata.sha256, 'fetched_at': metadata.fetched_at,
                                     'selected_rows': len(rows)}
                                    for source, snapshot, metadata, rows in prepared])
    manifests = {}
    for name, (body, manifest) in held.items():
        paths = [j['path'] for j in manifest['imports'] if j['source'] == 'curated']
        manifests[name] = dict(sha256=sha(body), unchanged_after_read=(ROOT / name).read_bytes() == body,
                               curated_jobs=len(paths), source_occurrences=sum(jobs[p]['source_occurrences'] for p in paths),
                               row_occurrences=sum(jobs[p]['row_occurrences'] for p in paths))
        assert manifests[name]['unchanged_after_read'], name
    report = dict(at_utc=dt.datetime.now(dt.timezone.utc).isoformat(), seconds=time.monotonic() - started,
                  scope='pure preparation only; union of held accepted and wave3 staged curated inputs, no imports/SQL/network',
                  manifests=manifests, unique_jobs=len(jobs), read_bytes_calls=sum(reads.values()),
                  snapshot_read_bytes_calls=sum(reads.values()) - len(jobs), jobs=jobs)
    (HERE / 'input-preflight.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k:v for k,v in report.items() if k != 'jobs'}, indent=2))


if __name__ == '__main__':
    main()

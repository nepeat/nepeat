import hashlib
import importlib.util
import json
import sqlite3
import statistics
import time
from pathlib import Path

from devicehunt import lookup

spec = importlib.util.spec_from_file_location('devicehunt.lookup_before_window', '/private/tmp/devicehunt_lookup_before_window.py')
before = importlib.util.module_from_spec(spec)
spec.loader.exec_module(before)
path = Path('data/devices.sqlite').resolve()
digest = lambda: hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
initial = digest()
cases = [(query, 20) for query in ['TIMBERWLFL10', 'Oracle', 'Inc']]
cases += [(query, limit) for query in ['AZ4444-52X-DC-10', '7760-32X-A-AC-F', 'STRASSE_100%', "' OR 1=1 --"] for limit in [0, 1, 20, -1]]
report = {'database_sha256': initial, 'sqlite_version': sqlite3.sqlite_version,
          'method': 'Read-only complete output equality, alternating order; three rounds for broad/default-limit cases, one for remaining limits. Warm-process wall time, not a guarantee.',
          'cases': []}
with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as db:
    db.row_factory = sqlite3.Row
    for index, (query, limit) in enumerate(cases):
        times = {'before': [], 'after': []}
        for repeat in range(3 if index < 3 else 1):
            results = {}
            names = ['before', 'after'] if (index + repeat) % 2 == 0 else ['after', 'before']
            for name in names:
                start = time.perf_counter()
                results[name] = (before if name == 'before' else lookup).run(db, query, limit)
                times[name].append(time.perf_counter() - start)
            assert results['before'] == results['after'], (query, limit)
        item = {'query': query, 'limit': limit, 'full_outputs_equal': True,
                'datasets': len(results['after']['datasets']),
                'dataset_matches': {s['dataset']: s['matches'] for s in results['after']['datasets']},
                'seconds': times, 'median_seconds': {k: statistics.median(v) for k, v in times.items()}}
        report['cases'].append(item)
        print(json.dumps({k: item[k] for k in ['query', 'limit', 'full_outputs_equal', 'median_seconds']}), flush=True)
report['database_sha256_after'] = digest()
assert report['database_sha256_after'] == initial
report['database_unchanged'] = True
Path('data/exports/lookup-window-audit.json').write_text(json.dumps(report, indent=2) + '\n')

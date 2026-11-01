"""Read-only profile of every curated input in one accepted manifest snapshot."""
from collections import Counter, defaultdict
import datetime
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def main():
    manifest_bytes = (ROOT / 'data/import-manifest.json').read_bytes()
    manifest = json.loads(manifest_bytes)
    fields = {'source': defaultdict(Counter), 'device': defaultdict(Counter)}
    tuple_fields = {name: {'containers': Counter(), 'lengths': Counter(), 'value_types': defaultdict(Counter),
                           'null_or_false_values': Counter(), 'repeated_pairs': 0}
                    for name in ('affiliations', 'identifiers', 'facts', 'hyperscaler')}
    files = []
    for job in manifest['imports']:
        if job['source'] != 'curated':
            continue
        path = ROOT / 'data' / job['path']
        body = path.read_bytes()
        data = json.loads(body)
        assert hashlib.sha256(body).hexdigest() == manifest['files'][job['path']]
        source_counts = Counter(json.dumps(s.get('id'), sort_keys=True) for s in data['sources'])
        keys = Counter(json.dumps([r.get('source'), r.get('key')], sort_keys=True) for r in data['devices'])
        source_ids = [s['id'] for s in data['sources']]
        orphan_rows = [n for n,r in enumerate(data['devices']) if r.get('source') not in source_ids]
        snapshots = []
        for source in data['sources']:
            for field,value in source.items():
                fields['source'][field][type(value).__name__] += 1
            snapshot = source.get('snapshot')
            if snapshot:
                local = path.parent / snapshot
                snapshots.append({'path': snapshot, 'exists': local.is_file(), 'bytes': local.stat().st_size if local.exists() else None})
        for row in data['devices']:
            for field,value in row.items():
                fields['device'][field][type(value).__name__] += 1
            for name,profile in tuple_fields.items():
                if name not in row:
                    profile['containers']['absent'] += 1
                    continue
                entries = row[name]
                profile['containers'][type(entries).__name__] += 1
                seen = set()
                for entry in entries:
                    profile['lengths'][str(len(entry))] += 1
                    key = json.dumps(entry, sort_keys=True)
                    if key in seen:
                        profile['repeated_pairs'] += 1
                    seen.add(key)
                    for n,value in enumerate(entry):
                        profile['value_types'][str(n)][type(value).__name__] += 1
                        if value is None or not value:
                            profile['null_or_false_values'][str(n) + ':' + type(value).__name__] += 1
        files.append({'path': job['path'], 'sha256': hashlib.sha256(body).hexdigest(), 'bytes': len(body),
                      'sources': len(data['sources']), 'devices': len(data['devices']),
                      'duplicate_source_ids': {k:v for k,v in source_counts.items() if v > 1},
                      'duplicate_source_keys': {k:v for k,v in keys.items() if v > 1},
                      'unreferenced_device_ordinals': orphan_rows,
                      'top_level_fields': sorted(data), 'snapshots': snapshots})
    report = {'at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'manifest_sha256': hashlib.sha256(manifest_bytes).hexdigest(),
              'manifest_changed_after_read': (ROOT/'data/import-manifest.json').read_bytes() != manifest_bytes,
              'curated_jobs': len(files), 'source_occurrences': sum(f['sources'] for f in files),
              'device_occurrences': sum(f['devices'] for f in files), 'files': files,
              'field_types': fields, 'tuple_fields': tuple_fields,
              'scope': 'all curated jobs in held accepted manifest bytes; read-only profile, no imports'}
    (HERE/'accepted-input-profile.json').write_text(json.dumps(report, indent=2) + '\n')
    print('curated jobs/source occurrences/device occurrences:', report['curated_jobs'], report['source_occurrences'], report['device_occurrences'])
    print('duplicates/orphans:', sum(bool(f['duplicate_source_ids']) for f in files), sum(bool(f['duplicate_source_keys']) for f in files), sum(bool(f['unreferenced_device_ordinals']) for f in files))
    print('tuple field profiles:', {k:{'containers':dict(v['containers']),'lengths':dict(v['lengths']),'types':{n:dict(c) for n,c in v['value_types'].items()},'skip_values':dict(v['null_or_false_values']),'duplicates':v['repeated_pairs']} for k,v in tuple_fields.items()})


if __name__ == '__main__':
    main()

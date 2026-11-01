"""Complete historical FCC grantee register, separate from device identities."""
import csv
import datetime as dt
import json
import re
from pathlib import Path

from .catalog import digest

DATASET = '3b3k-34jp'
SOURCE = 'fcc-grantees'
URL = 'https://opendata.fcc.gov/d/' + DATASET
METADATA_URL = 'https://opendata.fcc.gov/api/views/' + DATASET + '.json'
CSV_URL = 'https://opendata.fcc.gov/api/views/' + DATASET + '/rows.csv?accessType=DOWNLOAD'
COUNT_URL = 'https://opendata.fcc.gov/resource/' + DATASET + '.json?$select=count(*)'
FIELDS = ['grantee_code', 'grantee_name', 'mailing_address', 'po_box', 'city',
          'state', 'country', 'zip_code', 'contact_name', 'date_received']


def count_value(body):
    rows = json.loads(body)
    if not isinstance(rows, list) or len(rows) != 1 or not re.fullmatch(r'[1-9][0-9]*', str(rows[0].get('count', ''))):
        raise ValueError('Invalid FCC grantee row count')
    return int(rows[0]['count'])


def metadata(body):
    data = json.loads(body)
    if (data.get('id') != DATASET or data.get('name') != 'EAS Equipment Authorization Grantee Registrations'
            or [c['fieldName'] for c in data.get('columns', [])] != FIELDS
            or type(data.get('rowsUpdatedAt')) is not int):
        raise ValueError('FCC grantee dataset metadata changed')
    return data


def collect(raw, fetch, refresh=False):
    raw = Path(raw)
    bundle_path = raw / 'fcc-grantees-bundle.json'
    if bundle_path.exists() and not refresh:
        return bundle_path
    files = {}
    for key, url, filename in [
        ('metadata_before', METADATA_URL, 'fcc-grantees-metadata.json'),
        ('count_before', COUNT_URL, 'fcc-grantees-count-before.json'),
        ('csv', CSV_URL, 'fcc-grantees.csv'),
        ('count_after', COUNT_URL, 'fcc-grantees-count.json'),
        ('metadata_after', METADATA_URL, 'fcc-grantees-metadata-after.json'),
    ]:
        path = fetch(url, raw / filename, True, max_bytes=50_000_000)
        files[key] = {'path': filename, 'url': url, 'sha256': digest(path.read_bytes())}
    data = {'schema_version': 1, 'dataset': DATASET, 'files': files}
    # Check both snapshots and the entire CSV before publishing its manifest.
    parse_bundle(raw, data)
    tmp = bundle_path.with_suffix('.part')
    tmp.write_text(json.dumps(data, indent=2) + '\n')
    tmp.replace(bundle_path)
    return bundle_path


def parse_bundle(root, data):
    if data.get('schema_version') != 1 or data.get('dataset') != DATASET:
        raise ValueError('Unrecognized FCC grantee bundle')
    expected_urls = {'metadata_before': METADATA_URL, 'metadata_after': METADATA_URL,
                     'count_before': COUNT_URL, 'count_after': COUNT_URL, 'csv': CSV_URL}
    if set(data['files']) != set(expected_urls):
        raise ValueError('Incomplete FCC grantee bundle')
    bodies = {}
    for key, url in expected_urls.items():
        entry = data['files'][key]
        name = entry['path']
        if not isinstance(name, str) or Path(name).name != name or name in ('.', '..') or entry['url'] != url:
            raise ValueError('Invalid FCC grantee file reference')
        path = (root / name).resolve()
        if path.parent != root.resolve():
            raise ValueError('FCC grantee file escapes bundle directory')
        body = path.read_bytes()
        if digest(body) != entry['sha256']:
            raise ValueError('FCC grantee snapshot checksum mismatch: ' + key)
        bodies[key] = body.decode('utf-8-sig')
    before, after = metadata(bodies['metadata_before']), metadata(bodies['metadata_after'])
    if before['rowsUpdatedAt'] != after['rowsUpdatedAt']:
        raise ValueError('FCC grantee dataset changed during download')
    count = count_value(bodies['count_before'])
    if count != count_value(bodies['count_after']):
        raise ValueError('FCC grantee row count changed during download')
    # Use StringIO rather than splitlines to retain quoted multiline addresses.
    import io
    reader = csv.DictReader(io.StringIO(bodies['csv']))
    if reader.fieldnames != FIELDS:
        raise ValueError('FCC grantee CSV columns changed')
    rows, codes = [], set()
    for row in reader:
        if set(row) != set(FIELDS) or any(v is None for v in row.values()):
            raise ValueError('Malformed FCC grantee CSV row')
        code = row['grantee_code']
        if not re.fullmatch(r'(?:[A-Z0-9]{3}|[A-Z0-9]{5})', code) or code in codes:
            raise ValueError('Invalid or duplicate FCC grantee code: ' + code)
        dt.datetime.strptime(row['date_received'], '%m/%d/%Y %I:%M:%S %p')
        codes.add(code)
        rows.append(row)
    if len(rows) != count:
        raise ValueError('Truncated FCC grantee CSV: row count differs from API')
    return rows, before['rowsUpdatedAt']


def import_bundle(c, path):
    path = Path(path)
    rows, updated = parse_bundle(path.parent, json.loads(path.read_text()))
    date = dt.datetime.fromtimestamp(updated, dt.timezone.utc).date().isoformat()
    c.source(SOURCE, URL, 'regulatory', 'US government factual public records',
             'Full CSV grantee register: row count checked against API before/after download. '
             'Dataset last updated ' + date + '. Historical company registrations, not device authorizations, '
             'current active status, or manufacturing evidence. Original legal names and empty fields retained. '
             'Related subsidiaries and legacy Sun names are not automatically merged.', path, str(updated))
    c.db.execute('DELETE FROM fcc_grantees WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO fcc_grantees VALUES (' + ','.join('?' for _ in range(11)) + ')',
                     [tuple(r[k] for k in FIELDS) + (SOURCE,) for r in rows])
    c.finish(SOURCE, len(rows))
    return {'registrations': len(rows), 'dataset_updated': date,
            'unnamed_registrations': sum(not r['grantee_name'] for r in rows)}


def search(db, query='', limit=100):
    if limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    tokens = query.split()
    conditions, args = [], []
    for token in tokens:
        value = '%' + token.replace('\\', '\\\\').replace('%', '\\%').replace('_', '\\_') + '%'
        conditions.append("(grantee_name LIKE ? ESCAPE '\\' OR grantee_code LIKE ? ESCAPE '\\')")
        args.extend([value, value])
    clause = ' WHERE ' + ' AND '.join(conditions) if conditions else ''
    return [dict(r) for r in db.execute(
        'SELECT grantee_code,grantee_name,country,date_received,source_id FROM fcc_grantees' +
        clause + ' ORDER BY grantee_name,grantee_code LIMIT ?', args + [limit])]

"""IEEE public address assignments: registered organizations, not device models."""
import csv
import io
import json
import re
from html.parser import HTMLParser
from pathlib import Path

from .catalog import digest, now

SOURCE = 'ieee-mac-assignments'
URL = 'https://standards.ieee.org/products-programs/regauth/'
REGISTRIES = {
    'MA-L': (24, 'https://standards-oui.ieee.org/oui/oui.csv'),
    'MA-M': (28, 'https://standards-oui.ieee.org/oui28/mam.csv'),
    'MA-S': (36, 'https://standards-oui.ieee.org/oui36/oui36.csv'),
    'IAB': (36, 'https://standards-oui.ieee.org/iab/iab.csv'),
}
FIELDS = ['Registry', 'Assignment', 'Organization Name', 'Organization Address']
SCOPE = ('Registered address-block organizations only; not a device model, physical factory, '
         'hyperscaler deployment or proof of a unit\'s provenance. Names and addresses are verbatim. '
         'Duplicate assignments and private assignees are retained. The snapshot covers the four '
         'public MA-L, MA-M, MA-S and IAB CSV files, not confidential assignee identities or CID. '
         'Two downloads of each file agree; no independent database total is available.')


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.urls = set()

    def handle_starttag(self, tag, attrs):
        if tag == 'a':
            url = dict(attrs).get('href', '')
            self.urls.add('https://' + url[7:] if url.startswith('http://') else url)


def parse_csv(body, registry):
    bits, url = REGISTRIES[registry]
    reader = csv.reader(io.StringIO(body.decode('utf-8-sig'), newline=''), strict=True)
    try:
        if next(reader, None) != FIELDS:
            raise ValueError('IEEE CSV header changed: ' + registry)
        rows = []
        for number, cells in enumerate(reader, 1):
            if (len(cells) != 4 or cells[0] != registry
                    or not re.fullmatch('[0-9A-F]{' + str(bits // 4) + '}', cells[1])
                    or not cells[2].strip()):
                raise ValueError('Malformed IEEE assignment: ' + registry + ' row ' + str(number))
            rows.append((registry, number, cells[1], bits, cells[2], cells[3], url))
    except csv.Error as exc:
        raise ValueError('Malformed IEEE CSV: ' + registry) from exc
    if not rows:
        raise ValueError('Empty IEEE registry: ' + registry)
    return rows


def parse_bundle(root, data):
    if data.get('schema_version') != 1 or data.get('source_url') != URL:
        raise ValueError('Unrecognized IEEE bundle')
    urls = {'index': URL}
    for registry, (_, url) in REGISTRIES.items():
        urls[registry] = urls[registry + '_check'] = url
    if set(data['files']) != set(urls):
        raise ValueError('Incomplete IEEE file set')
    bodies = {}
    names = set()
    for key, url in urls.items():
        entry = data['files'][key]
        name = entry['path']
        if (not isinstance(name, str) or Path(name).name != name or name in ('.', '..')
                or name in names or entry['url'] != url):
            raise ValueError('Invalid IEEE dependency reference')
        names.add(name)
        path = (root / name).resolve()
        if path.parent != root.resolve():
            raise ValueError('IEEE dependency escapes bundle')
        bodies[key] = path.read_bytes()
        if digest(bodies[key]) != entry['sha256']:
            raise ValueError('IEEE checksum mismatch: ' + key)
    links = Links()
    links.feed(bodies['index'].decode('utf-8'))
    if any(url not in links.urls for _, url in REGISTRIES.values()):
        raise ValueError('IEEE index does not link all expected CSV files')
    rows, counts = [], {}
    for registry in REGISTRIES:
        if bodies[registry] != bodies[registry + '_check']:
            raise ValueError('IEEE registry changed between downloads: ' + registry)
        parsed = parse_csv(bodies[registry], registry)
        rows.extend(parsed)
        counts[registry] = len(parsed)
    if data.get('expected_rows') != counts:
        raise ValueError('IEEE row counts differ from capture')
    return rows, counts


def collect(raw, fetch, refresh=False):
    root = Path(raw) / 'ieee'
    root.mkdir(parents=True, exist_ok=True)
    bundle = root / 'bundle.json'
    if bundle.exists() and not refresh:
        parse_bundle(root, json.loads(bundle.read_text()))
        return bundle
    data = {'schema_version': 1, 'source_url': URL, 'captured_at': now(), 'files': {}, 'expected_rows': {}}
    jobs = [('index', URL, 'index.html')]
    jobs += [(registry, url, registry + '.csv') for registry, (_, url) in REGISTRIES.items()]
    jobs += [(registry + '_check', url, registry + '-check.csv') for registry, (_, url) in REGISTRIES.items()]
    for key, url, filename in jobs:
        path = fetch(url, root / filename, True, max_bytes=20_000_000)
        body = path.read_bytes()
        data['files'][key] = {'path': filename, 'url': url, 'sha256': digest(body)}
        if key in REGISTRIES:
            data['expected_rows'][key] = len(parse_csv(body, key))
    parse_bundle(root, data)
    temp = bundle.with_suffix('.part')
    temp.write_text(json.dumps(data, indent=2) + '\n')
    temp.replace(bundle)
    return bundle


def import_bundle(c, path):
    path = Path(path)
    rows, counts = parse_bundle(path.parent, json.loads(path.read_text()))
    c.source(SOURCE, URL, 'registration_authority', 'IEEE public registry; source rights retained', SCOPE, path)
    c.db.execute('DELETE FROM ieee_assignments WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO ieee_assignments VALUES (?,?,?,?,?,?,?,?)',
                    [(SOURCE, *row) for row in rows])
    c.finish(SOURCE, len(rows))
    return {'rows': len(rows), 'registries': counts}


def normalize_mac(value):
    if not isinstance(value, str):
        raise ValueError('MAC address must be text')
    value = value.strip()
    if not any(re.fullmatch(pattern, value) for pattern in (
        r'[0-9a-fA-F]{12}', r'(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}',
        r'(?:[0-9a-fA-F]{2}-){5}[0-9a-fA-F]{2}', r'(?:[0-9a-fA-F]{4}\.){2}[0-9a-fA-F]{4}',
    )):
        raise ValueError('Use a full 48-bit MAC: 12 hex digits, six colon/dash-separated octets, or xxxx.xxxx.xxxx')
    return re.sub(r'[:.\-]', '', value).upper()


def mac_lookup(db, value):
    from .lookup import source_metadata
    mac = normalize_mac(value)
    first = int(mac[:2], 16)
    classification = ('unspecified' if mac == '0' * 12 else 'broadcast' if mac == 'F' * 12 else
                      'multicast' if first & 1 else 'locally_administered' if first & 2 else 'universal_unicast')
    own_transaction = not db.in_transaction
    if own_transaction:
        db.execute('BEGIN')
    try:
        present = db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='ieee_assignments'").fetchone()
        sources = source_metadata(db, [SOURCE]) if present else []
        result = {'mac': ':'.join(mac[n:n+2] for n in range(0, 12, 2)),
                  'classification': classification, 'locally_administered': bool(first & 2),
                  'group_address': bool(first & 1), 'state': 'loaded' if sources else 'not_loaded',
                  'status': 'not_loaded', 'prefix_bits': None, 'matches': [], 'sources': sources,
                  'scope': SCOPE + ' A MAC can be reassigned or spoofed. Longest-prefix matches are leads only.'}
        if classification != 'universal_unicast':
            result['status'] = 'not_attributable'
            result['reason'] = ('No hardware assignee inference for local, group, broadcast or unspecified addresses. '
                                'Historical local-bit BlockID entries remain searchable through lookup or SQL.')
        elif sources:
            matches = [dict(r) for r in db.execute(
                'SELECT * FROM ieee_assignments WHERE (prefix_bits=36 AND assignment=?) '
                'OR (prefix_bits=28 AND assignment=?) OR (prefix_bits=24 AND assignment=?) '
                'ORDER BY prefix_bits DESC,registry,source_id,row_number', (mac[:9], mac[:7], mac[:6]))]
            if matches:
                result['prefix_bits'] = matches[0]['prefix_bits']
                result['matches'] = [r for r in matches if r['prefix_bits'] == result['prefix_bits']]
                result['status'] = 'matched'
            else:
                result['status'] = 'no_public_match'
        return result
    finally:
        if own_transaction:
            db.rollback()

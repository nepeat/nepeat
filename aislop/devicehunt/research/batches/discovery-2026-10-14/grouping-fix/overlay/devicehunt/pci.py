"""Community PCI ID names, kept separate from chassis and manufacturer evidence."""
import collections
import json
import re
from pathlib import Path

from .documents import dependency

SOURCE = 'pci-id-repository'
URL = 'https://github.com/pciutils/pciids'
SCOPE = ('Names in a commit-pinned community PCI ID snapshot, not a complete list of hardware, '
         'official PCI-SIG allocation register, chassis identity, factory, operator or unit provenance. '
         'Vendor/device IDs identify a PCI function; subsystem IDs have their own vendor namespace. '
         'Names, including uncertainty and wrong-ID annotations, are preserved. '
         'Amazon.com IDs are not relabeled as Amazon Data Services.')
IDS = ['vendor_id', 'device_id', 'subvendor_id', 'subdevice_id', 'class_id', 'subclass_id', 'interface_id']


def parse(body):
    text = body.decode('utf-8')
    rows, seen, comments, ignored = [], set(), [], []
    mode, vendor, device, cls, subcls = None, '', '', '', ''

    def add(kind, key, name, number, **ids):
        if (kind, key) in seen:
            raise ValueError('Duplicate PCI identity at line ' + str(number))
        seen.add((kind, key))
        rows.append(dict(line_number=number, entry_kind=kind, id_path=key, name=name,
                         comments='\n'.join(comments), **{k: ids.get(k, '') for k in IDS}))

    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        if line.startswith('#'):
            comments.append(line)
            continue
        if m := re.fullmatch(r'([0-9A-Fa-f]{4}) +([^\s].*)', line):
            mode, vendor, device = 'vendor', m[1].lower(), ''
            add('vendor', vendor, m[2], number, vendor_id=vendor)
        elif m := re.fullmatch(r'C ([0-9A-Fa-f]{2}) +([^\s].*)', line):
            mode, cls, subcls = 'class', m[1].lower(), ''
            add('class', 'C:' + cls, m[2], number, class_id=cls)
        elif m := re.fullmatch(r'S ([0-9A-Fa-f]{4})', line):
            mode, vendor, device = 'generic', m[1].lower(), ''
        elif re.match(r'[A-Za-z] ', line):
            # Official forward-compatibility rule: ignore unknown top-level
            # sections and their indented children, while reporting all lines.
            mode = 'unknown'
            ignored.append(number)
        elif mode == 'unknown' and line.startswith('\t'):
            ignored.append(number)
        elif mode == 'vendor' and (m := re.fullmatch(r'\t([0-9A-Fa-f]{4}) +([^\s].*)', line)):
            device = m[1].lower()
            add('device', vendor + ':' + device, m[2], number, vendor_id=vendor, device_id=device)
        elif mode == 'vendor' and device and (m := re.fullmatch(r'\t\t([0-9A-Fa-f]{4}) ([0-9A-Fa-f]{4}) +([^\s].*)', line)):
            sv, sd = m[1].lower(), m[2].lower()
            add('subsystem', vendor + ':' + device + '/' + sv + ':' + sd, m[3], number,
                vendor_id=vendor, device_id=device, subvendor_id=sv, subdevice_id=sd)
        elif mode == 'generic' and (m := re.fullmatch(r'\t([0-9A-Fa-f]{4}) +([^\s].*)', line)):
            sd = m[1].lower()
            add('generic_subsystem', 'S:' + vendor + ':' + sd, m[2], number, subvendor_id=vendor, subdevice_id=sd)
        elif mode == 'class' and (m := re.fullmatch(r'\t([0-9A-Fa-f]{2}) +([^\s].*)', line)):
            subcls = m[1].lower()
            add('subclass', 'C:' + cls + ':' + subcls, m[2], number, class_id=cls, subclass_id=subcls)
        elif mode == 'class' and subcls and (m := re.fullmatch(r'\t\t([0-9A-Fa-f]{2}) +([^\s].*)', line)):
            pi = m[1].lower()
            add('prog_if', 'C:' + cls + ':' + subcls + ':' + pi, m[2], number,
                class_id=cls, subclass_id=subcls, interface_id=pi)
        else:
            raise ValueError('Malformed or orphan PCI entry at line ' + str(number))
        comments = []
    vendors = {r['vendor_id']: r['name'] for r in rows if r['entry_kind'] == 'vendor'}
    if not vendors or not any(r['entry_kind'] == 'device' for r in rows):
        raise ValueError('PCI snapshot lacks vendors or devices')
    for row in rows:
        # Published snapshots include unresolved subvendor IDs (including 0000).
        # Preserve those rows with an empty resolved name, never invent one.
        row['vendor_name'] = vendors.get(row['vendor_id'], '')
        row['subvendor_name'] = vendors.get(row['subvendor_id'], '')
    return rows, dict(sorted(collections.Counter(r['entry_kind'] for r in rows).items())), ignored


def prepare(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or not re.fullmatch(r'[0-9a-f]{40}', data.get('commit', ''))):
        raise ValueError('Unrecognized PCI snapshot bundle')
    commit = json.loads(dependency(path.parent, data['commit_metadata']))
    if commit.get('sha') != data['commit']:
        raise ValueError('PCI commit metadata mismatch')
    readme = dependency(path.parent, data['readme']).decode('utf-8')
    if '3-clause BSD License' not in readme or 'pci.ids' not in readme:
        raise ValueError('PCI license documentation missing')
    body = dependency(path.parent, data['ids'])
    version = re.search(rb'^#\s*Version:\s*(\d{4}\.\d{2}\.\d{2})\s*$', body, re.M)
    if not version or version[1].decode() != data.get('version'):
        raise ValueError('PCI version header mismatch')
    expected_url = 'https://raw.githubusercontent.com/pciutils/pciids/' + data['commit'] + '/pci.ids'
    if data.get('ids_url') != expected_url:
        raise ValueError('PCI snapshot URL is not pinned to the reviewed commit')
    rows, counts, ignored = parse(body)
    if counts != data.get('expected_counts') or ignored != data.get('ignored_extension_lines'):
        raise ValueError('PCI parsed counts or extension scope changed')
    missing = sorted({r['subvendor_id'] for r in rows if r['subvendor_id'] and not r['subvendor_name']})
    if missing != data.get('undefined_subvendor_ids'):
        raise ValueError('PCI unresolved subvendor scope changed')
    return data, rows, counts


def import_bundle(c, path):
    data, rows, counts = prepare(path)
    c.source(SOURCE, data['ids_url'], 'community', 'GPL-2.0-or-later OR BSD-3-Clause; see cached README.md and pci.ids header',
             SCOPE + ' File version ' + data['version'] + '; unknown extension lines: ' +
             str(len(data['ignored_extension_lines'])) + '; undefined subvendor IDs retained: ' +
             ', '.join(data['undefined_subvendor_ids']) + '.', path, revision=data['commit'])
    c.db.execute('DELETE FROM pci_entries WHERE source_id=?', (SOURCE,))
    columns = ['line_number', 'entry_kind', 'id_path', 'name', 'comments', *IDS, 'vendor_name', 'subvendor_name']
    c.db.executemany('INSERT INTO pci_entries (source_id,' + ','.join(columns) + ') VALUES (' +
                    ','.join('?' for _ in range(len(columns) + 1)) + ')',
                    [(SOURCE, *(r[k] for k in columns)) for r in rows])
    c.finish(SOURCE, len(rows))
    return {'rows': len(rows), 'kinds': counts}


def pair(value):
    if not isinstance(value, str) or not re.fullmatch(r'[0-9a-fA-F]{4}:[0-9a-fA-F]{4}', value.strip()):
        raise ValueError('Use a complete PCI ID pair: hhhh:hhhh')
    return value.strip().lower()


def identify(db, value, subsystem=None, class_code=None):
    from .lookup import source_metadata
    key = pair(value)
    sub = pair(subsystem) if subsystem is not None else None
    if class_code is not None and (not isinstance(class_code, str) or not re.fullmatch(r'[0-9A-Fa-f]{4}(?:[0-9A-Fa-f]{2})?', class_code)):
        raise ValueError('PCI class code requires four or six hex digits')
    own = not db.in_transaction
    if own:
        db.execute('BEGIN')
    try:
        present = db.execute("SELECT 1 FROM sqlite_master WHERE name='pci_entries' AND type='table'").fetchone()
        sources = source_metadata(db, [SOURCE]) if present else []
        result = {'id': key, 'subsystem_id': sub, 'class_code': class_code.lower() if class_code else None,
                  'state': 'loaded' if sources else 'not_loaded', 'vendor': [], 'device': [],
                  'subsystem_vendor': [], 'subsystem': [], 'class': [], 'scope': SCOPE, 'sources': sources}
        if not sources:
            return result
        def get(kind, path):
            return [dict(r) for r in db.execute('SELECT * FROM pci_entries WHERE source_id=? AND entry_kind=? AND id_path=?',
                                              (SOURCE, kind, path))]
        result['vendor'] = get('vendor', key[:4])
        result['device'] = get('device', key)
        if sub:
            result['subsystem_vendor'] = get('vendor', sub[:4])
            result['subsystem'] = get('subsystem', key + '/' + sub) or get('generic_subsystem', 'S:' + sub)
        if class_code:
            parts = [class_code[n:n+2].lower() for n in range(0,len(class_code),2)]
            for n, kind in enumerate(['class', 'subclass', 'prog_if'][:len(parts)], 1):
                result['class'] += get(kind, 'C:' + ':'.join(parts[:n]))
        return result
    finally:
        if own:
            db.rollback()

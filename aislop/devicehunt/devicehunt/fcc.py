"""Import reviewed FCC application manifests; no remote crawling or PDF claims."""
import datetime
import json
import re
from pathlib import Path
from urllib.parse import urlparse

from .catalog import target_for


def required(row, key):
    value = row[key]
    if not isinstance(value, str) or not value.strip():
        raise ValueError('Missing FCC field: ' + key)
    return value


def url(value):
    parsed = urlparse(value)
    if parsed.scheme != 'https' or not parsed.hostname or parsed.username:
        raise ValueError('FCC evidence URL must be HTTPS')


def validate(data):
    if data['schema_version'] != 1 or data['complete_for_declared_scope'] is not True:
        raise ValueError('Unsupported or incomplete FCC manifest')
    url(required(data, 'source_url'))
    required(data, 'scope')
    datetime.date.fromisoformat(data['reviewed_at'])
    apps = data['applications']
    if not apps or len(apps) != data['expected_applications']:
        raise ValueError('FCC application count mismatch')
    seen = set()
    for a in apps:
        fid = required(a, 'fcc_id')
        if not re.fullmatch(r'[A-Z0-9][A-Z0-9-]{3,31}', fid):
            raise ValueError('Invalid FCC ID')
        key = (fid, required(a, 'application_id'))
        if key in seen:
            raise ValueError('Duplicate FCC application')
        seen.add(key)
        if target_for(required(a, 'grantee')) is None:
            raise ValueError('FCC grantee is outside the target companies')
        for field in ('model', 'equipment', 'purpose', 'frequency_mhz', 'exhibit_scope'):
            required(a, field)
        if not re.fullmatch(r'\d{10}', a['frn']):
            raise ValueError('Invalid FRN')
        datetime.date.fromisoformat(a['grant_date'])
        url(required(a, 'source_url'))
        if len(a['exhibits']) != a['expected_exhibits']:
            raise ValueError('FCC exhibit count mismatch')
        keys = set()
        for e in a['exhibits']:
            ek = required(e, 'key')
            if ek in keys:
                raise ValueError('Duplicate FCC exhibit')
            keys.add(ek)
            required(e, 'title'); required(e, 'type')
            if e['availability'] not in ('listed_public', 'restricted', 'metadata_only'):
                raise ValueError('Invalid FCC exhibit availability')
            if e.get('document_id') and not re.fullmatch(r'\d+', e['document_id']):
                raise ValueError('Invalid FCC document ID')
            if e.get('url'):
                url(e['url'])
            if e['availability'] == 'listed_public' and not e.get('url'):
                raise ValueError('Public exhibit needs a source link')
            if e.get('available_date'):
                datetime.date.fromisoformat(e['available_date'])
    if sorted({a['fcc_id'] for a in apps}) != sorted(data['fcc_ids']):
        raise ValueError('FCC declared ID scope does not match applications')


def import_manifest(c, path):
    data = json.loads(Path(path).read_text())
    validate(data)  # Reject truncated/manually incomplete input before replacing data.
    sid = 'fcc-manifest'
    c.source(sid, data['source_url'], 'regulatory_mirror',
             'Reviewed factual filing metadata; linked exhibits retain their own terms',
             data['scope'] + ' Reviewed metadata only; no locally archived PDF or complete FCC database claim.')
    for a in data['applications']:
        key = a['fcc_id'] + '|' + a['application_id']
        brand = target_for(a['grantee'])
        c.add(sid, key, a['source_url'], brand, a['model'], brand + ' ' + a['model'],
              a['equipment'], a, [(a['grantee'], 'fcc_grantee')],
              [('fcc_id', a['fcc_id']), ('fcc_application_id', a['application_id'])],
              [('grant_date', a['grant_date']), ('frequency_mhz', a['frequency_mhz']),
               ('filing_purpose', a['purpose'])])
        rid = c.db.execute('SELECT id FROM records WHERE source_id=? AND source_key=?', (sid, key)).fetchone()[0]
        c.db.execute('INSERT INTO fcc_applications VALUES (?,?,?,?,?,?,?,?,?,?)',
                     (rid, a['fcc_id'], a['application_id'], a['grantee'], a['frn'],
                      a['purpose'], a['grant_date'], a['frequency_mhz'], data['reviewed_at'], a['exhibit_scope']))
        for e in a['exhibits']:
            c.db.execute('INSERT INTO fcc_exhibits VALUES (?,?,?,?,?,?,?,?)',
                         (rid, e['key'], e.get('document_id'), e['title'], e['type'],
                          e['availability'], e.get('available_date'), e.get('url')))
    c.finish(sid, len(data['applications']))
    return {'applications': len(data['applications']), 'exhibits': sum(len(a['exhibits']) for a in data['applications'])}

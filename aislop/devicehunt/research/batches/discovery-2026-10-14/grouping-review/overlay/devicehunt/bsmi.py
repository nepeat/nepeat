"""Offline import of Taiwan BSMI's published certificate XML snapshot."""
import json
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

from .documents import dependency

SOURCE = 'bsmi-certificates-166852'
PAGE_URL = 'https://data.gov.tw/dataset/166852'
XML_URL = 'https://data.bsmi.gov.tw/opendata/download/313000000G-000130-001.action'
LICENSE_URL = 'https://data.gov.tw/license'
FIELDS = [('vendor_code', '廠商識別號'), ('certificate_number', '證書號碼'),
          ('applicant', '申請人'), ('expiry_raw', '證書期限'),
          ('product_name', '商品名稱'), ('primary_model', '主型式'), ('status_raw', '證書狀態')]
ATTRIBUTION = ('Bureau of Standards, Metrology and Inspection (Taiwan), 2026, '
               '驗證登錄新發證書資料集 (dataset 166852). Open Government Data License v1.0: '
               + LICENSE_URL)


def parse_xml(body, fields=FIELDS):
    text = body.decode('utf-8-sig')
    if '<!DOCTYPE' in text or '<!ENTITY' in text:
        raise ValueError('BSMI XML declarations/entities are unsupported')
    try:
        root = ET.fromstring(text)
    except ET.ParseError as exc:
        raise ValueError('Malformed BSMI XML: ' + str(exc)) from exc
    if root.tag != 'bsmi' or root.attrib or (root.text or '').strip():
        raise ValueError('Unexpected BSMI XML root')
    rows = []
    for element in root:
        if (element.tag != 'row' or element.attrib or (element.text or '').strip()
                or (element.tail or '').strip()
                or [child.tag for child in element] != [label for _, label in fields]):
            raise ValueError('Unexpected BSMI row structure')
        if any(child.attrib or len(child) or (child.tail or '').strip() for child in element):
            raise ValueError('Unexpected BSMI field structure')
        rows.append({key: child.text or '' for (key, _), child in zip(fields, element)})
    if not rows:
        raise ValueError('BSMI XML has no certificate rows')
    return rows


def summary(rows):
    certificates = Counter(r['certificate_number'] for r in rows if r['certificate_number'])
    return {'rows': len(rows), 'unique_nonempty_certificates': len(certificates),
            'duplicate_certificate_rows': sum(n - 1 for n in certificates.values()),
            'blank_certificates': sum(not r['certificate_number'] for r in rows),
            'blank_applicants': sum(not r['applicant'] for r in rows),
            'blank_models': sum(not r['primary_model'] for r in rows),
            'status_values': dict(sorted(Counter(r['status_raw'] for r in rows).items()))}


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('dataset_url') != PAGE_URL
            or data.get('download_url') != XML_URL or data.get('license_url') != LICENSE_URL):
        raise ValueError('Unrecognized BSMI bundle scope')
    meta = json.loads(dependency(path.parent, data['metadata']))
    if (meta.get('dataset_url') != PAGE_URL or meta.get('xml_url') != XML_URL
            or meta.get('capture_format') != 'factual web-tool metadata extract'):
        raise ValueError('BSMI metadata source association changed')
    rows = parse_xml(dependency(path.parent, data['xml']))
    counts = summary(rows)
    if counts != data.get('expected'):
        raise ValueError('BSMI counts differ from reviewed snapshot')
    c.source(SOURCE, PAGE_URL, 'regulatory', ATTRIBUTION,
             'Entire downloaded XML snapshot. Metadata describes current-year new certificates, '
             'but the file includes historical expiry dates and multiple status values; no '
             'historical completeness or issue year inferred. Applicant is not manufacturer. '
             'Primary model annotations, blanks, raw expiry dates and statuses remain intact. '
             'No inferred aliases, catalog identities, operator or present approval validity. '
             + ATTRIBUTION, path)
    c.db.execute('DELETE FROM bsmi_certificates WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO bsmi_certificates VALUES (?,?,?,?,?,?,?,?,?)',
                    [(SOURCE, n, *[r[key] for key, _ in FIELDS]) for n, r in enumerate(rows, 1)])
    c.finish(SOURCE, len(rows))
    return counts

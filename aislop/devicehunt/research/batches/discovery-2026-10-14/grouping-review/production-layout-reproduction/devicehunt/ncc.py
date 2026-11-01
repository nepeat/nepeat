"""Complete published Taiwan NCC CSV; company roles and raw values stay separate."""
import csv
import io
import json
from collections import Counter
from pathlib import Path

from .documents import dependency

SOURCE = 'ncc-equipment-5996'
PAGE_URL = 'https://data.gov.tw/dataset/5996'
CSV_URL = 'https://api.ncc.gov.tw/uploaddowndoc?file=datagov%2Fvv_IPCCDB.csv&filedisplay=vv_IPCCDB.csv&flag=doc'
LICENSE_URL = 'https://data.gov.tw/license'
FIELDS = [
    ('equipment_type', '設備種類'), ('year_suffix_raw', '西元年度後二位'),
    ('certificate_number', '審定證明編號'), ('trade_certificate_number', '填報貿易e網的認證號碼'),
    ('equipment_name', '設備名稱'), ('brand', '廠牌'), ('model', '型號'),
    ('applicant', '申請廠商'), ('manufacturer', '製造廠商'), ('certification_body', '驗證機構'),
    ('sar_raw', 'SAR值'), ('camera_raw', '拍照功能'), ('camera_sound_raw', '拍照時發出聲響功能'),
    ('charger', '充電器'), ('battery', '電池'), ('approval_date_raw', '審驗日期'),
    ('renewal_date_raw', '換證日期'), ('test_lab', '出具檢驗報告之測試實驗室'),
    ('test_lab_country', '測試實驗室所屬國家'), ('certificate_validity_raw', '驗證明是否仍屬有效'),
    ('revocation_date_raw', '廢止撤銷或註銷日期'),
]
ATTRIBUTION = ('National Communications Commission (Taiwan), 2026, 電信設備審驗合格清單 '
               '(dataset 5996, vv_IPCCDB.csv). Published under Open Government Data License v1.0; '
               'reuse is subject to its conditions: ' + LICENSE_URL)


def parse_csv(body):
    reader = csv.reader(io.StringIO(body.decode('utf-8-sig')), strict=True)
    if next(reader, None) != [label for _, label in FIELDS]:
        raise ValueError('NCC CSV headers differ from the reviewed export')
    rows = []
    try:
        for cells in reader:
            if len(cells) != len(FIELDS) or any('\x00' in value for value in cells):
                raise ValueError('Malformed NCC CSV row ' + str(len(rows) + 1))
            rows.append(dict(zip((key for key, _ in FIELDS), cells)))
    except csv.Error as exc:
        raise ValueError('Malformed NCC CSV: ' + str(exc)) from exc
    if not rows:
        raise ValueError('NCC CSV has no equipment rows')
    return rows


def summary(rows):
    certificates = Counter(r['certificate_number'] for r in rows if r['certificate_number'])
    return {'rows': len(rows), 'unique_nonempty_certificates': len(certificates),
            'duplicate_certificate_rows': sum(n - 1 for n in certificates.values()),
            'blank_certificates': sum(not r['certificate_number'] for r in rows),
            'blank_models': sum(not r['model'] for r in rows),
            'blank_manufacturers': sum(not r['manufacturer'] for r in rows),
            'confidential_identity_rows': sum(any('申請者申請保密' in r[k] for k in ('equipment_name', 'brand', 'model')) for r in rows),
            'validity_values': dict(sorted(Counter(r['certificate_validity_raw'] for r in rows).items()))}


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('dataset_url') != PAGE_URL
            or data.get('download_url') != CSV_URL or data.get('license_url') != LICENSE_URL):
        raise ValueError('Unrecognized NCC bundle scope')
    metadata = json.loads(dependency(path.parent, data['metadata']))
    if (metadata.get('dataset_url') != PAGE_URL or metadata.get('csv_url') != CSV_URL
            or metadata.get('capture_format') != 'factual web-tool metadata extract'):
        raise ValueError('NCC metadata source association changed')
    rows = parse_csv(dependency(path.parent, data['csv']))
    counts = summary(rows)
    if counts != data.get('expected'):
        raise ValueError('NCC CSV counts differ from reviewed snapshot')
    c.source(SOURCE, PAGE_URL, 'regulatory', ATTRIBUTION,
             'All rows of the cached NCC vv_IPCCDB CSV, not a claim of complete historical NCC coverage. '
             'Applicant, manufacturer and brand remain separate verbatim source fields; multi-company '
             'cells, confidentiality labels, duplicates and blanks remain intact. Dates, year suffixes '
             'and validity flags are raw snapshot claims, not current approval determinations. '
             'No catalog identities, company aliases, factory verification or operator attribution. '
             + ATTRIBUTION, path)
    c.db.execute('DELETE FROM ncc_equipment WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO ncc_equipment VALUES (' + ','.join('?' for _ in range(len(FIELDS) + 2)) + ')',
                    [(SOURCE, number, *[r[key] for key, _ in FIELDS]) for number, r in enumerate(rows, 1)])
    c.finish(SOURCE, len(rows))
    return counts

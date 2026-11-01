"""Three distinct BSMI raw registers; no inferred manufacturer associations."""
import json
from collections import Counter
from pathlib import Path

from .bsmi import LICENSE_URL, parse_xml
from .documents import dependency

DATASETS = {
    '162936': dict(table='bsmi_product_certificates', title='商品驗證機構資料集', code='000101',
        fields=[('certificate_number', '證書號碼'), ('applicant', '申請人'),
                ('expiry_raw', '證書期限'), ('contact_name', '聯絡人'), ('contact_phone', '聯絡電話'),
                ('product_name', '商品名稱'), ('primary_model', '主型式'), ('status_raw', '證書狀態')]),
    '30176': dict(table='bsmi_voluntary_certificates', title='自願性驗證證書資料集', code='000086',
        fields=[('certificate_number', '證書號碼'), ('applicant', '申請人'),
                ('expiry_raw', '證書期限'), ('product_name', '產品名稱'), ('primary_model', '主型式')]),
    '169189': dict(table='bsmi_declarations', title='符合性聲明產品登記資料集', code='000162',
        fields=[('registration_number', '登記證號'), ('company_name', '公司名稱'),
                ('business_number', '統一編號'), ('product_name', '商品中文名稱'),
                ('company_address', '公司登記地址'), ('contact_phone', '聯絡電話'),
                ('primary_model', '主要型式'), ('application_date_raw', '線上申請日期')]),
}


def urls(dataset):
    spec = DATASETS[dataset]
    return ('https://data.gov.tw/dataset/' + dataset,
            'https://data.bsmi.gov.tw/opendata/download/313000000G-' + spec['code'] + '-001.action')


def summary(rows, dataset):
    key = 'registration_number' if dataset == '169189' else 'certificate_number'
    company = 'company_name' if dataset == '169189' else 'applicant'
    numbers = Counter(r[key] for r in rows if r[key])
    return dict(rows=len(rows), unique_nonempty_numbers=len(numbers),
                duplicate_number_rows=sum(n - 1 for n in numbers.values()),
                blank_numbers=sum(not r[key] for r in rows),
                blank_companies=sum(not r[company] for r in rows),
                blank_models=sum(not r['primary_model'] for r in rows),
                status_values=dict(sorted(Counter(r['status_raw'] for r in rows).items()))
                    if dataset == '162936' else None)


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    dataset = data.get('dataset_id')
    if not isinstance(dataset, str) or dataset not in DATASETS:
        raise ValueError('Unknown additional BSMI dataset')
    spec = DATASETS[dataset]; page_url, xml_url = urls(dataset)
    if (data.get('schema_version') != 1 or data.get('dataset_url') != page_url
            or data.get('download_url') != xml_url or data.get('license_url') != LICENSE_URL):
        raise ValueError('Additional BSMI bundle scope changed')
    meta = json.loads(dependency(path.parent, data['metadata']))
    if (meta.get('dataset_url') != page_url or meta.get('xml_url') != xml_url
            or meta.get('title') != spec['title'] or meta.get('license_url') != LICENSE_URL
            or meta.get('fields') != [label for _, label in spec['fields']]
            or meta.get('capture_format') != 'factual web-tool metadata extract'):
        raise ValueError('Additional BSMI metadata association changed')
    rows = parse_xml(dependency(path.parent, data['xml']), spec['fields'])
    counts = summary(rows, dataset)
    if counts != data.get('expected'):
        raise ValueError('Additional BSMI counts differ from reviewed snapshot')
    # Validate both dependencies, full XML schema and counts before replacing any rows.
    sid = 'bsmi-additional-' + dataset
    attribution = ('Bureau of Standards, Metrology and Inspection (Taiwan), 2026, '
                   + spec['title'] + ' (dataset ' + dataset + '). Open Government Data License v1.0: ' + LICENSE_URL)
    c.source(sid, page_url, 'regulatory', attribution,
             'Entire downloaded XML snapshot. Separate published register, not an extension or '
             'deduplicated union of other BSMI tables. Applicant or company association only; '
             'no manufacturer, factory, operator, aliases or catalog identities inferred. '
             'All fields, duplicates, empty cells, annotations and raw dates retained. '
             'No current validity or historical completeness claim. ' + attribution, path)
    table = spec['table']; fields = [key for key, _ in spec['fields']]
    c.db.execute('DELETE FROM ' + table + ' WHERE source_id=?', (sid,))
    c.db.executemany('INSERT INTO ' + table + ' VALUES (' + ','.join('?' for _ in range(len(fields)+2)) + ')',
                    [(sid, n, *[row[key] for key in fields]) for n, row in enumerate(rows, 1)])
    c.finish(sid, len(rows))
    return counts

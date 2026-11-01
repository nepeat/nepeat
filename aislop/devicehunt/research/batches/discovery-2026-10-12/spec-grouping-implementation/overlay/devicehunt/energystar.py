"""Complete ENERGY STAR server export, with configuration-specific target facts."""
import csv
import io
import json
import re
from pathlib import Path

from .documents import dependency
from .rra import company_target

DATASET = 'qifb-fcj2'
SOURCE = 'energystar-enterprise-servers'
URL = 'https://data.energystar.gov/d/qifb-fcj2'
METADATA_URL = 'https://data.energystar.gov/api/views/qifb-fcj2'
CSV_URL = 'https://data.energystar.gov/api/v3/views/qifb-fcj2/export.csv?accessType=DOWNLOAD'
DETAIL = 'https://www.energystar.gov/productfinder/product/certified-enterprise-servers/details/'
NAME = 'ENERGY STAR Certified Enterprise Servers'
FIELDS = ['ENERGY STAR Unique ID', 'ENERGY STAR Partner', 'Brand Name', 'Model Name',
          'Model Number', 'ENERGY STAR Model Identifier', 'Date Certified', 'Date Available On Market']
CONFIGS = [('low', 'Low-end Performance Configuration'),
           ('typical', 'Typical or Single Configuration'),
           ('high', 'High-end Performance Configuration')]


def parse_id(value, allow_grouped_ids=False):
    if re.fullmatch(r'[0-9]+', value): return value
    if allow_grouped_ids and re.fullmatch(r'[1-9][0-9]{0,2}(?:,[0-9]{3})+', value):
        return value.replace(',', '')
    raise ValueError('Malformed ENERGY STAR primary ID')


def validate(path, *, dataset=DATASET, name=NAME, fields=FIELDS, allow_grouped_ids=False):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    url = 'https://data.energystar.gov/d/' + dataset
    metadata_url = 'https://data.energystar.gov/api/views/' + dataset
    csv_url = 'https://data.energystar.gov/api/v3/views/' + dataset + '/export.csv?accessType=DOWNLOAD'
    if (data.get('schema_version') != 1 or data.get('dataset_url') != url
            or data.get('metadata_url') != metadata_url or data.get('csv_url') != csv_url):
        raise ValueError('Unrecognized ENERGY STAR export bundle')
    before = json.loads(dependency(path.parent, data['metadata_before']))
    after = json.loads(dependency(path.parent, data['metadata_after']))
    for meta in (before, after):
        if (meta.get('id') != dataset or meta.get('name') != name
                or type(meta.get('rowsUpdatedAt')) is not int
                or not isinstance(meta.get('columns'), list)):
            raise ValueError('Unexpected ENERGY STAR dataset metadata')
    if before['rowsUpdatedAt'] != after['rowsUpdatedAt'] or before['columns'] != after['columns']:
        raise ValueError('ENERGY STAR dataset changed during capture')
    columns = before['columns']; headers = [c.get('name') for c in columns]
    if (any(not isinstance(h, str) or not h for h in headers)
            or len(set(headers)) != len(headers) or not set(fields) <= set(headers)):
        raise ValueError('Missing or ambiguous ENERGY STAR columns')
    id_column = next(c for c in columns if c['name'] == fields[0])
    if allow_grouped_ids and id_column.get('dataTypeName') != 'number':
        raise ValueError('Formatted primary IDs require numeric source metadata')
    stats = id_column.get('cachedContents', {})
    if (id_column.get('fieldName') != 'pd_id' or stats.get('null') != '0'
            or not re.fullmatch(r'[1-9][0-9]*', str(stats.get('non_null', '')))
            or stats.get('count') != stats['non_null']):
        raise ValueError('Missing ENERGY STAR primary-ID completeness statistics')
    reader = csv.reader(io.StringIO(dependency(path.parent, data['csv']).decode('utf-8-sig')), strict=True)
    if next(reader, None) != headers:
        raise ValueError('ENERGY STAR CSV and metadata headers differ')
    rows = []; seen = set()
    for cells in reader:
        if len(cells) != len(headers):
            raise ValueError('ENERGY STAR CSV row width differs')
        row = dict(zip(headers, cells)); key = parse_id(row[fields[0]], allow_grouped_ids)
        if key in seen or any(not row[k] for k in fields[1:5]):
            raise ValueError('Missing or duplicate ENERGY STAR identity')
        seen.add(key); rows.append(row)
    if len(rows) != int(stats['non_null']):
        raise ValueError('ENERGY STAR CSV is incomplete against metadata count')
    return data, before, rows


def configuration_facts(row):
    facts = []
    for code, label in CONFIGS:
        fields = {
            'model_or_config_id': 'Model Number or Config ID for ' + label,
            'processor': 'Processor Model Name ' + label,
            'installed_processors': 'Installed Processors for ' + label,
            'installed_memory_gb': 'Installed Memory for ' + label + ' (GB)',
            'idle_watts': 'SERT Idle Measurement (Watts) ' + label,
            'psu_rated_output_watts': 'Power Supply Rated Output ' + label + ' (W)',
            'psu_model': 'Power Supply Model Number ' + label,
            'psu_count': 'Num. Power Supplies for ' + label,
        }
        values = {name: row.get(field, '') for name, field in fields.items()}
        facts.extend(('energystar_' + code + '_' + k, v) for k, v in values.items() if v)
        if values['idle_watts']:
            facts.append(('power_description', 'ENERGY STAR SERT idle measurement, ' + label + ': '
                          + values['idle_watts'] + ' W; reported test configuration, not an auction-unit estimate.'))
        if values['psu_rated_output_watts']:
            facts.append(('power_description', 'ENERGY STAR PSU rated output, ' + label + ': '
                          + values['psu_rated_output_watts'] + ' W; power-supply rating, not measured system consumption.'))
    return facts


def import_bundle(c, path):
    data, meta, rows = validate(path)
    c.source(SOURCE, URL, 'regulatory', 'EPA data license: https://edg.epa.gov/EPA_Data_License.html',
             'Full published enterprise-server CSV checked against before/after metadata columns, '
             'data-update timestamp and non-null primary-ID count. All brands and original fields '
             'are retained in energystar_servers. This is the current dataset snapshot, not all '
             'historically qualified or manufactured servers. Brand and program-partner affiliations '
             'are not physical manufacturer or factory evidence. Test configurations remain separate; '
             'idle measurements and PSU ratings are not auction-unit consumption estimates. SERT total '
             'power scores remain raw fields, not instantaneous measured power. No family-pattern '
             'expansion, configuration-ID alias, current validity or hyperscaler deployment inferred.',
             path, str(meta['rowsUpdatedAt']))
    c.db.execute('DELETE FROM energystar_servers WHERE source_id=?', (SOURCE,))
    added = 0
    for row in rows:
        url = DETAIL + row[FIELDS[0]]
        c.db.execute('INSERT INTO energystar_servers VALUES (' + ','.join('?' for _ in range(11)) + ')',
                     (SOURCE, *[row[k] for k in FIELDS], json.dumps(row, ensure_ascii=False), url))
        target = company_target(row['Brand Name'])
        if (target is None or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._ -]*', row['Model Number'])
                or re.search(r'x{2,}', row['Model Number'], re.I)):
            continue
        model = row['Model Number']
        facts = [('energystar_model_name', row['Model Name']),
                 ('energystar_model_identifier', row['ENERGY STAR Model Identifier']),
                 ('energystar_certified_date_raw', row['Date Certified']),
                 ('energystar_available_date_raw', row['Date Available On Market'])]
        facts += configuration_facts(row)
        for field in ['Form Factor', 'Product Processor Socket Count', 'Maximum Memory Capacity (GB)',
                      'Additional Model Information', 'Operating Systems Supported', 'Markets']:
            if row.get(field): facts.append(('energystar_' + field.lower().replace(' ', '_'), row[field]))
        c.add(SOURCE, row[FIELDS[0]], url, target, model, target + ' ' + model,
              'enterprise server', row,
              [(row['Brand Name'], 'brand'), (row['ENERGY STAR Partner'], 'program_partner')],
              [('energystar_id', row[FIELDS[0]])], facts)
        added += 1
    for company in {r[k] for r in rows for k in ('Brand Name', 'ENERGY STAR Partner')}:
        c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=?)', (company_target(company), company, SOURCE))
    c.finish(SOURCE, len(rows))
    return dict(registry_rows=len(rows), model_assertions=added)

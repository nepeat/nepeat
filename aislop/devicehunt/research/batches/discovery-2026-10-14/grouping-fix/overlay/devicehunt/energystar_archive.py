"""Historical server qualifications; original workbook and hashed cell extraction."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency
from .rra import company_target

INDEX = 'https://www.energystar.gov/products/enterprise_servers/partners'
BASE = 'https://www.energystar.gov/sites/default/files/asset/document/'
FORMATS = {
    'v2': dict(url=BASE + 'EnterpriseServers_V2_Archived.xlsx',
               cutoff='2019-06-17', sheets={'Enterprise Servers QPL V2': (4, 1, 0, 430)},
               method='openpyxl 3.1.5; stored cell values; formulas rejected'),
    'v1_1': dict(url=BASE + 'EnterpriseServers_V1.1_Archived.xls',
                 cutoff='2013-12-16', sheets={'Single Configurations V1.1': (14, 1, 1, 39),
                                            'Family Configurations V1.1': (14, 2, 1, 54)},
                 method='xlrd 2.0.2; stored cell values; Excel date serials retained'),
}


def text(value):
    if value is None: return ''
    if isinstance(value, dict): return value['iso_datetime']
    if type(value) is int or type(value) is float and value.is_integer(): return str(int(value))
    return str(value)


def fields(sheet, layout):
    """Resolve only explicit merged header cells, retaining configuration subheaders."""
    start, levels, skip, width = layout
    rows = sheet['rows']
    header = [list(r) for r in rows[start - 1:start - 1 + levels]]
    for r0, r1, c0, c1 in sheet['merged_ranges']:
        if r0 < start - 1 or r1 > start - 1 + levels: continue
        value = rows[r0][c0]
        for r in range(r0, r1):
            for col in range(c0, c1):
                header[r - start + 1][col] = value
    names = []
    for col in range(skip, width):
        components = list(dict.fromkeys(text(r[col]) for r in header if text(r[col])))
        names.append(' | '.join(components))
    if any(not n for n in names) or len(names) != len(set(names)):
        raise ValueError('Empty or ambiguous archive headers')
    return names


def validate(path):
    path = Path(path).resolve(); b = json.loads(path.read_text())
    spec = FORMATS.get(b.get('version'))
    if (not spec or b.get('schema_version') != 1 or b.get('url') != spec['url']
            or b.get('index_url') != INDEX):
        raise ValueError('Unexpected ENERGY STAR archive bundle')
    original = dependency(path.parent, b['workbook'])
    index = dependency(path.parent, b['index']).decode('utf-8')
    if spec['url'].removeprefix('https://www.energystar.gov') not in index:
        raise ValueError('Archive link missing from source index')
    data = json.loads(dependency(path.parent, b['cells']))
    if (data.get('schema_version') != 1 or data.get('workbook_sha256') != digest(original)
            or data.get('extraction_method') != spec['method']):
        raise ValueError('Archive extraction does not match workbook or method')
    sheets = data.get('sheets', [])
    if len({s['name'] for s in sheets}) != len(sheets):
        raise ValueError('Duplicate extracted sheet')
    if set(b.get('expected_rows', {})) != set(spec['sheets']):
        raise ValueError('Expected counts must include every qualification sheet')
    if set(b.get('header_sha256', {})) != set(spec['sheets']):
        raise ValueError('Expected header hashes must include every qualification sheet')
    result = []
    for name, layout in spec['sheets'].items():
        sheet = next((s for s in sheets if s['name'] == name), None)
        if sheet is None: raise ValueError('Missing qualification sheet')
        start, levels, skip, width = layout
        if any(len(row) != width for row in sheet['rows']):
            raise ValueError('Archive row width changed')
        for span in sheet['merged_ranges']:
            if (len(span) != 4 or any(type(v) is not int for v in span)
                    or not 0 <= span[0] < span[1] <= len(sheet['rows'])
                    or not 0 <= span[2] < span[3] <= width):
                raise ValueError('Invalid merged cell range')
        names = fields(sheet, layout)
        if digest(json.dumps(names, ensure_ascii=False).encode()) != b['header_sha256'][name]:
            raise ValueError('Archive headers changed')
        required = {'ENERGY STAR Partner', 'Model Name', 'Model Number',
                    'Brand Name' if b['version'] == 'v2' else 'Brand'}
        if not required <= set(names): raise ValueError('Missing archive identity columns')
        count = 0; ids = set()
        for n, cells in enumerate(sheet['rows'][start - 1 + levels:], start + levels):
            if not any(text(c) for c in cells): continue
            if any(text(c) for c in cells[:skip]): raise ValueError('Unexpected leading-column data')
            values = dict(zip(names, [text(c) for c in cells[skip:]]))
            if any(not values[k].strip() for k in required):
                raise ValueError('Archive row has missing identity fields')
            if b['version'] == 'v2':
                key = values.get('PD_ID', '')
                if not key.isascii() or not key.isdigit() or key in ids:
                    raise ValueError('Missing or duplicate archive PD_ID')
                ids.add(key)
            result.append((name, n, values, cells)); count += 1
        expected = b['expected_rows'][name]
        if type(expected) is not int or expected < 1 or count != expected:
            raise ValueError('Archive record count differs from reviewed extraction')
    return b, spec, result


def power_facts(values, context):
    facts = []
    for field, value in values.items():
        if not re.fullmatch(r'[0-9]+(?:\.[0-9]+)?', value): continue
        if ((field.startswith(('Idle Power Draw', 'Idle Power ')) and '(W)' in field)
                or field.startswith('SERT Idle Measurement (Watts)')):
            kind = 'reported idle measurement'
        elif field.startswith('Power Supply Rated Output'):
            kind = 'PSU output rating, not system consumption'
        elif field.startswith('Full Power Load (W)'):
            kind = 'reported full-load measurement'
            supply = values.get(field.replace('Full Power Load (W)', 'Full Load Testing Voltage / Frequency'))
            if supply: kind += '; test voltage/frequency: ' + supply
        else: continue
        facts.append(('power_description', 'Archived ENERGY STAR ' + context + '; ' + field
                      + ': ' + value + ' W (' + kind + '; listed test configuration only).'))
    return facts


def import_bundle(c, path):
    b, spec, rows = validate(path)
    sid = 'energystar-server-archive-' + b['version']
    c.source(sid, b['url'], 'regulatory', 'EPA data license: https://edg.epa.gov/EPA_Data_License.html',
             'Complete nonempty qualification rows from the archived workbook, using a hashed cell extraction. '
             'Historical list preceding specification change ' + spec['cutoff'] + '; not current qualification '
             'or a complete history of manufactured servers. Original workbook, sheet and row retained. '
             'Workbook row count is reviewed, not an independent registry total. Program partner/brand '
             'are not manufacturer or factory claims. Family sheet catalog models use Model Name and keep '
             'Model Number/configuration IDs as facts; no alias expansion. Power fields remain scoped to '
             'the listed row/configuration/voltage and do not predict auction-unit consumption. '
             'No ownership, deployment, support or cross-brand identity inferred.', path, spec['cutoff'])
    c.db.execute('DELETE FROM energystar_server_archives WHERE source_id=?', (sid,))
    added = 0
    for sheet, number, values, cells in rows:
        partner = values['ENERGY STAR Partner']; brand = values.get('Brand Name', values.get('Brand', ''))
        family = sheet == 'Family Configurations V1.1'
        model = values['Model Name'] if family else values['Model Number']
        key = sheet + ':' + str(number)
        certified = values.get('Date Qualified', values.get('Date Qualified**', ''))
        available = values.get('Date Available On Market', values.get('Date Available On Market*',
                               values.get('Date Available on Market*', '')))
        c.db.execute('INSERT INTO energystar_server_archives VALUES (' + ','.join('?' for _ in range(13)) + ')',
                     (sid, sheet, number, partner, brand, values['Model Name'], values['Model Number'],
                      values.get('ENERGY STAR Model Identifier', ''), certified, available,
                      json.dumps(values, ensure_ascii=False), json.dumps(cells, ensure_ascii=False), b['url']))
        if not (company_target(brand) or company_target(partner)): continue
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._ -]*', model) or re.search(r'x{2,}', model, re.I): continue
        context = (values['Model Number'] if family else model)
        facts = [('energystar_archive_sheet', sheet), ('energystar_archive_row', str(number)),
                 ('energystar_archive_cutoff', spec['cutoff']), ('energystar_archive_model_number', values['Model Number'])]
        facts += power_facts(values, context)
        for field, value in values.items():
            if value and any(term in field for term in ('Processor', 'Memory', 'Config', 'Date ', 'Model Identifier')):
                facts.append(('energystar_archive_' + field, value))
        identifiers = [('energystar_id', values['PD_ID'])] if 'PD_ID' in values else []
        c.add(sid, key, b['url'], company_target(brand) or brand, model, brand + ' ' + model,
              'enterprise server', values, [(brand, 'brand'), (partner, 'program_partner')], identifiers, facts)
        added += 1
    for company in {v[k] for _, _, v, _ in rows for k in ('ENERGY STAR Partner', 'Brand Name' if b['version'] == 'v2' else 'Brand')}:
        c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=?)', (company_target(company), company, sid))
    c.finish(sid, len(rows))
    return dict(registry_rows=len(rows), model_assertions=added)

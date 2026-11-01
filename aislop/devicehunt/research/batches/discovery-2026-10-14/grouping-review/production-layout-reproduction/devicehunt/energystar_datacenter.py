"""All-brand network/storage exports and historical storage qualifications."""
import json
import re
from pathlib import Path

from . import energystar
from .catalog import digest
from .documents import dependency
from .energystar_archive import fields as archive_fields, text
from .rra import company_target

SPECS = {
    'network': ('n8cx-m62r', 'ENERGY STAR Certified Large Network Equipment',
                'ENERGY STAR Unique ID', 'CB Model Identifier', 'Date Certified', 'Date Available on Market'),
    'storage-file': ('put7-uu67', 'ENERGY STAR Certified Data Center Storage - File I/O',
                     'PD_ID', 'ENERGY STAR Model Identifier', 'Date Qualified', 'Date Available On Market'),
    'storage-block': ('3uec-2gqf', 'ENERGY STAR Certified Data Center Storage - Block I/O',
                      'ENERGY STAR Unique ID', 'CB Model Identifier', 'Date Qualified', 'Date Available On Market'),
}
INDEX = 'https://www.energystar.gov/products/data_center_storage/partners'
ARCHIVES = {
    'storage-file-v1': ('https://www.energystar.gov/sites/default/files/asset/document/DataCenterStorage__File_IO_V1_Archived.xlsx', 282),
    'storage-block-v1': ('https://www.energystar.gov/sites/default/files/asset/document/DataCenterStorage_Block_IO_V1_Archived.xlsx', 289),
}
IDENTITY = ['ENERGY STAR Partner', 'Brand Name', 'Model Name', 'Model Number']


def validate_archive(path, b, kind):
    url, width = ARCHIVES[kind]
    if b.get('schema_version') != 1 or b.get('url') != url or b.get('index_url') != INDEX:
        raise ValueError('Unexpected historical storage bundle')
    original = dependency(path.parent, b['workbook'])
    index = dependency(path.parent, b['index']).decode('utf-8')
    if url.removeprefix('https://www.energystar.gov') not in index:
        raise ValueError('Historical storage link missing from index')
    data = json.loads(dependency(path.parent, b['cells']))
    if (data.get('schema_version') != 1 or data.get('workbook_sha256') != digest(original)
            or data.get('extraction_method') != 'openpyxl 3.1.5; stored cell values; formulas rejected'):
        raise ValueError('Unverified historical storage extraction')
    sheets = data.get('sheets', [])
    if len(sheets) != 1 or sheets[0].get('name') != 'Storage QPL V1':
        raise ValueError('Unexpected historical storage sheets')
    sheet = sheets[0]
    if len(sheet['rows']) < 5 or any(len(row) != width for row in sheet['rows']):
        raise ValueError('Historical storage row shape changed')
    # These two original sheets have no merged cells in their one-row header.
    if any(r[0] <= 3 < r[1] for r in sheet['merged_ranges']):
        raise ValueError('Historical storage header merges changed')
    names = archive_fields(sheet, (4, 1, 0, width))
    if digest(json.dumps(names, ensure_ascii=False).encode()) != b.get('header_sha256'):
        raise ValueError('Historical storage headers changed')
    if not set(['PD_ID', *IDENTITY]) <= set(names):
        raise ValueError('Missing historical storage identity fields')
    rows = []; ids = set()
    for n, cells in enumerate(sheet['rows'][4:], 5):
        if not any(text(c) for c in cells): continue
        row = dict(zip(names, map(text, cells))); key = row['PD_ID']
        if not re.fullmatch(r'[0-9]+', key) or key in ids or any(not row[k].strip() for k in IDENTITY):
            raise ValueError('Missing or duplicate historical storage identity')
        ids.add(key); rows.append((sheet['name'], n, row))
    if type(b.get('expected_rows')) is not int or b['expected_rows'] < 1 or len(rows) != b['expected_rows']:
        raise ValueError('Historical storage row count differs')
    return url, rows


def import_bundle(c, path):
    path = Path(path).resolve(); b = json.loads(path.read_text()); kind = b.get('dataset_kind')
    if kind in SPECS:
        dataset, name, id_field, identifier, certified, available = SPECS[kind]
        required = [id_field, *IDENTITY, identifier, certified, available]
        _, meta, raw = energystar.validate(path, dataset=dataset, name=name, fields=required,
                                          allow_grouped_ids=kind == 'storage-file')
        url = b['dataset_url']; revision = str(meta['rowsUpdatedAt'])
        rows = [('', n, row) for n, row in enumerate(raw, 1)]
        scope = 'Full published CSV checked against before/after metadata and primary-ID totals. '
    elif kind in ARCHIVES:
        url, rows = validate_archive(path, b, kind)
        id_field = 'PD_ID'; identifier = 'ENERGY STAR Model Identifier'
        certified = 'Date Qualified'; available = 'Date Available On Market'; revision = '2021-03-15'
        scope = ('Historical qualification list preceding the March 15, 2021 specification change; '
                 'all nonempty qualification rows from the original workbook and hashed extraction. '
                 'Row count is reviewed from the workbook, not an independent registry total. ')
    else:
        raise ValueError('Unrecognized ENERGY STAR data-center dataset')
    sid = 'energystar-' + kind
    c.source(sid, url, 'regulatory', 'EPA data license: https://edg.epa.gov/EPA_Data_License.html',
             scope + 'All brands and fields retained in energystar_datacenter. Brand/program partner '
             'are not manufacturer or factory claims. No component, controller, module, power-supply '
             'or configuration identifier is promoted to a chassis alias or maker. No historical '
             'completeness, current validity, hyperscaler deployment or surplus-unit ownership inferred. '
             'Storage GB/W and MiBPS/W are efficiency ratios, not wattage. Test conditions and '
             'configuration fields remain separate, without estimating auction-unit consumption.', path, revision)
    c.db.execute('DELETE FROM energystar_datacenter WHERE source_id=?', (sid,))
    added = 0
    for sheet, number, row in rows:
        key = energystar.parse_id(row[id_field], allow_grouped_ids=kind == 'storage-file')
        detail = ('https://www.energystar.gov/productfinder/product/certified-' +
                  ('large-network-equipment' if kind == 'network' else 'data-center-storage') + '/details/' + key)
        source_url = url if kind in ARCHIVES else detail
        c.db.execute('INSERT INTO energystar_datacenter VALUES (' + ','.join('?' for _ in range(14)) + ')',
                     (sid, kind, key, *[row[k] for k in IDENTITY], row.get(identifier, ''),
                      row.get(certified, ''), row.get(available, ''), json.dumps(row, ensure_ascii=False),
                      source_url, sheet, number))
        target = company_target(row['Brand Name']); model = row['Model Number']
        if target is None or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._ -]*', model) or re.search(r'x{2,}', model, re.I):
            continue
        facts = [('energystar_field_' + k, v) for k, v in row.items() if v]
        facts += [('energystar_dataset_kind', kind), ('energystar_source_row', str(number))]
        if sheet: facts.append(('energystar_source_sheet', sheet))
        c.add(sid, key, source_url, target, model, target + ' ' + model,
              'network equipment' if kind == 'network' else 'storage system', row,
              [(row['Brand Name'], 'brand'), (row['ENERGY STAR Partner'], 'program_partner')],
              [('energystar_id', key)], facts)
        added += 1
    for company in {row[k] for _, _, row in rows for k in ('Brand Name', 'ENERGY STAR Partner')}:
        c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=?)', (company_target(company), company, sid))
    c.finish(sid, len(rows))
    return dict(registry_rows=len(rows), model_assertions=added)

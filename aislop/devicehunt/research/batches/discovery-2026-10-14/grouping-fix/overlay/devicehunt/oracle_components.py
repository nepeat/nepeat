"""Oracle ZFS component options; source-scoped part mappings, not OEM claims."""
import json
import re
from pathlib import Path

from .documents import dependency
from .fcc_index import Tree

BASE = 'https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/'
PROFILES = {
    'sun-zfs-storage-7420': ('Sun ZFS Storage 7420', 'GOWZJ', 9, 9, True),
    'oracle-zfs-storage-zs3-2': ('Oracle ZFS Storage ZS3-2', 'GOWTA', 10, 11, False),
    'oracle-zfs-storage-zs3-4': ('Oracle ZFS Storage ZS3-4', 'GOWRN', 10, 10, False),
}
PART = re.compile(r'[A-Z0-9]+(?:-[A-Z0-9]+)*')


def parse(body, slug):
    system, suffix, row_count, entry_count, has_mfg = PROFILES[slug]
    root = Tree(body).root
    if [n.text() for n in root.walk('h2')] != [system + ' PCIe I/O Cards']:
        raise ValueError('Oracle component page heading changed')
    tables = [t for t in root.walk('table') if t.parent.attrs.get('id', '').endswith('__' + suffix)]
    if len(tables) != 1 or system + ' Controller NIC/HBA Options' not in tables[0].parent.text():
        raise ValueError('Missing Oracle NIC/HBA options table')
    table = tables[0]; trs = list(table.walk('tr'))
    if len(trs) != row_count + 1:
        raise ValueError('Oracle component table row count changed')
    header = ['Mktg Part Number', 'Description'] + (['Mfg Part Number'] if has_mfg else [])
    ready, seen = [], set()
    for number, tr in enumerate(trs):
        cells = [n for n in tr.children if getattr(n, 'tag', '') in ('td', 'th')]
        if len(cells) != len(header) or any(n.attrs.get(k, '1') != '1' for n in cells for k in ('rowspan', 'colspan')):
            raise ValueError('Unexpected Oracle component columns or spanning cells')
        if number == 0:
            if [n.text() for n in cells] != header:
                raise ValueError('Oracle component table header changed')
            continue
        # Paired paragraphs in the same row retain their source order. Do not
        # split identifiers/descriptions on spaces or transfer mappings across rows.
        groups = [[n.text() for n in cell.walk('p')] for cell in cells]
        if not groups[0] or len({len(group) for group in groups}) != 1:
            raise ValueError('Unpaired Oracle component paragraphs')
        if any(' '.join(group) != cell.text() for group, cell in zip(groups, cells)):
            raise ValueError('Unparsed Oracle component cell text')
        for position, values in enumerate(zip(*groups), 1):
            model, description = values[:2]
            mfg = values[2] if has_mfg else ''
            if (not PART.fullmatch(model) or model in seen or not description
                    or (has_mfg and not PART.fullmatch(mfg))):
                raise ValueError('Invalid or duplicate Oracle component part number')
            seen.add(model)
            ready.append({'marketing_part_number': model, 'description': description,
                          'manufacturing_part_number': mfg, 'system': system,
                          'table_fragment': table.parent.attrs['id'], 'table_row': number,
                          'paragraph': position, 'raw_cells': [n.text() for n in cells]})
    if len(ready) != entry_count:
        raise ValueError('Oracle component assertion count changed')
    return ready


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or set(data.get('pages', {})) != set(PROFILES):
        raise ValueError('Incomplete Oracle component bundle')
    ready = []
    for slug in PROFILES:
        entry = data['pages'][slug]
        url = BASE + slug + '-pcie-io-cards.html'
        if entry.get('url') != url:
            raise ValueError('Oracle component URL changed')
        body = dependency(path.parent, entry['html']).decode('utf-8')
        ready.append((slug, url, parse(body, slug)))
    for slug, url, rows in ready:
        sid = 'oracle-components-' + slug
        c.source(sid, url, 'vendor', 'Factual component option fields; Oracle source rights retained',
                 'Selected NIC/HBA options table from Oracle ZFS service documentation. Marketing part '
                 'numbers identify documented component options in an Oracle namespace, not physical OEM. '
                 'Mfg Part Number mappings stay scoped to the source record and are not universal aliases. '
                 'Descriptions and conflicting statements are retained per system; no current support, '
                 'general interchangeability, firmware compatibility or hyperscaler deployment inferred.', path)
        for row in rows:
            model = row['marketing_part_number']
            facts = [('documented_component_description', row['description']),
                     ('documented_system_option', row['system']),
                     ('identity_scope', 'Oracle documentation namespace; component option, not manufacturer attribution'),
                     ('source_table_row', str(row['table_row'])), ('source_paragraph', str(row['paragraph']))]
            ids = []
            if row['manufacturing_part_number']:
                ids.append(('oracle_mfg_part_number', row['manufacturing_part_number']))
                facts.append(('mapping_scope', 'Printed Mktg Part Number to Mfg Part Number in this source row; not a universal alias'))
            c.add(sid, model, url + '#' + row['table_fragment'], 'Oracle', model,
                  'Oracle component ' + model, 'documented component option', row,
                  [('Oracle', 'documentation_vendor')], ids, facts)
        c.finish(sid, len(rows))
    return {'sources': len(ready), 'assertions': sum(len(rows) for _, _, rows in ready),
            'marketing_parts': len({r['marketing_part_number'] for _, _, rows in ready for r in rows})}

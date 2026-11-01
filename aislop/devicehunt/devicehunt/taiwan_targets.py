"""Exact Chinese company mappings backed by cached primary company and government pages."""
import json
from collections import Counter
from pathlib import Path

from . import bsmi, ncc, bsmi_additional
from .catalog import digest
from .fcc_index import Tree, Node

EDGECORE_NAME_URL = 'https://www.sipa.gov.tw/home.jsp?contlink=ap%2Fmanufacturers_1_3_view.jsp&dataserno=A0902&level2=Y&menudata=ChineseMenu&mserno=201002010011&qsipa=01&serno=201002010011&stype=C'

MAPPINGS = {
    '鈺登科技股份有限公司': ('Edge-Core', EDGECORE_NAME_URL),
    '智邦科技股份有限公司': ('Accton', 'https://www.accton.com/tw/accton-jun-2026-sales-revenue-report/'),
    '美商甲骨文有限公司台灣分公司': ('Oracle', 'https://www.oracle.com/tw/corporate/contact/field-offices/'),
}
SOURCES = {'bsmi': 'bsmi-reviewed-targets', 'ncc': 'ncc-reviewed-targets',
           'bsmi_product': 'bsmi-product-reviewed-targets'}


def dependency(root, entry, url):
    name = entry.get('path')
    if (not isinstance(name, str) or not name or Path(name).is_absolute()
            or '..' in Path(name).parts or entry.get('url') != url):
        raise ValueError('Invalid Taiwan target dependency')
    path = (root / name).resolve()
    if root not in path.parents:
        raise ValueError('Taiwan dependency escapes bundle directory')
    body = path.read_bytes()
    if digest(body) != entry.get('sha256'):
        raise ValueError('Taiwan dependency checksum mismatch')
    return body


def validate_company_pages(pages):
    oracle = Tree(pages['Oracle'].decode('utf-8')).root
    headings = [n for n in oracle.walk('h4')
                if n.text() == '美商甲骨文有限公司台灣分公司 – 台北辦事處 辦公室']
    if (len(headings) != 1 or headings[0].parent.tag != 'li'
            or not any(n.text() == 'Oracle Taiwan LLC, Taiwan Branch'
                       for n in headings[0].parent.walk('li'))):
        raise ValueError('Oracle Taiwan legal-name evidence changed')
    accton = Tree(pages['Accton'].decode('utf-8')).root
    if not any('智邦科技股份有限公司 (Accton)' in n.text() for n in accton.walk('p')):
        raise ValueError('Accton Chinese-name evidence changed')
    if 'Edge-Core' in pages:
        root = Tree(pages['Edge-Core'].decode('utf-8')).root
        required = {('廠商編號', 'A0902'), ('廠商名稱 (中文)', '鈺登科技股份有限公司'),
                    ('廠商名稱 (英文)', 'Edgecore Networks Corporation'),
                    ('營利事業 統一編號', '25063543')}
        matches = []
        for table in root.walk('table'):
            rows = []
            for tr in table.walk('tr'):
                # Only this table's own rows, never join fields from nested companies.
                if tr.parent is not table and not (tr.parent.tag in ('thead', 'tbody', 'tfoot') and tr.parent.parent is table):
                    continue
                rows.append(tuple(c.text() for c in tr.children if isinstance(c, Node) and c.tag in ('td', 'th')))
            if required.issubset(set(rows)):
                matches.append(rows)
        if len(matches) != 1:
            raise ValueError('Edgecore bilingual government company record changed')


def select_rows(bsmi_rows, ncc_rows, product_rows=(), mappings=MAPPINGS):
    selected = []
    for registry, rows in [('bsmi', bsmi_rows), ('ncc', ncc_rows), ('bsmi_product', product_rows)]:
        fields = [('applicant', 'regulatory_applicant')]
        if registry == 'ncc':
            fields.append(('manufacturer', 'registry_manufacturer'))
        for number, row in enumerate(rows, 1):
            matched = [(field, role, mappings[row[field]][0], row[field], mappings[row[field]][1])
                       for field, role in fields if row[field] in mappings]
            if not matched:
                continue
            model = row['model' if registry == 'ncc' else 'primary_model']
            if not model.strip() or '申請者申請保密' in model:
                continue
            # Use the printed NCC brand, even for another company's module.
            # BSMI has no brand column: the target is an applicant namespace.
            brand = row['brand'] if registry == 'ncc' else matched[0][2]
            if not brand.strip() or '申請者申請保密' in brand:
                brand = 'NCC applicant/manufacturer: ' + matched[0][2]
            selected.append(dict(registry=registry, row_number=number, raw=row,
                                 model=model, brand=brand, mappings=matched))
    return selected


def summary(selected):
    return {'assertions': len(selected),
            'registries': dict(sorted(Counter(r['registry'] for r in selected).items())),
            'mapped_roles': dict(sorted(Counter(r['registry'] + ':' + m[2] + ':' + m[1]
                                                for r in selected for m in r['mappings']).items()))}


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text()); root = path.parent
    if data.get('schema_version') not in (1, 2):
        raise ValueError('Unrecognized Taiwan target bundle')
    targets = {'Accton', 'Oracle'} | ({'Edge-Core'} if data['schema_version'] == 2 else set())
    mappings = {name: item for name, item in MAPPINGS.items() if item[0] in targets}
    if set(data.get('companies', {})) != targets:
        raise ValueError('Taiwan target company set changed')
    pages = {target: dependency(root, data['companies'][target], url)
             for target, url in mappings.values()}
    validate_company_pages(pages)
    bsmi_rows = bsmi.parse_xml(dependency(root, data['bsmi'], bsmi.XML_URL))
    ncc_rows = ncc.parse_csv(dependency(root, data['ncc'], ncc.CSV_URL))
    if bsmi.summary(bsmi_rows) != data.get('bsmi_summary') or ncc.summary(ncc_rows) != data.get('ncc_summary'):
        raise ValueError('Taiwan raw registry summaries changed')
    product_rows = []
    if data['schema_version'] == 2:
        product_rows = bsmi.parse_xml(dependency(root, data['bsmi_product'], bsmi_additional.urls('162936')[1]),
                                     bsmi_additional.DATASETS['162936']['fields'])
        if bsmi_additional.summary(product_rows, '162936') != data.get('bsmi_product_summary'):
            raise ValueError('Taiwan additional BSMI registry summary changed')
    selected = select_rows(bsmi_rows, ncc_rows, product_rows, mappings); counts = summary(selected)
    if counts != data.get('expected') or not selected:
        raise ValueError('Taiwan target counts differ from reviewed snapshot')
    # Validate every dependency and selection before any source is replaced.
    registries = [('bsmi', bsmi.PAGE_URL, bsmi.ATTRIBUTION), ('ncc', ncc.PAGE_URL, ncc.ATTRIBUTION)]
    if data['schema_version'] == 2:
        registries.append(('bsmi_product', bsmi_additional.urls('162936')[0],
            'Bureau of Standards, Metrology and Inspection (Taiwan), 2026, 商品驗證機構資料集 (dataset 162936). Open Government Data License v1.0: ' + bsmi.LICENSE_URL))
    for registry, page_url, attribution in registries:
        sid = SOURCES[registry]
        c.source(sid, page_url, 'regulatory', attribution,
                 'Exact Chinese applicant/manufacturer cells mapped to reviewed targets using '
                 'hashed company-owned or government bilingual name evidence. No substring, translation guess or multi-company '
                 'cell splitting. BSMI uses an applicant catalog namespace; NCC preserves printed brand. '
                 'Whole model annotations and suffixes remain. Applicant is not manufacturer; '
                 'registry manufacturer is not independent factory verification. No operator, '
                 'current validity or support inference. ' + attribution, path)
        records = [r for r in selected if r['registry'] == registry]
        for record in records:
            row = record['raw']; mappings = record['mappings']
            aff = [(target, role) for _, role, target, _, _ in mappings]
            if registry == 'ncc':
                aff += [(row['brand'], 'registry_brand'), (row['applicant'], 'regulatory_applicant'),
                        (row['manufacturer'], 'registry_manufacturer')]
            facts = [('registry_' + k, v) for k, v in row.items()]
            facts += [('company_name_evidence', json.dumps({'field': f, 'raw_name': raw, 'target': t, 'url': u}, ensure_ascii=False))
                      for f, _, t, raw, u in mappings]
            facts.append(('evidence_scope', 'Dated regulatory source claim. Applicant, brand and manufacturer remain separate; no operator attribution.'))
            raw = {**record, 'company_evidence': data['companies'], 'registry_dependency': data[registry]}
            c.add(sid, str(record['row_number']), page_url, record['brand'], record['model'],
                  record['brand'] + ' ' + record['model'], 'registered equipment', raw, aff,
                  [(registry + '_certificate_number', row['certificate_number'])], facts)
        c.finish(sid, len(records))
    return counts

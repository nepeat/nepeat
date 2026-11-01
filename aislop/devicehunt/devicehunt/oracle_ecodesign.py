"""Source-scoped Oracle compliance-code/product relationships, never aliases."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency, read_pages

DOCUMENTS = {
    'x9-2': ('https://www.oracle.com/a/otn/docs/WH1CL_X9-2_EcoDesignReportingInfo.pdf', 'X9-2', 'WH1CL', 3),
    'x9-2l': ('https://www.oracle.com/a/otn/docs/WH2CL_X9-2L_EcoDesignReportingInfo.pdf', 'X9-2L', 'WH2CL', 3),
    'x10m': ('https://www.oracle.com/a/otn/docs/x10m_2u_zu2xl_ecodesignreportinginfo.pdf', 'X10M', 'ZU2XL', 2),
    'x11m': ('https://www.oracle.com/a/otn/docs/X11M_2U_ZU2XL_EcoDesignReportingInfo.pdf', 'X11M', 'ZU2XL', 2),
}


def parse_page(text):
    text = ' '.join(text.split())
    if 'Technical Documentation for EU regulation 2019/424' not in text:
        raise ValueError('Oracle EcoDesign document heading changed')
    pattern = (r'Product Type (.*?) Name/Address of Manufacturer (.*?) '
               r'Product Model Number Compliance Model Number '
               r'(\S+) Server, all models as defined in the DoC (\S+) '
               r'Year of Manufacture (\d{4}) PSU Efficiency ')
    matches = re.findall(pattern, text)
    if len(matches) != 1:
        raise ValueError('Oracle EcoDesign identity table changed')
    product_type, manufacturer, product, code, year = matches[0]
    if product_type != 'Rack-mount Server' or not re.fullmatch(r'Oracle America Inc[ .…]+', manufacturer):
        raise ValueError('Oracle EcoDesign manufacturer or product type changed')
    psu = re.findall(r'Power Supply rated power output (\d+W) Idle State Power ', text)
    if len(psu) != 1:
        raise ValueError('Oracle EcoDesign PSU rating changed')
    return dict(product_model=product, compliance_model=code, manufacturer='Oracle America Inc.',
                manufacturer_raw=manufacturer, product_type=product_type,
                product_scope=product + ' Server, all models as defined in the DoC',
                year_of_manufacture_raw=year, psu_rated_output_raw=psu[0],
                document_marking='Confidential - Oracle Restricted' if 'Confidential - Oracle Restricted' in text else '')


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or set(data.get('documents', {})) != set(DOCUMENTS):
        raise ValueError('Oracle EcoDesign bundle scope changed')
    ready = []
    for key, (url, product, code, count) in DOCUMENTS.items():
        entry = data['documents'][key]
        if entry.get('url') != url:
            raise ValueError('Oracle EcoDesign URL changed')
        pdf = dependency(path.parent, entry['pdf'])
        if not pdf.startswith(b'%PDF-'):
            raise ValueError('Oracle EcoDesign input is not a PDF')
        sha = digest(pdf); pages = read_pages(dependency(path.parent, entry['text']), sha)
        if len(pages) != count:
            raise ValueError('Oracle EcoDesign page count changed')
        row = parse_page(pages[0]['text'])
        if (row['product_model'], row['compliance_model']) != (product, code) or row != entry.get('expected'):
            raise ValueError('Oracle EcoDesign reviewed fields changed')
        ready.append((key, url, row, sha))
    for key, url, row, sha in ready:
        sid = 'oracle-ecodesign-' + key
        c.source(sid, url, 'vendor', 'Factual manufacturer documentation; original Oracle document rights retained',
                 'Visually reviewed page 1 identity table. Named legal manufacturer and '
                 'compliance-code/product relationship only. Product scope remains qualified by '
                 'the DoC. Compliance codes may cover multiple products and do not become aliases. '
                 'Year is a source field, not a unit manufacturing date. PSU output rating is '
                 'not wall draw. No physical factory, operator, current support or certification inference.', path)
        facts = [(name, value) for name, value in row.items()]
        facts += [('pdf_sha256', sha), ('source_page', '1'),
                  ('identity_scope', 'Compliance code and qualified product scope are related by this document, not interchangeable model aliases.'),
                  ('manufacturer_scope', 'Named legal manufacturer; no independent physical factory verification.'),
                  ('power_description', row['product_scope'] + ': power supply rated output ' + row['psu_rated_output_raw'] + '; not measured wall draw or whole-server maximum consumption.'),
                  ('physical_description', row['product_scope'] + ': ' + row['product_type'])]
        c.add(sid, row['compliance_model'], url + '#page=1', 'Oracle', row['compliance_model'],
              'Oracle ' + row['compliance_model'], 'compliance model',
              {**row, 'page': 1, 'pdf_sha256': sha}, [(row['manufacturer'], 'manufacturer')],
              [('ecodesign_product_scope', row['product_scope'])], facts)
        c.finish(sid, 1)
    return {'sources': len(ready), 'assertions': len(ready),
            'compliance_codes': len({r['compliance_model'] for _, _, r, _ in ready})}

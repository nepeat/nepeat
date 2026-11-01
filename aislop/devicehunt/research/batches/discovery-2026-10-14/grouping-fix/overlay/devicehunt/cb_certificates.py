"""Reviewed Cisco-hosted CB certificates with separate manufacturer/factory roles."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency, read_pages

SOURCE = 'cisco-ncs-cb-certificates'
URL = 'https://www.cisco.com/web/dofc/21435998.pdf'
REVIEWED = [('US-37447-M3-UL', 1), ('US-37446-M3-UL', 3)]


def field(text, start, end):
    if text.count(start) != 1 or text.count(end) != 1:
        raise ValueError('Missing or ambiguous CB field: ' + start)
    value = text.split(start, 1)[1].split(end, 1)[0].strip()
    if not value:
        raise ValueError('Empty CB field: ' + start)
    return value


def certificate(front, back, certificate_id):
    a, b = (' '.join(p['text'].split()) for p in (front, back))
    for text, page_marker in [(a, '1/2'), (b, '2/2')]:
        ids = re.findall(r'Ref\. Certif\. No\. (US-[A-Z0-9-]+)', text)
        if ids != [certificate_id] or not text.startswith(page_marker):
            raise ValueError('CB certificate/page pairing mismatch')
    for text in (a, b):
        if not re.search(r'Date: 2023\s*-08-31 Original Issue Date: 2021\s*-01-28', text):
            raise ValueError('Unreviewed CB certificate date')
    product = field(a, 'CB TEST CERTIFICATE Product', 'Name and address of the applicant')
    applicant = field(a, 'Name and address of the applicant', 'Name and address of the manufacturer')
    manufacturer = field(a, 'Name and address of the manufacturer', 'Name and address of the factory')
    primary_factory = field(a, 'Note: When more than one factory, please report on page 2', 'Ratings and principal characteristics')
    primary_factory = primary_factory.removesuffix('Additional Information on page 2').strip()
    extra_factory = field(b, 'Factory(ies):', 'Additional Ratings:' if 'Additional Ratings:' in b else 'Summary of Modifications')
    if (not applicant.startswith('CISCO SYSTEMS INC ') or applicant != manufacturer
            or not primary_factory.startswith('CELESTICA (THAILAND) LIMITED ')
            or not extra_factory.startswith('FABRINET CO LTD ')
            or product != 'Network Convergence System'):
        raise ValueError('Unreviewed CB company roles or product')
    model_cell = field(a, 'Model / Type Ref.', 'Additional information (if necessary')
    model_cell = model_cell.removesuffix('Additional Information on page 2').strip()
    match = re.fullmatch(r'(NCS\d+(?:\s*,\s*NCS\d+)*)\s*,\s*(may be followed by additional alphanumeric characters or blank)', model_cell)
    if not match:
        raise ValueError('Unreviewed CB model syntax')
    models = [m.strip() for m in match[1].split(',')]
    if len(set(models)) != len(models):
        raise ValueError('Duplicate CB model')
    ratings_text = field(a, 'Ratings and principal characteristics', 'Trademark / Brand (if any)')
    ratings_text = ratings_text.replace('Additional Information on page 2', '').strip()
    if 'Additional Ratings:' in b:
        ratings_text += ' ' + field(b, 'Additional Ratings:', 'Summary of Modifications')
    ratings = {}
    for m in re.finditer(r'(NCS\d+)\s*:?\s*(.*?)(?=NCS\d+|$)', ratings_text):
        if m[1] in ratings or not m[2].strip():
            raise ValueError('Ambiguous CB model power rating')
        ratings[m[1]] = m[2].strip()
    if set(ratings) != set(models):
        raise ValueError('CB ratings and models do not agree')
    return {'certificate_id': certificate_id, 'pages': [front['page'], back['page']],
            'product': product, 'applicant': applicant, 'manufacturer': manufacturer,
            'factories': [primary_factory, extra_factory], 'models': models,
            'model_cell': model_cell, 'suffix_qualifier': match[2], 'ratings': ratings,
            'issue_date': '2023-08-31', 'original_issue_date': '2021-01-28'}


def import_bundle(c, path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('url') != URL
            or data.get('reviewed_certificates') != [r[0] for r in REVIEWED]
            or data.get('visually_reviewed_pages') != [1, 2, 3, 4]):
        raise ValueError('Unrecognized CB certificate review')
    pdf = dependency(path.parent, data['pdf'])
    if not pdf.startswith(b'%PDF-'):
        raise ValueError('CB source is not a PDF')
    sha = digest(pdf)
    pages = read_pages(dependency(path.parent, data['text']), sha)
    if len(pages) != 6:
        raise ValueError('CB document page count changed')
    ready = [certificate(pages[n-1], pages[n], cid) for cid, n in REVIEWED]
    c.source(SOURCE, URL, 'vendor', 'Factual extracts from Cisco-hosted UL certificates; source rights retained',
             'Two reviewed 2023 M3 certificates on PDF pages 1-4; the older 2021 certificate on pages 5-6 is retained '
             'in the original archive but not imported. Cisco is named manufacturer/applicant; Celestica Thailand '
             'and Fabrinet are listed factories for the certificate scope, not proven factories of any particular unit. '
             'Explicit base models only; open-ended suffix language is never expanded into invented ordering codes. '
             'Separate AC/DC ratings are certificate/model-specific alternatives, not universal PSU capabilities. '
             'No current certificate validity, software rights or hyperscaler deployment inferred.', path)
    for cert in ready:
        for model in cert['models']:
            facts = [('cb_certificate', cert['certificate_id']), ('certificate_issue_date', cert['issue_date']),
                     ('certificate_original_issue_date', cert['original_issue_date']),
                     ('source_pdf_pages', ', '.join(map(str, cert['pages']))), ('pdf_sha256', sha),
                     ('source_model_cell', cert['model_cell']), ('model_suffix_qualifier', cert['suffix_qualifier']),
                     ('factory_scope', 'Factories listed for certificate scope; factory of an individual unit is unverified'),
                     ('certificate_manufacturer_address', cert['manufacturer']),
                     ('power_description', model + ' rating in ' + cert['certificate_id'] + ': ' + cert['ratings'][model] +
                      '. Applies to this certificate configuration; inspect the unit and PSU labels before selecting input power.')]
            facts += [('listed_factory_address', f) for f in cert['factories']]
            c.add(SOURCE, cert['certificate_id'] + ':' + model, URL + '#page=' + str(cert['pages'][0]),
                  'Cisco', model, 'Cisco ' + model, 'network convergence system', cert,
                  [('Cisco', 'brand'), ('CISCO SYSTEMS INC', 'manufacturer'), ('CISCO SYSTEMS INC', 'certificate_applicant'),
                   ('CELESTICA (THAILAND) LIMITED', 'listed_factory'), ('FABRINET CO LTD', 'listed_factory')],
                  [('cb_certificate', cert['certificate_id'])], facts)
    c.finish(SOURCE, len(ready))
    return {'certificates': len(ready), 'model_assertions': sum(len(r['models']) for r in ready)}

"""Exact Snowcone declaration scopes and dated family return requirements."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency, read_pages
from .fcc_index import Tree

GUIDE_URL = 'https://d1.awsstatic.com/Digital%20Marketing/House/Hero/products/Snow/16%20Compliance%20Notice_Snowcone_EU_Italian_it-IT_locked.pdf'
FAMILY_URL = 'https://d1.awsstatic.com/English-Guide-for-Snowball-and-Snowcone_Final_Sep2022.pdf'
INDEX_URL = 'https://aws.amazon.com/snow/safety-and-compliance-guide/'
MANUFACTURER = 'Amazon Data Services, Inc.'
MODELS = {7: ('AF1', 'Data storage device'), 9: ('AF1 BD1', 'Dongle')}
RETURN_TEXT = ('Per AWS Service Terms, customers are not permitted to obtain title or maintain '
               'permanent possession of Snow products, and all products must be returned to AWS.')


def prepare(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('index_url') != INDEX_URL
            or data.get('guide_url') != GUIDE_URL or data.get('family_url') != FAMILY_URL):
        raise ValueError('Unexpected Snowcone bundle scope')
    captures = {}
    for key, count in [('guide', 10), ('family', 3)]:
        pdf = dependency(path.parent, data[key]['pdf'])
        if not pdf.startswith(b'%PDF-'): raise ValueError('Snow guide is not a PDF')
        pages = read_pages(dependency(path.parent, data[key]['text']), digest(pdf))
        if len(pages) != count: raise ValueError('Snow guide page count changed')
        captures[key] = (digest(pdf), [' '.join(p['text'].split()) for p in pages])
    sha, pages = captures['guide']
    if ('Informazioni sulla sicurezza e la conformità di AWS Snowcone' not in pages[0]
            or "l'apparecchiatura radio tipo AF1 è conforme" not in pages[0]):
        raise ValueError('Snowcone family-to-AF1 guide identity changed')
    ready = []
    for pn, (expected_model, expected_object) in MODELS.items():
        pattern = (r'1\. Products in scope of this Declaration of Conformity: (.*?) '
                   r'2\. Manufacturer Identification Name: (.*?) Address: .*? '
                   r'3\. This declaration is issued under the sole responsibility of the manufacturer '
                   r'4\. Object of the Declaration: (.*?) 5\. Brand: (.*?) 6\.')
        matches = re.findall(pattern, pages[pn-1])
        if matches != [(expected_model, MANUFACTURER, expected_object, 'AWS')]:
            raise ValueError('Snowcone declaration identity, company or object changed')
        ready.append({'model': expected_model, 'object': expected_object, 'manufacturer': MANUFACTURER,
                      'brand': 'AWS', 'page': pn, 'pdf_sha256': sha})
    root = Tree(dependency(path.parent, data['index']).decode()).root
    if [n.text() for n in root.walk('h1')] != ['AWS Snow Family Device Safety and Compliance Guide']:
        raise ValueError('Missing Snow family safety index')
    links = [n.text() for n in root.walk('a') if n.attrs.get('href') == FAMILY_URL]
    if links != ['EU / English']: raise ValueError('Snow family guide index link changed')
    family_sha, pages = captures['family']
    if ('AWS Snowball and Snowcone Safety and Compliance Information' not in pages[0]
            or 'September 2022' not in pages[2] or RETURN_TEXT not in pages[2]):
        raise ValueError('Dated Snow family return statement changed')
    return path, ready, family_sha


def import_bundle(c, path):
    path, ready, family_sha = prepare(path)
    sid = 'aws-snowcone-declarations'
    c.source(sid, GUIDE_URL, 'vendor', 'Factual declaration fields; original AWS document rights retained',
             'English declarations on pages 7 and 9 in the public Italian safety-guide PDF. '
             'AF1 and the unsplit printed scope AF1 BD1 are separate objects. Legal manufacturer '
             'and AWS brand only; no physical factory, current validity, FCC ID, deployment or '
             'model equivalence inferred. No signature or declaration date extracted.', path)
    for row in ready:
        c.add(sid, row['model'], GUIDE_URL + '#page=' + str(row['page']), 'Amazon Data Services', row['model'],
              'AWS Snowcone ' + row['model'] + ' (' + row['object'].lower() + ')', row['object'].lower(), row,
              [(MANUFACTURER, 'manufacturer'), ('AWS', 'brand')], [],
              [('product_family', 'AWS Snowcone'), ('source_page', str(row['page'])),
               ('pdf_sha256', row['pdf_sha256']), ('declaration_product_scope', row['model']),
               ('identity_scope', 'Complete printed declaration scope retained; no splitting, aliases or omitted variants inferred'),
               ('manufacturer_scope', 'Named legal manufacturer, not an independently verified physical factory')])
    c.finish(sid, len(ready))
    sid = 'aws-snow-family-return-context'
    c.source(sid, FAMILY_URL, 'vendor', 'Dated factual source context; original AWS document rights retained',
             'September 2022 family-guide return statement linked to AF1 through its Snowcone safety guide. '
             'Historical source context only; no determination about ownership, title, contractual rights '
             'or transferability of any particular surplus unit. Not extended to the separately named dongle.', path)
    c.add(sid, 'AF1', FAMILY_URL + '#page=3', 'Amazon Data Services', 'AF1',
          'AWS Snowcone AF1 (data storage device)', 'data storage device',
          {'source_month': '2022-09', 'page': 3, 'pdf_sha256': family_sha,
           'family': 'AWS Snowball and Snowcone', 'identity_source': GUIDE_URL + '#page=1'},
          [('AWS', 'documentation_vendor')], [],
          [('source_document_month', '2022-09'), ('source_page', '3'), ('pdf_sha256', family_sha),
           ('source_family', 'AWS Snowball and Snowcone'),
           ('return_requirement_context', 'The September 2022 guide says Snow products must be returned to AWS and customers do not acquire title or permanent possession.'),
           ('return_requirement_scope', 'Dated family-guide statement; no assessment of any particular auction unit or current contractual rights')])
    c.finish(sid, 1)
    return {'sources': 2, 'models': 2, 'model_assertions': 3}

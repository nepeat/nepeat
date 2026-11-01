"""AWS Panorama appliance identity, distinct company roles and service closure."""
import json
from pathlib import Path

from .catalog import digest
from .documents import dependency, read_pages
from .fcc_index import Tree

PDF_URL = 'https://d1.awsstatic.com/product-marketing/panorama/1_MHT_Safety_ComplianceManual_ENGLISHv3.pdf'
NOTICE_URL = 'https://docs.aws.amazon.com/panorama/latest/dev/panorama-end-of-support.html'
MODEL = 'AMHT-1'
MANUFACTURER = 'Amazon Data Services, Inc.'
FCC_PARTY = 'Amazon Development Center US, Inc.'
DEVICE_NAME = 'AWS Panorama Appliance (Hub)'
INTRO = ('After careful consideration, we decided to end support for the AWS Panorama, effective May 31, 2026. '
         'AWS Panorama will no longer accept new customers beginning May 20, 2025. '
         'As an existing customer with an account signed up for the service before May 20, 2025, '
         'you can continue to use AWS Panorama features. After May 31, 2026, you will no longer be able to use AWS Panorama.')
OFFLINE = ('No. The Panorama device and applications are dependent on connectivity to the Panorama cloud service. '
           'Once that service is discontinued on May 31, 2026, neither the Panorama application nor the Panorama device will continue to function.')
PLATFORM = 'The existing AWS Panorama appliance is based on the Nvidia Jetson Xavier platform.'


def prepare(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('pdf_url') != PDF_URL
            or data.get('notice_url') != NOTICE_URL):
        raise ValueError('Unexpected Panorama bundle scope')
    pdf = dependency(path.parent, data['pdf'])
    if not pdf.startswith(b'%PDF-'):
        raise ValueError('Panorama safety guide is not a PDF')
    pages = read_pages(dependency(path.parent, data['text']), digest(pdf))
    if len(pages) != 11:
        raise ValueError('Panorama safety guide page count changed')
    texts = {n: ' '.join(pages[n-1]['text'].split()) for n in (3, 10)}
    identity = ('Device Name: ' + DEVICE_NAME + ' Model: ' + MODEL)
    responsible = 'The party responsible for FCC compliance is ' + FCC_PARTY + ', 410 Terry Ave North'
    if identity not in texts[3] or responsible not in texts[3]:
        raise ValueError('Panorama device identity or FCC responsible party changed')
    declaration = ('Declaration of Conformity 1. Product in scope of this Declaration of Conformity: '
                   'Appliance, model: AMHT-1 2. Manufacturer: Amazon Data Services, Inc. Address: '
                   '410 Terry Avenue North, Seattle, Washington, 98109, United States of America '
                   '3. This Declaration is issued under the sole responsibility of the manufacturer. '
                   '4. Object of the Declaration: Appliance, Model: AMHT-1 Brand: AWS')
    if declaration not in texts[10] or 'CC SVC 00233019 2022 TR' not in texts[10]:
        raise ValueError('Panorama declaration model, manufacturer or brand changed')
    root = Tree(dependency(path.parent, data['notice']).decode('utf-8')).root
    if [n.text() for n in root.walk('h1')] != ['AWS Panorama end of support']:
        raise ValueError('Missing Panorama end-of-support heading')
    paragraphs = [n.text() for n in root.walk('p')]
    if paragraphs.count(INTRO) != 1 or paragraphs.count(OFFLINE) != 1:
        raise ValueError('Panorama service dates or offline dependency notice changed')
    platforms = [s for s in paragraphs if s.startswith(PLATFORM)]
    if len(platforms) != 1:
        raise ValueError('Panorama appliance platform statement changed')
    return path, digest(pdf), {'intro': INTRO, 'offline': OFFLINE, 'platform': platforms[0]}


def import_bundle(c, path):
    path, pdf_sha, notice = prepare(path)  # All dependencies checked before source replacement.
    sid = 'aws-panorama-safety'
    c.source(sid, PDF_URL, 'vendor', 'Factual identifiers and references; AWS document rights retained',
             'Reviewed embedded text on pages 3 and 10 of the original 11-page AWS safety PDF. '
             'UK declaration names the manufacturer and brand; FCC compliance names a different responsible party. '
             'Image-only declaration pages are retained in the PDF but not parsed. No inferred FCC ID, '
             'physical factory, current certification validity or hyperscaler deployment.', path)
    common = [('product_family', 'AWS Panorama'), ('pdf_sha256', pdf_sha),
              ('identity_namespace', 'Amazon Data Services legal manufacturer; AWS is the declared brand')]
    c.add(sid, 'uk-declaration', PDF_URL + '#page=10', 'Amazon Data Services', MODEL,
          DEVICE_NAME + ' ' + MODEL, 'computer vision appliance',
          {'model': MODEL, 'page': 10, 'document': 'CC SVC 00233019 2022 TR',
           'manufacturer': MANUFACTURER, 'brand': 'AWS', 'pdf_sha256': pdf_sha},
          [(MANUFACTURER, 'manufacturer'), ('AWS', 'brand')],
          [('declaration_document', 'CC SVC 00233019 2022 TR')],
          common + [('source_page', '10'), ('manufacturer_scope', 'Named legal manufacturer in UK declaration; not physical factory evidence')])
    c.add(sid, 'fcc-responsible-party', PDF_URL + '#page=3', 'Amazon Data Services', MODEL,
          DEVICE_NAME + ' ' + MODEL, 'computer vision appliance',
          {'model': MODEL, 'page': 3, 'device_name': DEVICE_NAME,
           'fcc_responsible_party': FCC_PARTY, 'pdf_sha256': pdf_sha},
          [(FCC_PARTY, 'fcc_responsible_party')], [],
          common + [('source_page', '3'), ('fcc_identifier_scope', 'Guide refers to the FCC ID on the product; no ID is printed in this identity block')])
    c.finish(sid, 2)
    sid = 'aws-panorama-end-of-support'
    c.source(sid, NOTICE_URL, 'vendor', 'Factual dated notice and references; AWS document rights retained',
             'Dated Panorama service closure, cloud dependency and appliance platform statements. '
             'Linked to AMHT-1 using the device name on safety-guide page 3. No assertion about '
             'unlocking, alternative firmware, repurposing, transfer rights or unit provenance.', path)
    c.add(sid, MODEL, NOTICE_URL, 'Amazon Data Services', MODEL,
          DEVICE_NAME + ' ' + MODEL, 'computer vision appliance',
          dict(notice, identity_source=PDF_URL + '#page=3'), [('AWS', 'documentation_vendor')], [],
          [('product_family', 'AWS Panorama'), ('identity_source', PDF_URL + '#page=3'),
           ('service_new_customer_end_date', '2025-05-20'), ('service_end_date', '2026-05-31'),
           ('service_notice', notice['intro']), ('service_cloud_dependency', notice['offline']),
           ('compute_platform_description', notice['platform']),
           ('repurposing_scope', 'Alternative firmware, boot access and operation after repurposing have not been established')])
    c.finish(sid, 1)
    return {'sources': 2, 'models': 1, 'model_assertions': 3}

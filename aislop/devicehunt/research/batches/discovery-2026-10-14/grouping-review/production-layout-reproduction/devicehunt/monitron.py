"""Reviewed AWS Monitron identities, company roles and dated service notice."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency, read_pages
from .fcc_index import Tree

INDEX_URL = 'https://aws.amazon.com/monitron/terms/safety-and-compliance-information/'
HISTORY_URL = 'https://docs.aws.amazon.com/Monitron/latest/user-guide/doc-history.html'
BASE = 'https://d1.awsstatic.com/legal/Amazon%20Monitron/'
GUIDES = {
    'sensor': (BASE + 'TE1Ax0z-SM-monitron-sensor-safety-guide_v0.2.pdf',
               ['TE1A001', 'TE1A105'], 2, 'manufacturer', 'condition monitoring sensor'),
    'ex-sensor': (BASE + 'TE1Ax9z-SM-monitron-sensor-ex-safety-guide.pdf',
                  ['TE1A195'], 1, 'manufacturer', 'condition monitoring sensor'),
    'wifi-eu': (BASE + 'new-safety-guides/Amazon%20Monitron%20Gateway%20TG1A001%20Safety-Guide-EU-UK.pdf',
                ['TG1A001'], 2, 'compliance_declarant', 'Wi-Fi gateway'),
    'ethernet-eu': (BASE + 'new-safety-guides/amazon-monitron-gateway-tg1a-002-safety-guide-eu-Een.pdf',
                    ['TG1A002'], 2, 'compliance_declarant', 'Ethernet gateway'),
}
NOTICE = ('Amazon Monitron is no longer open to new customers. '
          'Existing customers can continue to use the service as normal.')


def plain(s):
    return ' '.join(s.split())


def prepare(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('index_url') != INDEX_URL
            or data.get('history_url') != HISTORY_URL or set(data.get('guides', {})) != set(GUIDES)):
        raise ValueError('Unexpected Monitron bundle scope')
    root = Tree(dependency(path.parent, data['index']).decode()).root
    if 'Amazon Monitron Safety Guide' not in [n.text() for n in root.walk('h1')]:
        raise ValueError('Missing Monitron safety index heading')
    ready = []
    for name, (url, models, page_number, role, kind) in GUIDES.items():
        links = [a for a in root.walk('a') if a.attrs.get('href') == url]
        if len(links) != 1:
            raise ValueError('Missing or duplicate Monitron guide link: ' + name)
        label = links[0].text()
        if name in ('sensor', 'ex-sensor'):
            expected = ', '.join(models) + (' Ex' if name == 'ex-sensor' else '') + ' Sensor Safety Manual'
            if label != expected:
                raise ValueError('Sensor guide model link changed')
        else:
            heading = 'Amazon Monitron ' + kind.replace('gateway', 'Gateway') + ' (' + models[0] + ') Safety and Compliance Information:'
            if label != 'EU / English' or heading not in root.text():
                raise ValueError('Gateway index model heading changed')
        entry = data['guides'][name]
        if entry.get('url') != url:
            raise ValueError('Unexpected guide URL')
        pdf = dependency(path.parent, entry['pdf'])
        if not pdf.startswith(b'%PDF-'):
            raise ValueError('Monitron guide is not a PDF')
        pages = read_pages(dependency(path.parent, entry['text']), digest(pdf))
        if type(entry.get('page_count')) is not int or len(pages) != entry['page_count'] or len(pages) < page_number:
            raise ValueError('Monitron PDF page count changed')
        text = plain(pages[page_number - 1]['text'])
        if role == 'manufacturer':
            if not re.search(r'Amazon Monitron Manufacturer Amazon Data Services, Inc\., .*? Model ', text):
                raise ValueError('Missing explicit sensor manufacturer block')
            model_field = 'TE1Ax9z' if name == 'ex-sensor' else 'TE1A001, TE1A105'
            if ('Model ' + model_field + ' ') not in text:
                raise ValueError('Sensor PDF model field changed')
            if name == 'ex-sensor' and 'Note: x and z define the product variant configuration' not in text:
                raise ValueError('Missing configurable-family qualifier')
        else:
            statement = ('Hereby, Amazon Data Services, Inc. declares that the radio equipment type '
                         'Amazon Monitron Gateway ' + models[0] + ' is in compliance with Directive 2014/53/EU.')
            if statement.casefold() not in text.casefold():
                raise ValueError('Missing gateway model-specific declarant statement')
            model_field = models[0]
        identifiers = []
        if name == 'sensor':
            for field, value, prefix in [('fcc_id', '2AVPL-TE1A001', 'FCC ID: '), ('ic_id', '25869-TE1A001', 'IC: ')]:
                if prefix + value not in text:
                    raise ValueError('Sensor regulatory row changed')
                identifiers.append((field, value))
        ready.append(dict(name=name, url=url, models=models, role=role, kind=kind,
                          page=page_number, model_field=model_field, link_label=label,
                          identifiers=identifiers, pdf_sha256=digest(pdf)))
    history = Tree(dependency(path.parent, data['history']).decode()).root
    if 'Document history for the Amazon Monitron User Guide' not in [n.text() for n in history.walk('h1')]:
        raise ValueError('Missing Monitron document history heading')
    notices = []
    for row in history.walk('tr'):
        cells = [n.text() for n in row.children if getattr(n, 'tag', None) == 'td']
        if len(cells) == 3 and cells[2] == 'October 31, 2024' and cells[1].startswith(NOTICE):
            notices.append(cells)
    if len(notices) != 1:
        raise ValueError('Missing or ambiguous dated Monitron service notice')
    return path, data, ready, notices[0]


def import_bundle(c, path):
    path, data, ready, notice = prepare(path)  # Validate every dependency before replacing any source.
    count = 0
    for row in ready:
        sid = 'aws-monitron-' + row['name']
        c.source(sid, row['url'], 'vendor', 'Factual identifiers and references; AWS document rights retained',
                 'Reviewed identity page of the PDF linked by the cached AWS safety index. '
                 'Sensors name a manufacturer; gateways name only a compliance declarant. '
                 'TE1A195 mapping uses the exact index link label to a configurable-family guide. '
                 'AWS Monitron product identity is distinct from hyperscaler data-center deployment. '
                 'No family expansion, factory attribution, deployment or current certification validity inferred.', path)
        for model in row['models']:
            url = row['url'] + '#page=' + str(row['page'])
            facts = [('product_family', 'Amazon Monitron'), ('device_type', row['kind']),
                     ('source_model_field', row['model_field']), ('guide_index_url', INDEX_URL),
                     ('guide_link_label', row['link_label']), ('source_page', str(row['page'])),
                     ('pdf_sha256', row['pdf_sha256'])]
            if row['name'] == 'ex-sensor':
                facts.append(('identity_link_scope', 'TE1A195 is the exact AWS index label; PDF names configurable family TE1Ax9z. No other variants inferred.'))
            c.add(sid, model, url, 'Amazon Data Services', model, 'Amazon Monitron ' + model,
                  row['kind'], row, [('Amazon Data Services, Inc.', row['role']), ('AWS', 'documentation_vendor')],
                  row['identifiers'], facts,
                  [('AWS', 'documented_product',
                    'The cached AWS safety index and linked guide identify ' + model +
                    ' as an Amazon Monitron ' + row['kind'] +
                    '. This is an industrial monitoring product link, not data-center deployment '
                    'or proof of an auction unit\'s prior ownership or service eligibility.')])
            count += 1
        c.finish(sid, len(row['models']))
    sid = 'aws-monitron-service-history'
    c.source(sid, HISTORY_URL, 'vendor', 'Factual dated notice and source references; source rights retained',
             'October 31, 2024 service notice applied to models explicitly linked by the cached Monitron safety index. '
             'This is dated service context, not proof that an auction unit can enroll or has transferable warranty. '
             'No manufacturer or deployment relationship asserted by this notice.', path)
    for row in ready:
        for model in row['models']:
            c.add(sid, model, HISTORY_URL, 'Amazon Data Services', model, 'Amazon Monitron ' + model,
                  row['kind'], {'notice_cells': notice, 'identity_source': row['url'], 'index_url': INDEX_URL},
                  [('AWS', 'documentation_vendor')], (),
                  [('product_family', 'Amazon Monitron'), ('service_notice_date', '2024-10-31'),
                   ('service_new_customer_access', 'Closed to new customers according to the dated AWS notice'),
                   ('service_existing_customer_access', 'Notice says existing customers can continue normally'),
                   ('service_eligibility_scope', 'No assertion about enrollment, account transfer or warranty eligibility for a surplus unit')])
    c.finish(sid, count)
    return {'sources': 5, 'model_assertions': count * 2, 'models': count}

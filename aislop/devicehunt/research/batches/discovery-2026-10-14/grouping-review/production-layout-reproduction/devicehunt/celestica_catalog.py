"""Product cards in Celestica's reviewed eight-page 2025 platform catalog."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency, read_pages

SOURCE = 'celestica-2025-catalog'
URL = 'https://www.celestica.com/uploadedFiles/artifact/OSS_2025_Catalog.pdf'
MODEL = r'(?:DS|ES|EG|SC|SD|SE)\d{4}'
CARD = re.compile(MODEL + r'(?: / ' + MODEL + r')*')


def product_cards(pages):
    if (len(pages) != 8 or 'Celestica' not in pages[0]['text']
            or '2025' not in pages[7]['text'] or 'Copyright Celestica' not in pages[7]['text']):
        raise ValueError('Unexpected Celestica catalog edition or page count')
    cards, seen = [], set()
    for number, expected in [(4, 10), (5, 7)]:
        lines = [line.strip() for line in pages[number-1]['text'].splitlines() if line.strip()]
        page_cards = []
        for i, line in enumerate(lines):
            if not CARD.fullmatch(line):
                continue
            if i + 2 >= len(lines):
                raise ValueError('Truncated product card')
            description = ' '.join(lines[i+1:i+3])
            models = line.split(' / ')
            if (any(m in seen for m in models) or len(set(models)) != len(models)
                    or not re.match(r'^[124]U \d', description)
                    or any(CARD.fullmatch(v) for v in lines[i+1:i+3])):
                raise ValueError('Duplicated or malformed product card')
            if 'Storage Controller' in description:
                kind = 'storage controller'
            elif 'Expansion System' in description:
                kind = 'storage expansion system'
            elif 'Gateway' in description:
                kind = 'network gateway'
            elif 'Switch' in description:
                kind = 'network switch'
            else:
                raise ValueError('Unrecognized product card type')
            expected_kind = ('network switch' if models[0].startswith(('DS', 'ES')) else
                             'network gateway' if models[0].startswith('EG') else
                             'storage controller' if models[0].startswith('SC') else 'storage expansion system')
            if kind != expected_kind:
                raise ValueError('Model and description category mismatch')
            seen.update(models)
            page_cards.append({'heading': line, 'models': models, 'description': description,
                               'page': number, 'kind': kind})
        if len(page_cards) != expected:
            raise ValueError('Product-card count changed on page ' + str(number))
        cards += page_cards
    if len(seen) != 19:
        raise ValueError('Grouped product-model count changed')
    return cards


def import_bundle(c, path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or data.get('url') != URL:
        raise ValueError('Unexpected Celestica catalog bundle')
    pdf = dependency(path.parent, data['pdf'])
    if not pdf.startswith(b'%PDF-'):
        raise ValueError('Celestica catalog is not a PDF')
    sha = digest(pdf)
    pages = read_pages(dependency(path.parent, data['text']), sha)
    cards = product_cards(pages)
    c.source(SOURCE, URL, 'vendor', 'Factual product-card extracts; Celestica retains original document rights',
             'All 17 product cards (19 named models) on pages 4-5 of the reviewed eight-page 2025 catalog. '
             'Grouped model names expand into separate identities sharing the original description; no aliases inferred. '
             'Brand relationship only, not physical factory or model-specific hyperscaler deployment. '
             'Historical descriptions do not establish current availability, included drives, support entitlement or all-time inventory. '
             'PDF reading order and card boundaries were visually checked; embedded text is retained including extraction artifacts.', path)
    count = 0
    for card in cards:
        for model in card['models']:
            text = card['description']
            facts = [('source_year', '2025'), ('source_page', str(card['page'])),
                     ('source_model_heading', card['heading']), ('product_description', text),
                     ('physical_description', text.split()[0] + ' form factor stated in 2025 catalog'),
                     ('form_factor', text.split()[0]), ('pdf_sha256', sha)]
            if len(card['models']) > 1:
                facts.append(('description_scope', 'Shared product-card description; alternatives are not allocated to individual models'))
            if 'Controller Controller' in text:
                facts.append(('extraction_note', 'Embedded PDF text duplicates Controller; visually the product caption shows it once'))
            c.add(SOURCE, model, URL + '#page=' + str(card['page']), 'Celestica', model,
                  'Celestica ' + model, card['kind'], card, [('Celestica', 'brand')], (), facts)
            count += 1
    c.finish(SOURCE, len(cards))
    return {'product_cards': len(cards), 'model_assertions': count, 'pages': len(pages)}

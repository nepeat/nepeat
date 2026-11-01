"""Non-CS model identifiers in a reviewed public UL search-index capture."""
import datetime as dt
import json
import re
from pathlib import Path
from urllib.parse import urlsplit

from .ul import DOCUMENT, HOLDER

SOURCE = 'ul-celestica-platforms'
SCOPE = 'celestica-non-cs-identifiers-v1'
# Whitespace-qualified names precede ordinary identifiers. Case is preserved.
MODEL = re.compile(r'(?<![A-Za-z0-9_.-])(?:DS4000-[FR] 02|SRU 0101|Voyager 1\.1|'
                   r'[A-Za-z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)*)(?![A-Za-z0-9_.-])')
NAMED = {'Voyager', 'BRIXIA', 'CONAE', 'CONAE-DC'}


def extract(text):
    models = {}
    for match in MODEL.finditer(text):
        model = match[0]
        if model.startswith('CS') or re.search(r'x{2,}', model, re.I) or not (any(c.isdigit() for c in model) or model in NAMED):
            continue
        tail = text[match.end():]
        # Keep the literal trademark note in the capture; it is not a model
        # suffix and does not authorize inferring a second brand or alias.
        tail = re.sub(r'^\(for trademark HPE\)', '', tail)
        if re.match(r'\s*(?:\(|\b\d+\b|\b(?:AC|DC|v\d+|US\d+|\d+V)\b)', tail):
            raise ValueError('Unreviewed qualifier after UL Celestica model: ' + model)
        models.setdefault(model, []).append([match.start(), match.end()])
    return models


def import_capture(c, path):
    data = json.loads(Path(path).read_text())
    url = urlsplit(data.get('source_url', ''))
    if (data.get('schema_version') != 1 or data.get('capture_format') != 'web_search_models_section'
            or data.get('document') != DOCUMENT or data.get('listing_holder') != HOLDER
            or data.get('section') != 'Models' or data.get('extraction_scope') != SCOPE
            or url.scheme != 'https' or url.netloc != 'productiq.ulprospector.com'
            or url.path != '/en/profile/2858121/azot.e362364'):
        raise ValueError('Unrecognized UL Celestica platform capture')
    try:
        dt.datetime.strptime(data['retrieved_at'], '%Y-%m-%d %H:%M:%S UTC')
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError('Missing UL retrieval timestamp') from exc
    text = data.get('model_section_text')
    if not isinstance(text, str) or not text.strip() or len(text) > 100_000:
        raise ValueError('Missing or oversized UL model section')
    models = extract(text)
    if not models or sorted(models) != data.get('models'):
        raise ValueError('UL reviewed extraction differs from cached models')
    c.source(SOURCE, data['source_url'], 'certification_body',
             'UL factual identifiers and listing-holder fields; source rights retained',
             'Selected non-CS identifiers from the public abridged Models section exposed by web search. '
             'Direct page opening returned 403; original HTML is not cached. Search-index freshness '
             'and complete listing coverage are unverified. CS identifiers remain in a separate capture. '
             'IcePack Canister is left unparsed pending review of grouping. Whitespace qualifiers are '
             'preserved. Celestica is the listing-holder identity namespace, not an asserted brand, '
             'factory or physical manufacturer. No cross-brand equivalence, deployment or current '
             'certification validity is inferred; entries may describe products or components.', path)
    for model, spans in sorted(models.items()):
        c.add(SOURCE, model, data['source_url'], 'Celestica', model, model, 'listed ICT equipment',
              {'model': model, 'document': DOCUMENT, 'listing_holder': HOLDER,
               'model_section_spans': spans, 'capture_format': data['capture_format'],
               'extraction_scope': SCOPE, 'retrieved_at': data['retrieved_at']},
              [(HOLDER, 'listing_holder')], [('ul_document', DOCUMENT)],
              [('listing_holder_name', HOLDER), ('capture_format', data['capture_format']),
               ('retrieved_at', data['retrieved_at']),
               ('identity_namespace', 'Celestica listing holder; product brand not established by this record')])
    c.finish(SOURCE, len(models))
    return {'model_assertions': len(models)}

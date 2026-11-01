"""Import a bounded factual extract from UL's public abridged listing."""
import datetime as dt
import json
import re
from pathlib import Path
from urllib.parse import urlsplit


SOURCE = 'ul-celestica-cs'
DOCUMENT = 'AZOT.E362364'
HOLDER = 'CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO LTD'
MODEL_PATTERN = r'CS\d{4}-\d{2}X-(?:UN|DC)-\d{2}'


def import_capture(c, path):
    data = json.loads(Path(path).read_text())
    url = data.get('source_url', '')
    parsed = urlsplit(url)
    if (data.get('schema_version') != 1 or data.get('capture_format') != 'web_tool_model_extract'
            or data.get('document') != DOCUMENT or data.get('listing_holder') != HOLDER
            or data.get('section') != 'Models' or parsed.scheme != 'https'
            or parsed.netloc != 'productiq.ulprospector.com'
            or parsed.path not in ('/en/profile/2858121/azot.e362364', '/zh-cn/profile/2858121/azot.e362364')):
        raise ValueError('Unrecognized UL public-listing capture')
    if data.get('extraction_pattern') != r'\b' + MODEL_PATTERN + r'\b':
        raise ValueError('UL capture extraction scope changed')
    try:
        dt.datetime.strptime(data['retrieved_at'], '%Y-%m-%d %H:%M:%S UTC')
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError('UL capture requires its retrieval timestamp') from exc
    models = data.get('models')
    if (not isinstance(models, list) or not models
            or any(not isinstance(m, str) or not re.fullmatch(MODEL_PATTERN, m) for m in models)
            or len(models) != len(set(models))):
        raise ValueError('Missing, duplicate, or out-of-scope UL model identifiers')
    c.source(SOURCE, url, 'certification_body', 'UL factual model identifiers; source rights retained',
             'Automatically extracted complete CS-series identifiers from the public Models section via web.run. '
             'Cache is a structured factual extract, not original HTML or a full listing. Direct HTML retrieval '
             'returned HTTP 403. Listing-holder relationship only; no physical OEM, AWS deployment, '
             'current certification validity, or exhaustive product-coverage assertion.', path)
    for model in models:
        c.add(SOURCE, model, url, 'Celestica', model, 'Celestica ' + model,
              'listed ICT equipment', {'model': model, 'document': DOCUMENT, 'listing_holder': HOLDER,
              'capture_format': data['capture_format'], 'retrieved_at': data['retrieved_at']},
              [('Celestica', 'listing_holder')], [('ul_document', DOCUMENT)],
              [('listing_holder_name', HOLDER), ('capture_format', data['capture_format']),
               ('retrieved_at', data['retrieved_at']),
               ('source_scope', 'Selected CS-series models from a public abridged listing; may include products or components')])
    c.finish(SOURCE, len(models))
    return len(models)

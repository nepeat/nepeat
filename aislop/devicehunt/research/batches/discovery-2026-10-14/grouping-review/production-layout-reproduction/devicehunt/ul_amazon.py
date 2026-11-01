"""Reviewed exact names from the public abridged Amazon UL listing."""
import datetime as dt
import json
import re
from pathlib import Path
from urllib.parse import urlsplit

SOURCE = 'ul-amazon-platforms'
DOCUMENT = 'AZOT7.E515139'
HOLDER = 'Amazon Data Services, Inc.'
SCOPE = 'amazon-reviewed-model-entries-v1'
# These are literal listing templates, never expanded into product identities.
PATTERNS = frozenset({
    'TE1AXYZ', 'Frederic_Mistral-xxx', 'AmbroseXXF*****',
    'Charles_Richet-xxx', 'Ambrose*****', 'Camillo_Golgi-xxx',
    'Rudyard_Kipling-xxx', 'CMWA#6400-XYZ', 'Tiger_Woods-xxx',
    'Arnold_Palmer-XXX',
})
SPACED_NAMES = {'BARRELEYE MGMT', 'BARRELEYE SWITCH', 'BARRELEYE COMPUTE'}


def extract(entries):
    if not isinstance(entries, list) or not entries or len(entries) > 1000:
        raise ValueError('Missing or oversized reviewed UL entry list')
    models, patterns = {}, {}
    for index, entry in enumerate(entries):
        if not isinstance(entry, str) or not entry or entry != entry.strip():
            raise ValueError('Invalid reviewed UL entry')
        if entry in PATTERNS:
            patterns.setdefault(entry, []).append(index)
            continue
        if (re.search(r'[*#]|xxx|XYZ|XXF', entry, re.I)
                or not (re.fullmatch(r'[A-Za-z][A-Za-z0-9_-]*', entry)
                        or entry in SPACED_NAMES)):
            raise ValueError('Unreviewed UL name, grouping or placeholder: ' + entry)
        models.setdefault(entry, []).append(index)
    if not models:
        raise ValueError('No exact UL names in the reviewed capture')
    return models, patterns


def import_capture(c, path):
    data = json.loads(Path(path).read_text())
    url = urlsplit(data.get('source_url', ''))
    if (data.get('schema_version') != 1
            or data.get('capture_format') != 'web_search_reviewed_model_entries'
            or data.get('document') != DOCUMENT or data.get('listing_holder') != HOLDER
            or data.get('section') != 'Models' or data.get('extraction_scope') != SCOPE
            or url.scheme != 'https' or url.netloc != 'productiq.ulprospector.com'
            or url.path != '/en/profile/2859718/azot7.e515139'):
        raise ValueError('Unrecognized UL Amazon capture')
    try:
        dt.datetime.strptime(data['retrieved_at'], '%Y-%m-%d %H:%M:%S UTC')
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError('Missing UL retrieval timestamp') from exc
    models, patterns = extract(data.get('model_section_entries'))
    if sorted(models) != data.get('models') or sorted(patterns) != data.get('unexpanded_patterns'):
        raise ValueError('Reviewed UL extraction differs from cached lists')
    c.source(SOURCE, data['source_url'], 'certification_body',
             'UL factual identifiers and listing-holder fields; source rights retained',
             'Reviewed exact names from the abridged public Models section returned by web search. '
             'Direct opening returned HTTP 403; this is a factual search extract, not original HTML. '
             'Search-index freshness, original formatting and complete listing coverage are unverified. '
             'Repeated names retain entry positions; placeholder patterns remain only in the capture. '
             'Names may identify products or components. Amazon Data Services is the listing-holder '
             'identity namespace, not an asserted brand, manufacturer, operator or deployment. '
             'No cross-brand equivalence or current certification validity is inferred.', path)
    for model, positions in sorted(models.items()):
        c.add(SOURCE, model, data['source_url'], 'Amazon Data Services', model, model,
              'listed ICT equipment',
              {'model': model, 'document': DOCUMENT, 'listing_holder': HOLDER,
               'model_section_entry_positions': positions, 'capture_format': data['capture_format'],
               'extraction_scope': SCOPE, 'retrieved_at': data['retrieved_at']},
              [(HOLDER, 'listing_holder')], [('ul_document', DOCUMENT)],
              [('listing_holder_name', HOLDER), ('capture_format', data['capture_format']),
               ('retrieved_at', data['retrieved_at']),
               ('identity_namespace', 'Amazon Data Services listing holder; product brand unestablished'),
               ('source_scope', 'Selected names from an abridged listing; products or components')])
    c.finish(SOURCE, len(models))
    return {'model_assertions': len(models), 'unexpanded_patterns': len(patterns)}

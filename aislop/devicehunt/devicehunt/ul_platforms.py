"""Extract bounded platform identifiers from a cached public UL Models section."""
import datetime as dt
import json
import re
from pathlib import Path
from urllib.parse import urlsplit

SOURCE = 'ul-accton-platforms'
DOCUMENT = 'AZOT7.E141136'
HOLDER = 'ACCTON TECHNOLOGY CORPORATION'
SCOPE = 'accton-platform-identifiers-v1'

# The original capture pins this first extraction, which remains unchanged so
# the captured reviewed list can still be checked independently of expansions.
# The boundary prevents extracting a shorter model from a longer order code.
MODEL = re.compile(
    r'(?<![A-Za-z0-9-])(?:'
    r'(?:AS|AZ|AIS|ECS|CSX|AMX)[A-Za-z0-9]*\d[A-Za-z0-9]*(?:-[A-Za-z0-9]+)*'
    r'|DMC(?:-[A-Za-z0-9]+)+'
    r'|Wedge\d+[A-Za-z]*-[A-Za-z0-9]+'
    r'|Minipack(?:-AS\d+)?|Barefoot DNA X1032C|ORV3 HSM'
    r')(?: (?:AC|DC|48V|v2|US2|\(16-Core\)))?(?![A-Za-z0-9-])')

# Additional reviewed commercial naming families. Brand and cross-brand
# equivalence cannot be inferred from a prefix; these remain holder records.
# Underscores, dots and hyphens must not allow a partial identifier to match.
COMMERCIAL_MODEL = re.compile(
    r'(?<![A-Za-z0-9_.-])(?:'
    r'(?:QFX|EX|ACX|JNP|SRX)\d+[A-Za-z0-9]*(?:-[A-Za-z0-9]+)*'
    r'|JCP-JNET2-ID)(?![A-Za-z0-9_.-])')
COMMERCIAL_SCOPE = 'qfx-ex-acx-jnp-srx-jcp-identifiers-v1'

# Additional complete names from the cached section. Numeric-only names,
# ambiguous 1FINITY groupings and wildcard templates are deliberately excluded.
EXTENDED_MODEL = re.compile(
    r'(?<![A-Za-z0-9_.-])(?:'
    r'(?:JL\d{3}|R[89][A-Z]\d{2}|S\d[A-Z]\d{2,3})[A-Z]{1,2}'
    r'|(?:NSG|SAF|SAU|SDX|SDW|NE|EWS|EAPM|EAP|UHD|AG|ES|CSG|RM|HLS|MLTG|XE|WIS|VED|CSP|DXS|AX|EC|ARCN|SC|X7|IXTP|IXBP|IXPR|REG)(?=\d|-)[A-Za-z0-9]*(?:-[A-Za-z0-9]+)*'
    r'|(?:FortiSwitch-AX|FS-AX)[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*'
    r'|GigaVUE-TA\d+E'
    r'|Vision E(?:100|40) (?:AC|DC)|Vision Edge 1S|Vision E50'
    r'|NSE 3000|Nile NSW200|PFS 5120'
    r')(?![A-Za-z0-9_.-])')
EXTENDED_SCOPE = 'additional-complete-commercial-identifiers-v1'
RMN_ANNOTATION = re.compile(r'\s*(\(RMN(?: ID:\s*|)RSVLC-\s*\d{4}\))')


def extract_extended(text):
    models = {}
    annotations = {}
    for match in EXTENDED_MODEL.finditer(text):
        model = match[0]
        if not any(char.isdigit() for char in model) or re.search(r'x{2,}|xy', model, re.I):
            continue
        tail = text[match.end():]
        annotation = RMN_ANNOTATION.match(tail)
        if annotation:
            # Preserve adjacency, not an inferred product/RMN equivalence.
            annotations.setdefault(model, set()).add(annotation[1])
            tail = tail[annotation.end():]
        if re.match(r'\s*(?:\([^)]*\)|\bv\d+|\bUS\d+|\b\d+V|\bAC\b|\bDC\b)', tail):
            raise ValueError('Unreviewed whitespace qualifier after UL extended model: ' + model)
        models.setdefault(model, []).append([match.start(), match.end()])
    return models, annotations


def extract_commercial(text):
    models = {}
    for match in COMMERCIAL_MODEL.finditer(text):
        model = match[0]
        if re.search(r'x{2,}', model, re.I):
            continue
        if re.match(r'\s+(?:\([^)]*\)|v\d+|US\d+|\d+V|AC\b|DC\b)', text[match.end():]):
            raise ValueError('Unreviewed whitespace qualifier after UL commercial model: ' + model)
        models.setdefault(model, []).append([match.start(), match.end()])
    return models


def extract(text):
    models = {}
    for match in MODEL.finditer(text):
        # An unknown suffix attached with whitespace must not silently become
        # a base model. Known suffixes above are kept in full, not aliases.
        model = match[0]
        if re.search(r'x{2,}', model, re.I):
            continue
        tail = text[match.end():]
        if re.match(r'\s+(?:\([^)]*\)|v\d+|US\d+|\d+V|AC\b|DC\b)', tail):
            raise ValueError('Unreviewed whitespace qualifier after UL model: ' + model)
        models.setdefault(model, []).append([match.start(), match.end()])
    return models


def validate(path):
    data = json.loads(Path(path).read_text())
    url = urlsplit(data.get('source_url', ''))
    if (data.get('schema_version') != 1 or data.get('capture_format') != 'web_tool_models_section'
            or data.get('document') != DOCUMENT or data.get('listing_holder') != HOLDER
            or data.get('section') != 'Models' or data.get('extraction_scope') != SCOPE
            or url.scheme != 'https' or url.netloc != 'productiq.ulprospector.com'
            or url.path != '/en/profile/2858881/azot7.e141136'):
        raise ValueError('Unrecognized UL Accton capture')
    try:
        dt.datetime.strptime(data['retrieved_at'], '%Y-%m-%d %H:%M:%S UTC')
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError('Missing UL retrieval timestamp') from exc
    text = data.get('model_section_text')
    if not isinstance(text, str) or not text.strip() or len(text) > 100_000:
        raise ValueError('Missing or oversized UL factual model section')
    models = extract(text)
    if not models or sorted(models) != data.get('models'):
        raise ValueError('UL reviewed extraction differs from the cached model list')
    return data, models


def import_capture(c, path):
    data, models = validate(path)
    commercial = extract_commercial(data['model_section_text'])
    extended, annotations = extract_extended(data['model_section_text'])
    # Rules must remain disjoint so a new rule cannot overwrite provenance.
    if set(extended) & (set(models) | set(commercial)):
        raise ValueError('UL extraction scopes overlap')
    models = {**models, **commercial, **extended}
    c.source(SOURCE, data['source_url'], 'certification_body',
             'UL factual identifiers and listing-holder fields; source rights retained',
             'Selected complete platform identifiers from the public abridged Models section, via web.run. '
             'The original HTML is not cached. The page explicitly excludes some models from public view. '
             'Reviewed AS/AZ/AIS/ECS/DMC/CSX/AMX/Wedge families and Minipack, Barefoot DNA X1032C, ORV3 HSM '
             'names, plus reviewed QFX/EX/ACX/JNP/SRX/JCP and additional complete commercial codes '
             'and named models. Adjacent RMN annotations are retained without inferred aliases. '
             'Numeric-only names, ambiguous 1FINITY groupings, unreviewed names and wildcard patterns '
             'remain unparsed. Accton is the listing-holder identity namespace, '
             'not an asserted product brand. Known whitespace '
             'qualifiers are preserved. Listing-holder association only: not physical manufacturer, '
             'cross-brand equivalence, hyperscaler deployment or current certification validity.', path)
    for model, spans in sorted(models.items()):
        c.add(SOURCE, model, data['source_url'], 'Accton', model, model, 'listed ICT equipment',
              {'model': model, 'document': DOCUMENT, 'listing_holder': HOLDER,
               'model_section_spans': spans, 'capture_format': data['capture_format'],
               'extraction_scope': EXTENDED_SCOPE if model in extended else COMMERCIAL_SCOPE if model in commercial else SCOPE,
               'adjacent_annotations': sorted(annotations.get(model, [])),
               'retrieved_at': data['retrieved_at']},
              [(HOLDER, 'listing_holder')], [('ul_document', DOCUMENT)],
              [('listing_holder_name', HOLDER), ('capture_format', data['capture_format']),
               ('retrieved_at', data['retrieved_at']),
               ('identity_namespace', 'Accton listing holder; product brand not established by this record'),
               ('source_scope', 'Selected identifiers from an abridged listing; may describe products or components')]
              + [('ul_adjacent_annotation', note) for note in sorted(annotations.get(model, []))])
    c.finish(SOURCE, len(models))
    return {'model_assertions': len(models)}

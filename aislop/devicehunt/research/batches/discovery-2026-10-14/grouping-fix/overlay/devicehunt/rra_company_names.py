"""Narrow, evidence-backed company spellings for an optional RRA capture.

These mappings never alter global aliases or split composite manufacturer cells.
The filing establishes reported corporate relationships at its reporting date,
not parent ownership at the approval date or a physical manufacturing factory.
"""
import datetime as dt
import hashlib
import json

from .catalog import normalize
from .documents import dependency

URL = 'https://celestica.gcs-web.com/node/17171/html'
METHOD = 'reviewed factual web-tool exhibit extract; not original HTML'
NAMES = {
    'Celestica Cayman Holdings 1 Limited', 'Celestica LLC',
    'Celestica (Thailand) Limited', 'Celestica (USA) Inc.',
    'Celestica Holdings Pte Limited',
}
REVIEWED = [
    ('Celestica LLC', 'Celestica LLC', 'exact legal name'),
    ('Celestica (Thailand) Ltd', 'Celestica (Thailand) Limited',
     'reviewed Ltd abbreviation of Limited'),
    ('Celestica Holdings Pte Ltd', 'Celestica Holdings Pte Limited',
     'reviewed Ltd abbreviation of Limited'),
    ('Celestica Holdings Pte Limited', 'Celestica Holdings Pte Limited', 'exact legal name'),
]

ACCTON_URL = 'https://www.accton.com/wp-content/uploads/2017/09/2013-Consolidated-Report.pdf'
ACCTON_METHOD = 'reviewed factual web-search extract; not original PDF'
ACCTON_NAMES = {
    'Accton Technology Corp. Pte Ltd (Singapore)', 'SMC Networks Inc.',
    'SMC Networks (Taiwan) Corporation', 'SMC Networks Spain, S.L.',
}


def load_accton(data, entry):
    if (data.get('schema_version') != 1 or data.get('source_url') != ACCTON_URL
            or data.get('capture_method') != ACCTON_METHOD
            or data.get('document') != '2013-Consolidated-Report.pdf'
            or data.get('reporting_year') != '2013'
            or data.get('reported_parent') != 'Edgecore Networks'
            or data.get('relationship') != 'wholly owned subsidiaries'
            or not isinstance(data.get('subsidiaries'), list)
            or any(not isinstance(name, str) for name in data['subsidiaries'])
            or len(data['subsidiaries']) != len(ACCTON_NAMES)
            or set(data['subsidiaries']) != ACCTON_NAMES):
        raise ValueError('Unrecognized reviewed Accton company extract')
    dt.date.fromisoformat(data['reviewed_on'])
    # The report adds a geographic qualifier; the RRA whole-cell name does not.
    # Target is the Accton-named manufacturer namespace, not a parent-company
    # manufacturing claim. No SMC or broader subsidiary aliases are enabled.
    return {normalize('Accton Technology Corp. Pte Ltd'): dict(
        target='Accton', legal_name='Accton Technology Corp. Pte Ltd',
        mapping_basis='reviewed whole legal name; report adds Singapore geographic qualifier',
        reported_name='Accton Technology Corp. Pte Ltd (Singapore)',
        reported_parent='Edgecore Networks', reported_relationship='wholly owned subsidiary',
        source_url=ACCTON_URL, sha256=entry['sha256'], capture_method=ACCTON_METHOD,
        reporting_year='2013',
        scope='Accton-named manufacturer namespace; parent ownership at approval date unverified')}


COMPOSITE_KIND = 'rra-composite-manufacturer-cells'
COMPOSITE_ROLES = frozenset({'registry_manufacturer'})
BASE_KIND = 'celestica-historical-company-names'


def _composite_wrapper(root, entry):
    """The composite-cell wrapper, or None when `entry` is any other company evidence."""
    data = json.loads(dependency(root, entry))
    return data if data.get('kind') == COMPOSITE_KIND else None


def load(root, entry):
    if entry is None:
        return {}
    data = json.loads(dependency(root, entry))
    if data.get('kind') == COMPOSITE_KIND:
        # The wrapper adds certificate-scoped entries only; whole-company names come from its
        # base, which must be the historical Celestica bundle (never another wrapper).
        base = data.get('base')
        if (not isinstance(base, dict) or set(base) != {'path', 'sha256'}
                or json.loads(dependency(root, base)).get('kind') != BASE_KIND):
            raise ValueError('Composite manufacturer cells require the historical Celestica base')
        return load(root, base)
    if data.get('kind') == 'celestica-historical-company-names':
        from . import rra_celestica_historical
        return rra_celestica_historical.load(root, data)
    if data.get('source_url') == ACCTON_URL:
        return load_accton(data, entry)
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or data.get('capture_method') != METHOD
            or data.get('exhibit') != '21.1'
            or data.get('heading') != 'Subsidiaries of the Registrant'
            or data.get('reporting_date') != '2025-12-31'
            or data.get('registrant') != 'Celestica Inc.'
            or not isinstance(data.get('subsidiaries'), list)
            or any(not isinstance(name, str) for name in data['subsidiaries'])
            or len(data['subsidiaries']) != len(NAMES)
            or set(data['subsidiaries']) != NAMES):
        raise ValueError('Unrecognized reviewed Celestica subsidiary exhibit')
    dt.date.fromisoformat(data['reviewed_on'])
    return {normalize(spelling): dict(target='Celestica', legal_name=legal_name,
            mapping_basis=basis, source_url=URL, sha256=entry['sha256'],
            capture_method=METHOD, reporting_date=data['reporting_date'])
            for spelling, legal_name, basis in REVIEWED}


def composite_coverage(composites):
    if not composites:
        return ''
    return (' Exception to unsplit composite cells: ' + str(len(composites)) + ' reviewed certificate-scoped entr'
            + ('y' if len(composites) == 1 else 'ies') + ' (' + ', '.join(sorted(k[0] for k in composites)) +
            '): each keys one exact manufacturer cell (certificate plus cell text and hash, with its '
            'detail page hash) and applies to that certificate and cell only. A split part carries '
            'registry_manufacturer only where reviewed legal-name evidence for that name exists; every other '
            'part, and any trailing unlabelled parenthetical, is unmapped literal context with no role, '
            'target or affiliation. No factory, per-factory country or sole-'
            'manufacturer claim is made, and the same cell under another query gets no claim.')


def coverage(reviewed):
    if not reviewed:
        return ''
    if any('evidence_period' in v for v in reviewed.values()):
        return (' Additional whole-cell Celestica legal names depend on separately hashed reviewed '
                'factual extracts: the existing 2025 exhibit plus SEC exhibit 8.1 and an archived '
                'exhibit 21.1. The latter archive was available as a search extract only; its '
                'original PDF returned 403 and was not retried. Exact legal names and reviewed '
                'Ltd/Limited abbreviations retain per-record evidence. Reporting periods for '
                'the two historical excerpts are unverified; these establish historical legal-name '
                'associations, not current ownership, ownership at approval date or factory location. '
                'Composite manufacturer cells remain unsplit.')
    if {value['source_url'] for value in reviewed.values()} == {ACCTON_URL}:
        return (' Additional whole-cell Accton Singapore spellings depend on a separately '
                'hashed factual search extract of the 2013 Accton consolidated report, '
                'not original PDF. The geographic qualifier interpretation and reported '
                'Edgecore parent are retained per record. Accton is the named manufacturer '
                'namespace; historical or current parent ownership, parent manufacturing '
                'and cross-company device equivalence are not inferred.')
    return (' Additional whole-cell company spellings depend on a separately hashed factual '
            'web-tool extract of Celestica 2025 Exhibit 21.1, not original filing HTML. '
            'Reviewed Ltd/Limited abbreviations are explicit company-name interpretations. '
            'The later filing does not establish parent ownership at historical approval dates.')


def load_composites(root, entry):
    """{(certificate, cell text sha256): reviewed entry} from a composite-cell wrapper, else {}.

    Exact-cell only: an entry's parts joined by its separator plus the unlabelled parenthetical
    must reproduce the cell text byte for byte. Nothing here splits a cell by pattern.
    """
    if entry is None:
        return {}
    data = _composite_wrapper(root, entry)
    if data is None:
        return {}
    reviewed = load(root, entry)
    if (data.get('schema_version') != 1 or not isinstance(data.get('entries'), list) or not data['entries']):
        raise ValueError('Unrecognized composite manufacturer cell evidence')
    dt.date.fromisoformat(data['reviewed_on'])
    result = {}
    for item in data['entries']:
        cell, parts, paren = item.get('cell'), item.get('parts'), item.get('unlabelled_parenthetical')
        if (not isinstance(cell, dict) or cell.get('field') != 'manufacturer' or not isinstance(cell.get('text'), str)
                or not isinstance(parts, list) or len(parts) < 2 or not isinstance(paren, dict)
                or paren.get('role') is not None or not isinstance(paren.get('text'), str)
                or item.get('separator') != ',' or not isinstance(item.get('certificate_number'), str)
                or not all(isinstance(v, str) for v in (item.get('detail_sha256'), item.get('query_maker')))):
            raise ValueError('Malformed composite manufacturer cell entry')
        text = cell['text']
        if hashlib.sha256(text.encode()).hexdigest() != cell.get('text_sha256'):
            raise ValueError('Composite cell text does not match its hash')
        if ','.join(p.get('text', '') for p in parts) + ' ' + paren['text'] != text:
            raise ValueError('Composite cell parts do not reproduce the exact cell')
        mapped = 0
        for part in parts:
            known = normalize(part.get('text', '')) in reviewed
            if part.get('role') is None:
                # Unmapped literal context: no role, no target, no affiliation; must not be mappable already.
                if part.get('target') is not None or part.get('name_evidence') != 'unmapped_context' or known:
                    raise ValueError('Composite cell context part is not unmapped: ' + str(part.get('text')))
                continue
            mapped += 1
            if (part['role'] not in COMPOSITE_ROLES or part.get('target') != 'Celestica'
                    or part.get('name_evidence') != 'base' or not known
                    or reviewed[normalize(part['text'])]['target'] != part['target']):
                raise ValueError('Composite cell part is not backed as declared: ' + str(part.get('text')))
        if not mapped:
            raise ValueError('Composite cell entry maps no part')
        key = (item['certificate_number'], cell['text_sha256'])
        if key in result:
            raise ValueError('Duplicate composite cell entry')
        result[key] = dict(item, parts=[dict(p) for p in parts], source_sha256=entry['sha256'],
                           reviewed_on=data['reviewed_on'])
    return result

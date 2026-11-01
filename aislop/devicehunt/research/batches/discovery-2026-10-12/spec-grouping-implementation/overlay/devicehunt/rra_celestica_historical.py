"""Additional whole Celestica legal names from reviewed historical exhibits."""
import datetime as dt
import json

from .catalog import normalize
from .documents import dependency

EXHIBITS = {
    'sec-8.1': {
        'source_url': 'https://www.sec.gov/Archives/edgar/data/1030894/000162828018003056/exhibit81.htm',
        'capture_method': 'reviewed factual web-tool exhibit extract; not original HTML',
        'exhibit': '8.1',
        'names': {'Celestica (Dongguan-SSL) Technology Limited', 'Celestica Hong Kong Limited',
                  'Celestica (Suzhou) Technology Co. Ltd'},
        'reviewed': [
            ('Celestica (Dongguan-SSL) Technology Limited', 'Celestica (Dongguan-SSL) Technology Limited', 'exact legal name'),
            ('Celestica (Dongguan-SSL) Technology Ltd', 'Celestica (Dongguan-SSL) Technology Limited', 'reviewed Ltd abbreviation of Limited'),
            ('Celestica Hong Kong Ltd', 'Celestica Hong Kong Limited', 'reviewed Ltd abbreviation of Limited'),
            ('Celestica (Suzhou) Technology Co. Ltd', 'Celestica (Suzhou) Technology Co. Ltd', 'exact legal name'),
        ],
    },
    'archived-21.1': {
        'source_url': 'https://corporate.celestica.com/static-files/aae92c39-4634-4e66-805e-6fb4adca1376',
        'capture_method': 'reviewed factual web-search exhibit extract; not original PDF',
        'exhibit': '21.1',
        'names': {'Dongguan Celestica Electronics, Ltd.', 'Celestica Kladno, s.r.o.'},
        'reviewed': [
            ('Dongguan Celestica Electronics Ltd', 'Dongguan Celestica Electronics, Ltd.', 'exact legal name after punctuation normalization'),
            ('Celestica Kladno s.r.o.', 'Celestica Kladno, s.r.o.', 'exact legal name after punctuation normalization'),
        ],
    },
}
PERIOD = 'not established from reviewed excerpt'


def load(root, data):
    from . import rra_company_names
    if (data.get('schema_version') != 2 or data.get('kind') != 'celestica-historical-company-names'
            or set(data.get('exhibits', {})) != set(EXHIBITS)):
        raise ValueError('Unrecognized historical Celestica company bundle')
    # This base must be the already supported version-1 extract, never a
    # recursively nested company bundle or unrelated company's evidence.
    base = json.loads(dependency(root, data['base']))
    if base.get('schema_version') != 1 or base.get('source_url') != rra_company_names.URL:
        raise ValueError('Historical Celestica bundle requires its reviewed base')
    result = rra_company_names.load(root, data['base'])
    for key, spec in EXHIBITS.items():
        entry = data['exhibits'][key]
        evidence = json.loads(dependency(root, entry))
        if (evidence.get('schema_version') != 1
                or any(evidence.get(k) != spec[k] for k in ('source_url', 'capture_method', 'exhibit'))
                or evidence.get('registrant') != 'Celestica Inc.'
                or evidence.get('relationship') != 'subsidiaries of registrant'
                or evidence.get('evidence_period') != PERIOD
                or not isinstance(evidence.get('subsidiaries'), list)
                or any(not isinstance(n, str) for n in evidence['subsidiaries'])
                or len(evidence['subsidiaries']) != len(spec['names'])
                or set(evidence['subsidiaries']) != spec['names']):
            raise ValueError('Unrecognized historical Celestica subsidiary extract')
        dt.date.fromisoformat(evidence['reviewed_on'])
        for spelling, legal, basis in spec['reviewed']:
            name = normalize(spelling)
            if name in result: raise ValueError('Conflicting company-name evidence')
            result[name] = dict(target='Celestica', legal_name=legal, mapping_basis=basis,
                                source_url=spec['source_url'], sha256=entry['sha256'],
                                capture_method=spec['capture_method'], exhibit=spec['exhibit'],
                                evidence_period=PERIOD,
                                scope='historical legal-name association only; ownership at approval date and current ownership unverified')
    return result

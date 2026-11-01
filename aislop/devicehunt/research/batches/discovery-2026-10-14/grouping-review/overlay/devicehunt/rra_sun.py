"""Sun approvals with a dated Oracle acquisition relationship, not OEM aliases."""
import datetime as dt
import json
from pathlib import Path

from . import rra
from .catalog import digest, normalize
from .documents import dependency

URL = 'https://www.sec.gov/Archives/edgar/data/1341439/000119312510015241/d8k.htm'
METHOD = 'reviewed factual web-tool filing extract; not original HTML'
ROLE = 'acquired_company_portfolio'
NAMES = {normalize('Sun Microsystems, Inc.'): 'exact legal name',
         normalize('Sun Microsystems'): 'reviewed short company name without Inc suffix'}


def company_basis(name):
    if any(c.isalnum() and not c.isascii() for c in name):
        return None
    return NAMES.get(normalize(name))


def validate(path):
    path = Path(path).resolve()
    data, ready = rra.validate(path)
    if data['query']['maker'] != 'Sun Microsystems' or data.get('company_evidence') is not None:
        raise ValueError('Sun portfolio import requires its exact query and separate lineage evidence')
    entry = data.get('sun_lineage')
    if not isinstance(entry, dict):
        raise ValueError('Missing Sun acquisition evidence')
    proof = json.loads(dependency(path.parent, entry))
    expected = dict(schema_version=1, source_url=URL, capture_method=METHOD,
                    filing='Form 8-K', item='2.01', registrant='Oracle Corporation',
                    acquired_company='Sun Microsystems, Inc.', completed_on='2010-01-26',
                    announcement_on='2010-01-27', relationship='wholly owned subsidiary')
    if any(proof.get(k) != v for k, v in expected.items()):
        raise ValueError('Unrecognized Oracle-Sun acquisition evidence')
    dt.date.fromisoformat(proof['reviewed_on'])
    return data, ready, proof


def import_bundle(c, path):
    data, ready, proof = validate(path)  # Validate both sources before replacing either.
    # Preserve all original registry rows, including unfamiliar legal entities.
    raw_result = rra.import_bundle(c, path)
    sid = 'rra-sun-portfolio-' + digest(json.dumps(data['query'], sort_keys=True).encode())[:12]
    c.source(sid, rra.FORM, 'regulatory', 'RRA and SEC factual records; source rights retained',
             'Selected exact Sun Microsystems manufacturer names from the checked official RRA query. '
             'All query rows remain separately searchable in rra_equipment. A separately hashed factual '
             'web-tool extract of Oracle Form 8-K item 2.01 records Sun becoming an Oracle subsidiary '
             'on 2010-01-26; original filing HTML was not cached after direct HTTP 403. '
             'Oracle affiliation is acquired_company_portfolio only, including older Sun products: '
             'not Oracle manufacturing, ownership at approval date, current support or device deployment. '
             'Sun remains the manufacturer and identity namespace. Its reviewed short name without Inc '
             'is accepted; other legal entities/composite cells remain unassigned. No cross-brand model '
             'alias, factory or auction-unit provenance is inferred.', path)
    added = matched = 0
    for _, entry, row in ready:
        basis = company_basis(row['manufacturer'])
        if basis is None:
            continue
        matched += 1
        for model_role, model in rra.models(row):
            c.add(sid, row['certificate_number'] + ':' + model, entry['url'],
                  'Sun Microsystems', model, 'Sun Microsystems ' + model, row['equipment_name'], row,
                  [(row['manufacturer'], 'registry_manufacturer'),
                   (row['applicant'], 'regulatory_applicant'), ('Oracle Corporation', ROLE)],
                  [('rra_certificate_id', row['certificate_number'])],
                  [('rra_model_role', model_role), ('rra_main_model', row['main_model']),
                   ('country_raw', row['country_raw']), ('approval_date_raw', row['approval_date_raw']),
                   ('other_raw', row['other_raw']), ('detail_sha256', entry['sha256']),
                   ('lineage_company_mapping_basis', basis), ('lineage_acquired_company', proof['acquired_company']),
                   ('lineage_completed_on', proof['completed_on']),
                   ('lineage_announcement_on', proof['announcement_on']),
                   ('lineage_source_url', URL), ('lineage_capture_method', METHOD),
                   ('lineage_source_sha256', data['sun_lineage']['sha256']),
                   ('lineage_scope', 'Acquired-company portfolio; not Oracle manufacture or ownership at approval date')])
            added += 1
    # Apply the same Unicode/composite-company safeguards as the original RRA importer.
    for company in {row[key] for _, _, row in ready for key in ('manufacturer', 'applicant')}:
        c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND role!=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=?)',
                     (rra.company_target(company), company, ROLE, sid))
    c.finish(sid, added)
    return dict(registry_rows=raw_result['registry_rows'], matched_manufacturer_rows=matched,
                portfolio_assertions=added)

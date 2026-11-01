"""Exact Joy manufacturer rows with a separately dated Accton portfolio link."""
import datetime as dt
import json
from pathlib import Path

from . import anatel
from .documents import dependency

SOURCE = 'anatel-accton-historical-portfolio'
COMPANY = 'Joy Technology (Shenzhen) Co., Ltd.'
ROLE = 'historical_subsidiary_portfolio'
PRESS = 'https://www.accton.com/accton-technology-announces-strategic-move-with-digital-china-group-to-reshape-market-presence-in-china/'
FILING = 'https://www.accton.com/wp-content/uploads/2025/10/20251028075444.pdf'
METHOD = 'reviewed factual web-search extracts; original HTML and PDF unavailable (HTTP 402)'
PROOF = dict(schema_version=1, company=COMPANY, parent='Accton Technology Corporation',
             capture_method=METHOD, subsidiary_stated_on='2024-09-26',
             announcement_url=PRESS, disposal_source_url=FILING,
             disposal_note='32', control_transferred_on='2025-04-30')
SCOPE = ('Exact Joy Technology manufacturer cells from the validated Anatel snapshot. '
         'Accton has historical_subsidiary_portfolio role only: its 2024-09-26 announcement '
         'names Joy as a subsidiary; a later filing, note 32, reports control transferred '
         '2025-04-30. Corporate evidence is a hashed factual web-search extract, not original '
         'HTML/PDF (HTTP 402). No Accton parent manufacturing, ownership at approval date, '
         'present subsidiary status, retail brand, factory, hyperscaler deployment, model '
         'alias or auction-unit provenance is inferred. Other legal names remain unresolved.')


def validate(path):
    path = Path(path).resolve()
    bundle = json.loads(path.read_text())
    if bundle.get('schema_version') != 1:
        raise ValueError('Unrecognized Accton portfolio bundle')
    # Both dependencies and the complete registry validate before any mutation.
    dependency(path.parent, bundle['registry'])
    proof = json.loads(dependency(path.parent, bundle['company_evidence']))
    if any(proof.get(k) != v for k, v in PROOF.items()):
        raise ValueError('Unrecognized Joy-Accton corporate evidence')
    dt.date.fromisoformat(proof['reviewed_on'])
    rows = anatel.validate_bundle(path.parent / bundle['registry']['path'])
    return bundle, rows, proof


def import_bundle(c, path):
    bundle, rows, proof = validate(path)
    c.source(SOURCE, anatel.PAGE, 'regulatory',
             'Anatel and Accton factual records; source rights retained', SCOPE, path)
    count = 0
    for n, cells in enumerate(rows, 1):
        row = dict(zip((key for key, _ in anatel.FIELDS), cells))
        if row['manufacturer'].strip().casefold() != COMPANY.casefold() or not row['model'].strip():
            continue
        facts = [(k, v) for k, v in row.items() if k not in {'model', 'manufacturer'} and v]
        facts += [('registry_manufacturer', row['manufacturer']), ('csv_record_number', str(n)),
                  ('registry_bundle_sha256', bundle['registry']['sha256']),
                  ('company_evidence_sha256', bundle['company_evidence']['sha256']),
                  ('lineage_subsidiary_stated_on', proof['subsidiary_stated_on']),
                  ('lineage_control_transferred_on', proof['control_transferred_on']),
                  ('lineage_announcement_url', PRESS), ('lineage_disposal_url', FILING),
                  ('lineage_capture_method', METHOD), ('evidence_scope', SCOPE)]
        c.add(SOURCE, str(n), anatel.PAGE, COMPANY, row['model'],
              COMPANY + ' ' + row['model'], row['product_type'], row,
              [(row['manufacturer'], 'registry_manufacturer'),
               (row['applicant'], 'regulatory_applicant'), (proof['parent'], ROLE)],
              [('anatel_approval_id', row['approval_number'])], facts)
        # Explicitly keep the subsidiary manufacturer distinct from the parent target.
        c.db.execute('UPDATE affiliations SET target=NULL WHERE role=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=?)', ('registry_manufacturer', SOURCE))
        count += 1
    c.finish(SOURCE, count)
    return dict(registry_rows_checked=len(rows), portfolio_assertions=count)

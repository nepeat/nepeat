"""Refresh FCC collection and hyperscaler reports from the current read-only catalog."""
import csv
import json
from pathlib import Path

from devicehunt.catalog import Catalog, TARGETS
from devicehunt import fcc_coverage, hyperscaler_inventory
from devicehunt.__main__ import write_rows
from devicehunt.rebuild import file_sha256

root = Path.cwd()
out = root / 'data/exports'
path = root / 'data/devices.sqlite'
before = file_sha256(path)
c = Catalog(path, read_only=True)

def save(name, result, flat):
    (out / (name + '.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    with (out / (name + '.csv')).open('w', newline='') as stream:
        write_rows(flat(result), 'csv', stream)

fcc = {'all': {}, 'missing-index': {'missing': 'index'}}
for target in TARGETS:
    slug = {'Amazon Data Services': 'amazon', 'Edge-Core': 'edgecore'}.get(target, target.lower().replace(' ', '-'))
    fcc[slug] = {'vendor': target}
fcc_summaries = {}
for name, filters in fcc.items():
    result = fcc_coverage.run(c.db, limit=-1, **filters)
    save('fcc-coverage-' + name, result, fcc_coverage.flat_rows)
    fcc_summaries[name] = result['summary']

hyper = {
    'hyperscaler-inventory': {},
    'hyperscaler-inventory-all': {'include_unverified': True},
    'hyperscaler-deployed-manufacturer': {'role': 'manufacturer', 'relationship': 'documented_platform_deployment'},
}
for target in TARGETS:
    hyper['hyperscaler-inventory-' + target.lower().replace(' ', '-')] = {'vendor': target}
hyper_summaries = {}
for name, filters in hyper.items():
    result = hyperscaler_inventory.run(c.db, limit=-1, **filters)
    save(name, result, hyperscaler_inventory.flat_rows)
    hyper_summaries[name] = {k: result[k] for k in ('assertions', 'unique_identities', 'counts_by_relationship')}

# Global sitemap discovery stays separate from target filing-stage accounting.
lead_rows = []
for code, target in fcc_coverage.GRANTEES.items():
    for row in c.db.execute('SELECT fcc_id,record_url,sitemap_url,lastmod_raw FROM fcc_sitemap_urls WHERE substr(fcc_id,1,?)=? ORDER BY fcc_id', (len(code), code)):
        lead_rows.append(dict(candidate_target=target, prefix_rule=code, **dict(row)))
with (out / 'fcc-sitemap-prefix-leads.csv').open('w', newline='') as stream:
    write_rows(lead_rows, 'csv', stream)
summary = dict(page_counts=[dict(r) for r in c.db.execute('SELECT capture_state,count(*) AS pages,sum(row_count) AS rows FROM fcc_sitemap_pages GROUP BY capture_state')],
               kinds=[dict(r) for r in c.db.execute('SELECT page_kind,count(*) AS rows,count(DISTINCT record_url) AS unique_urls FROM fcc_sitemap_urls GROUP BY page_kind')],
               prefix_lead_counts={target: sum(r['candidate_target'] == target for r in lead_rows) for target in TARGETS},
               reviewed_grantee_prefixes=fcc_coverage.GRANTEES,
               scope='Literal discovery URL prefixes only; not verified company, manufacturer, model, filing or operator assertions. Zero means no matching captured URL.')
known = {r['fcc_id'].casefold() for r in fcc_coverage.run(c.db, limit=-1)['rows']}
raw = {r[0].casefold() for r in c.db.execute("SELECT fcc_id FROM fcc_sitemap_urls WHERE fcc_id!=''")}
summary['current_known_id_comparison'] = dict(known=len(known), present=len(known & raw), absent=len(known - raw))
(out / 'fcc-sitemap-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
c.db.close()
assert file_sha256(path) == before
report = dict(database_sha256=before, read_only=True, fcc=fcc_summaries, hyperscaler=hyper_summaries)
(out / 'collection-reports-audit.json').write_text(json.dumps(report, indent=2) + '\n')
(out / 'hyperscaler-inventory-audit.json').write_text(json.dumps(
    dict(database_sha256=before, database_unchanged=True, counts=hyper_summaries), indent=2) + '\n')
print(json.dumps({'fcc': fcc_summaries['all'], 'hyperscaler': hyper_summaries['hyperscaler-inventory'], 'target_groups': len(TARGETS)}, indent=2))

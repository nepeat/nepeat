import json
from pathlib import Path
from devicehunt import rra_company_names as names
from devicehunt.catalog import digest
root=Path('data/raw/rra-accton-official')
evidence=root/'accton-singapore-2013.json'
data=dict(schema_version=1,source_url=names.ACCTON_URL,capture_method=names.ACCTON_METHOD,reviewed_on='2026-09-27',reporting_year='2013',document='2013-Consolidated-Report.pdf',reported_parent='Edgecore Networks',relationship='wholly owned subsidiaries',subsidiaries=sorted(names.ACCTON_NAMES),scope='Selected factual company names and stated relationship from the public web-search result. Direct PDF opening returned HTTP 402 and was not retried; original pages and full context were not retrieved. Year identifies the report, not a verified exact reporting date. Whole RRA manufacturer spelling maps only to the Accton-named company namespace. No parent ownership at device approval dates, current ownership, factory, model alias, or deployment is inferred. SMC company names provide extract context only, not additional aliases.')
evidence.write_text(json.dumps(data,indent=2)+'\n')
p=root/'bundle.json';original=p.read_bytes()
backup=root/'bundle-before-company-review.json'
assert not backup.exists()
backup.write_bytes(original)
bundle=json.loads(original);assert 'company_evidence' not in bundle
bundle['company_evidence']=dict(path=evidence.name,sha256=digest(evidence.read_bytes()))
p.write_text(json.dumps(bundle,indent=2)+'\n')

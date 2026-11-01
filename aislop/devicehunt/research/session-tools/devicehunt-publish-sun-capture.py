import json
from pathlib import Path
from devicehunt import rra,rra_sun
from devicehunt.catalog import digest
root=Path('data/raw/rra-sun-official')
proof=dict(schema_version=1,source_url=rra_sun.URL,capture_method=rra_sun.METHOD,reviewed_on='2026-09-27',filing='Form 8-K',item='2.01',registrant='Oracle Corporation',acquired_company='Sun Microsystems, Inc.',completed_on='2010-01-26',announcement_on='2010-01-27',relationship='wholly owned subsidiary',scope='Factual acquisition fields from the successfully read public web-tool filing text. A subsequent direct HTML request returned HTTP 403 and was not retried. Original HTML is not cached. Completion date is from Item 2.01; announcement date is from Item 8.01. Corporate acquisition only: not Oracle manufacture or ownership at every device approval date, current support, product equivalence or deployment.')
p=root/'oracle-sun-acquisition.json';p.write_text(json.dumps(proof,indent=2)+'\n')
data=json.loads((root/'candidate.json').read_text());data['csv_comparison']='exact_rows_ignore_order';data['sun_lineage']=dict(path=p.name,sha256=digest(p.read_bytes()))
p=root/'reviewed-candidate.json';p.write_text(json.dumps(data,indent=2)+'\n')
rra_sun.validate(p);assert not (root/'bundle.json').exists();p.rename(root/'bundle.json')
print('Published reviewed bundle; original failed candidate and both CSVs retained.')

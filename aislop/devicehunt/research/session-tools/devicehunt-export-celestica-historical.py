import json
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.__main__ import write_rows
with_catalog=Catalog(Path('data/devices.sqlite'),read_only=True)
rows=[dict(r) for r in with_catalog.db.execute("""SELECT d.model,r.source_url,a.company AS manufacturer,
(SELECT value FROM facts f WHERE f.record_id=r.id AND f.name='manufacturer_company_legal_name') AS reviewed_legal_name,
(SELECT value FROM facts f WHERE f.record_id=r.id AND f.name='manufacturer_company_source_url') AS legal_name_evidence_url,
(SELECT value FROM facts f WHERE f.record_id=r.id AND f.name='manufacturer_company_scope') AS scope
FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id
WHERE a.target='Celestica' AND a.role='registry_manufacturer' AND EXISTS
(SELECT 1 FROM facts f WHERE f.record_id=r.id AND f.name='manufacturer_company_evidence_period')
ORDER BY d.model,r.source_url""")]
assert len(rows)==21
out=Path('data/exports')
(out/'celestica-historical-additions.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
with (out/'celestica-historical-additions.csv').open('w',newline='') as f:write_rows(rows,'csv',f)
with_catalog.db.close()
print('Exported 21 manufacturer assertions with legal-name evidence and limitations.')

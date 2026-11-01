import json
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt.__main__ import write_rows
path=Path('data/devices.sqlite').resolve();before=digest(path.read_bytes());c=Catalog(path,read_only=True);out=Path('data/exports')
queries={
 'nigeria-register': 'SELECT * FROM nigeria_equipment ORDER BY row_number',
 'nigeria-target-claims': "SELECT d.brand,d.model,a.role,a.company,a.target,r.source_url FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id='nigeria-ncc-equipment' ORDER BY d.brand,d.model,a.role",
 'nigeria-text-issues': "SELECT row_number,equipment_name,manufacturer,invalid_cell_bytes_json FROM nigeria_equipment WHERE invalid_cell_bytes_json<>'{}' ORDER BY row_number",
 'nigeria-amazon-query': "SELECT models,manufacturer,equipment_name,applicant,certificate_holder FROM nigeria_equipment WHERE manufacturer LIKE '%AMAZON DATA%' ORDER BY row_number"
}
for name,query in queries.items():
 rows=[dict(r) for r in c.db.execute(query)]
 (out/(name+'.json')).write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
 with (out/(name+'.csv')).open('w',newline='') as f:write_rows(rows,'csv',f)
 print(name,len(rows))
assert not c.db.execute("SELECT 1 FROM nigeria_equipment WHERE models IN ('7760-32X-A-AC-F','AZ4444-52X')").fetchall()
c.db.close();after=digest(path.read_bytes());assert before==after
(out/'nigeria-sql-check.json').write_text(json.dumps(dict(database_sha256=after,readonly_queries_unchanged_database=True,queries=queries,missing_baseline_labels_absent=True),indent=2)+'\n')

import json
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt.__main__ import write_rows
path=Path('data/devices.sqlite').resolve();before=digest(path.read_bytes());c=Catalog(path,read_only=True);out=Path('data/exports')
queries={
 'ghana-register':'SELECT * FROM ghana_equipment ORDER BY row_number',
 'ghana-oracle-query':"SELECT approval_number,manufacturer,brand,product_type,model,detail_url FROM ghana_equipment WHERE manufacturer LIKE '%Oracle%' ORDER BY row_number",
 'ghana-missing-approval-numbers':"SELECT row_number,detail_id,manufacturer,brand,model,detail_url FROM ghana_equipment WHERE approval_number='' ORDER BY row_number"
}
for name,q in queries.items():
 rows=[dict(r) for r in c.db.execute(q)]
 (out/(name+'.json')).write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
 with (out/(name+'.csv')).open('w',newline='') as f:write_rows(rows,'csv',f)
 print(name,len(rows))
assert not c.db.execute("SELECT 1 FROM ghana_equipment WHERE model IN ('7760-32X-A-AC-F','AZ4444-52X')").fetchall()
c.db.close();after=digest(path.read_bytes());assert before==after
(out/'ghana-sql-check.json').write_text(json.dumps(dict(database_sha256=after,readonly_queries_unchanged_database=True,queries=queries,missing_baseline_labels_absent=True),indent=2)+'\n')

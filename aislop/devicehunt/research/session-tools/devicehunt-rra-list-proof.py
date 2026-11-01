import json
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import validation
from devicehunt.__main__ import write_rows
root=Path.cwd();path=root/'data/devices.sqlite';before=digest(path.read_bytes())
c=Catalog(path,read_only=True);old=Catalog(root/'data/rebuilt-with-rra-main-models.sqlite',read_only=True)
db=c.db;db.execute('ATTACH DATABASE ? AS previous',((root/'data/rebuilt-with-rra-main-models.sqlite').as_uri()+'?mode=ro',))
result={}
for table in ('devices','records','affiliations','identifiers','facts','hyperscaler_evidence','sources','rra_equipment'):
    cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')]
    fields=','.join('"'+k+'"' for k in cols)
    missing=db.execute('SELECT count(*) FROM (SELECT '+fields+' FROM previous.'+table+
        ' EXCEPT SELECT '+fields+' FROM main.'+table+')').fetchone()[0]
    assert missing==0,table
    previous=db.execute('SELECT count(*) FROM previous.'+table).fetchone()[0]
    current=db.execute('SELECT count(*) FROM main.'+table).fetchone()[0]
    if table=='rra_equipment':assert previous==current
    result[table]=dict(previous=previous,current=current,previous_rows_missing_or_changed=missing)
old_base=validation.run(old.db,root/'validation/owned-hardware.json');base=validation.run(db,root/'validation/owned-hardware.json')
assert old_base['results']==base['results']
query="""SELECT d.brand,d.model,a.company AS named_manufacturer,
       f.value AS complete_manufacturer_field,r.source_url
FROM records r
JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id AND a.role='registry_manufacturer'
JOIN facts f ON f.record_id=r.id AND f.name='rra_manufacturer_as_printed'
WHERE r.source_id='rra-reviewed-manufacturer-lists'
ORDER BY d.brand,d.model,r.source_url,a.company"""
rows=[dict(r) for r in db.execute(query)];assert len(rows)==47
out=root/'data/exports';out.joinpath('rra-manufacturer-lists-sql.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
with out.joinpath('rra-manufacturer-lists-sql.csv').open('w',newline='') as f:write_rows(rows,'csv',f)
db.close();old.db.close();assert before==digest(path.read_bytes())
audit=dict(tables=result,baseline_results_unchanged=True,query=query,query_rows=len(rows),
    database_sha256=before,readonly_queries_unchanged_database=True)
out.joinpath('rra-manufacturer-lists-proof-check.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))

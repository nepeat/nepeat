import json
from pathlib import Path
from devicehunt.catalog import Catalog, digest
from devicehunt import validation
from devicehunt.__main__ import write_rows
root=Path.cwd(); path=root/'data/devices.sqlite'; before=digest(path.read_bytes())
live=Catalog(path,read_only=True); old=Catalog(root/'data/rebuilt-with-ghana.sqlite',read_only=True)
old_ids={r[0] for r in old.db.execute('SELECT id FROM records')}
checked={}
for table in ('affiliations','identifiers','facts','hyperscaler_evidence'):
    previous={tuple(r) for r in old.db.execute('SELECT * FROM '+table)}
    current={tuple(r) for r in live.db.execute('SELECT * FROM '+table) if r['record_id'] in old_ids}
    assert previous==current, table
    checked[table]=len(previous)
old_base=validation.run(old.db,root/'validation/owned-hardware.json')
base=validation.run(live.db,root/'validation/owned-hardware.json')
for result in (old_base,base):
    for row in result['results']:
        for proof in row['evidence']+row['leads']:
            proof.pop('source_coverage')
assert old_base['results']==base['results'], 'Baseline proof changed beyond coverage wording'
query="""SELECT d.brand, d.model, a.company AS manufacturer, f.value AS main_model_as_printed,
       r.source_url
FROM records r
JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id AND a.role='registry_manufacturer'
JOIN facts f ON f.record_id=r.id AND f.name='rra_main_model'
JOIN facts role ON role.record_id=r.id AND role.name='rra_model_role' AND role.value='main'
WHERE r.source_id GLOB 'rra-official-query-*' AND f.value LIKE '%,%'
ORDER BY d.brand,d.model,r.source_url"""
rows=[dict(r) for r in live.db.execute(query)]; assert len(rows)==84
out=root/'data/exports'
out.joinpath('rra-main-models-sql.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
with out.joinpath('rra-main-models-sql.csv').open('w',newline='') as stream:write_rows(rows,'csv',stream)
live.db.close();old.db.close(); assert digest(path.read_bytes())==before
result=dict(unchanged_existing_record_evidence=checked,
    baseline_results_unchanged_except_source_coverage_wording=True,
    query=query,query_rows=len(rows), database_sha256=before,readonly_queries_unchanged_database=True)
out.joinpath('rra-main-models-proof-check.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))

import json,re,time
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import validation
from devicehunt.__main__ import write_rows

root=Path.cwd();path=root/'data/devices.sqlite';before=digest(path.read_bytes())
c=Catalog(path,read_only=True);copy=Catalog(root/'data/rebuilt-with-rra-manufacturer-lists.sqlite',read_only=True)
baseline=validation.run(c.db,root/'validation/owned-hardware.json')
old_baseline=json.loads((root/'data/exports/inventory-validation.json').read_text())
assert baseline['results']==old_baseline['results']
cases=[('7760',None,None),('7760 32X',None,None),('7760_32X',None,None),
       ('7760','Edge-Core',None),('7760','Edge-Core','registry_manufacturer'),
       ('8200','Celestica',None),('8210','Celestica',None),('7772','Edge-Core',None),
       ('AS7760-32X',None,None),('100G',None,None),('E5-2C','Oracle',None),
       ('7760-32X-A-AC-F','Edge-Core',None),('AZ4444-52X','Edge-Core',None)]
audit=[]
for query,target,role in cases:
    args=[' AND '.join('"'+t+'"*' for t in re.findall(r'\w+',query,flags=re.UNICODE))]
    sql='SELECT d.id FROM devices d WHERE d.id IN (SELECT device_id FROM search_index WHERE search_index MATCH ?)'
    if target or role:
        sub='SELECT 1 FROM records r JOIN affiliations a ON a.record_id=r.id WHERE r.device_id=d.id'
        if target:sub+=' AND a.target=?';args.append(target)
        if role:sub+=' AND a.role=?';args.append(role)
        sql+=' AND EXISTS ('+sub+')'
    previous_ids={r[0] for r in c.db.execute(sql,args)}
    start=time.perf_counter();rows=c.search(query,target=target,role=role,limit=-1);elapsed=time.perf_counter()-start
    assert previous_ids <= {r['id'] for r in rows}, query
    assert rows==copy.search(query,target=target,role=role,limit=-1)
    audit.append(dict(query=query,target=target,role=role,previous_word_prefix_matches=len(previous_ids),
                      current_matches=len(rows),elapsed_ms=round(elapsed*1000,3),
                      matches_verified_rebuild=True,rows=rows))
assert audit[0]['previous_word_prefix_matches']==1 and audit[0]['current_matches']==3
with (root/'data/exports/search-fragment-examples.json').open('w') as f:
    json.dump(audit,f,ensure_ascii=False,indent=2);f.write('\n')
with (root/'data/exports/search-7760.csv').open('w',newline='') as f:write_rows(audit[0]['rows'],'csv',f)
c.db.close();copy.db.close();after=digest(path.read_bytes());assert before==after
result=dict(database_sha256=after,readonly_search_preserved_database=True,
            baseline_results_unchanged=True,baseline=baseline['summary'],
            checked_queries=len(audit),checked_against='data/rebuilt-with-rra-manufacturer-lists.sqlite',
            max_elapsed_ms=max(a['elapsed_ms'] for a in audit))
(root/'data/exports/search-fragment-audit.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
for item in audit:print(item['query'],item['target'],item['role'],item['previous_word_prefix_matches'],'->',item['current_matches'])

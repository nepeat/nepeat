import json
from collections import Counter
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import specifications, validation
from devicehunt.__main__ import write_rows
from devicehunt.rebuild import file_sha256

out=Path('data/exports'); before=json.loads((out/'specification-scope-before.json').read_text())
db_path=Path('data/devices.sqlite'); assert file_sha256(db_path)==before['database_sha256']
c=Catalog(db_path,read_only=True)
rows=specifications.search(c,limit=-1)
def original_key(row):
    return tuple(row[k] for k in ['device_id','brand','model','category','statement','source_id','source_url','tier'])
assert Counter(map(original_key,rows))==Counter(map(original_key,before['unfiltered']))
affiliations={}
for row in c.db.execute('SELECT record_id,target FROM affiliations WHERE target IS NOT NULL'):
    affiliations.setdefault(row['record_id'],set()).add(row['target'])
summary={}
for target,old in before['vendors'].items():
    current=specifications.search(c,target=target,limit=-1)
    assert all(target in affiliations.get(r['record_id'],set()) for r in current)
    expected=[r for r in rows if target in affiliations.get(r['record_id'],set())]
    assert Counter(map(original_key,current))==Counter(map(original_key,expected))
    removed=Counter(map(original_key,old))-Counter(map(original_key,current))
    assert not (Counter(map(original_key,current))-Counter(map(original_key,old)))
    summary[target]=dict(before=len(old),after=len(current),cross_record_statements_removed=sum(removed.values()))
    with (out/('specifications-'+target.lower().replace(' ','-')+'.json')).open('w') as stream:
        write_rows(current,'json',stream)
for fmt in ['json','csv']:
    with (out/('specifications.'+fmt)).open('w',newline='') as stream:write_rows(rows,fmt,stream)
with (out/'dcs204-specifications.csv').open('w',newline='') as stream:
    write_rows(specifications.search(c,'DCS204',exact_model=True,limit=-1),'csv',stream)
example=specifications.search(c,'AIS800-32O',target='Accton',exact_model=True,limit=-1)
assert all(r['source_id']!='edgecore-products' for r in example)
assert any(r['source_id']=='edgecore-products' for r in before['example'])
assert any(r['source_id']=='edgecore-products' for r in specifications.search(c,'AIS800-32O',target='Edge-Core',exact_model=True,limit=-1))
# All exact-model queries with categorized statements keep their own models and
# every corresponding source record; aliases are validated separately in tests.
exact_queries=0
for model in sorted({r['model'] for r in rows}):
    exact=specifications.search(c,model,exact_model=True,limit=-1)
    direct=[r for r in rows if r['model'].strip().casefold()==model.strip().casefold()]
    assert not (Counter(map(original_key,direct))-Counter(map(original_key,exact)))
    exact_queries+=1
baseline=validation.run(c.db,Path('validation/owned-hardware.json'))
assert baseline['summary']==dict(total=10,hits=9,gaps=1)
c.db.close(); assert file_sha256(db_path)==before['database_sha256']
audit=dict(database_sha256=before['database_sha256'],database_unchanged=True,all_unfiltered_statements_preserved=len(rows),vendor_counts=summary,exact_model_queries_checked=exact_queries,example_after=example,owned_baseline=baseline['summary'])
(out/'specification-scope-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))

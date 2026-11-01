import json,sqlite3,re
ROOT='/home/erin/nepeat/aislop/devicehunt'
r=sqlite3.connect('file:'+ROOT+'/data/devices.sqlite?mode=ro',uri=True)
d=json.load(open(ROOT+'/research/batches/discovery-2026-10-11/onie/extraction.json'))
print(r.execute("select kind,count(*) from identifiers where kind like '%onie%' group by kind").fetchall())
print(r.execute("select d.brand,d.model,i.value from identifiers i join devices d on d.id=i.record_id where i.kind='onie_platform' limit 3").fetchall()) if 'record_id' else None
ids={}
for k,v in r.execute("select kind,value from identifiers"): ids.setdefault(v.lower(),set()).add(k)
models={}
for b,m in r.execute("select brand,model from devices"): models.setdefault(m.lower(),set()).add(b)
dir_ident=[];no=[];sonic=[];onl=[];hdr={}
for n,v in d.items():
    k=ids.get(n.lower())
    (dir_ident if k else no).append((n,sorted(k) if k else None))
    if ('sonic_platform' in ids.get(f"x86_64-{n}-r0".lower(),())): sonic.append(f"x86_64-{n}-r0")
    o="x86-64-"+n.replace('_','-')+"-r0"
    if 'onl_platform' in ids.get(o,()): onl.append(o)
print('dir name equals existing identifier value:',len(dir_ident),'kinds',{tuple(x[1]) for x in dir_ident})
print('no identifier match:',[x[0] for x in no])
print('sonic x86_64-<dir>-r0 equals existing sonic_platform id:',len(sonic),sonic)
print('onl x86-64-<dir with - >-r0 equals existing onl_platform:',len(onl))
# dir name equals devices.model exactly
print('dir equals a devices.model:',[(n,models[n.lower()]) for n in d if n.lower() in models])

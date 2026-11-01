import sqlite3,json,re,os
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/entity-manager'
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-entity-manager'
d=sqlite3.connect(S+'/t.sqlite')
for t in('sources','devices','records','identifiers','facts','affiliations'):
    try: print(t,d.execute(f'select count(*) from {t}').fetchone()[0])
    except Exception as e: print(t,e)
print(d.execute('pragma integrity_check').fetchall(),d.execute('pragma foreign_key_check').fetchall())
# line-literal verification
c=json.load(open(RAW+'/entity-manager-config-facts-2026-10-11.json'))
cache={}
tot=bad=0
for dv in c['devices']:
    for k,v in dv['facts']:
        if k in('em_probe','em_config_name','em_config_type') or k.startswith('em_probe_') or k.startswith('em_asset'):
            m=re.match(r'(.*) \((\S+\.json):(\d+)\)$',v,re.S); assert m,(k,v)
            val,b,l=m.group(1),m.group(2),int(m.group(3))
            val=re.sub(r'^FRU-field reference (\S+) \(not a literal value\)$',r'\1',val);val=re.sub(r'^placeholder (\S+) \(.*\)$',r'\1',val);val=re.sub(r'^literal "(.*)"$',r'\1',val)
            p=[x for x in os.popen('true')] 
            path=[f for f in dv['facts'] if f[0]=='em_config_file'][0][1]
            lines=cache.setdefault(path,open(RAW+'/'+path).read().split('\n'))
            tot+=1
            if val not in lines[l-1]: bad+=1;print('MISS',k,v[:80],lines[l-1][:80])
print('line-literal checks',tot,'miss',bad)
# owned
pat=re.compile(r'CS8200|CS8210|AZ4444|7760|DMC-12|AS7772|SV1120|SV1121|SV2121|SV2221|bytedance|B4020|B4010',re.I)
hits=[]
for r,_,fs in os.walk(RAW):
    for f in fs:
        p=os.path.join(r,f)
        if f in('capture-manifest.json',) or f.startswith('entity-manager-config-facts'): continue
        for i,l in enumerate(open(p,encoding='utf-8',errors='replace'),1):
            if pat.search(l): hits.append((p[len(RAW):],i,l.strip()[:100]))
        if pat.search(p): hits.append((p,0,'path'))
print('owned hits',hits[:10],len(hits))

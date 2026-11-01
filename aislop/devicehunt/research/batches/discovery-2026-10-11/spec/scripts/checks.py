import json,re,sqlite3,glob,os
R='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-spec'
D='/home/erin/nepeat/aislop/devicehunt/data/devices.sqlite'
cand=json.load(open(R+'/spec-results-facts-2026-10-11.json'))
c=sqlite3.connect('file:'+D+'?mode=ro',uri=True)
# owned check over candidate values + paths + all captured text
pat=re.compile(r'CS8200|CS8210|AZ4444|7760|AS7760|DMC-12|AS7772|SV1120|SV1121|SV2121|SV2221|ByteDance|B4020|B4010|\bE4-2C\b|\bE5-2C\b|\bX7-2C\b|\bX10-2C\b',re.I)
hits=[]
nfiles=0
for p in glob.glob(R+'/**/*',recursive=True):
    if os.path.isfile(p) and 'index/' not in p:
        nfiles+=1
        s=open(p,'rb').read().decode('utf-8','replace')
        for m in pat.finditer(p+'\n'+s): hits.append((p[len(R)+1:],m.group(0)))
print('files',nfiles,'hits',len(hits)); 
import collections; print(collections.Counter((h[0].split('/')[0],h[1]) for h in hits).most_common(20))
# index files separately
ih=collections.Counter()
for p in glob.glob(R+'/index/*'):
    s=open(p,'rb').read().decode('utf-8','replace')
    for m in pat.finditer(s): ih[(os.path.basename(p),m.group(0))]+=1
print('index hits',ih)
# leads
models={}
for d in cand['devices']: models.setdefault((d['brand'],d['model']),0); models[(d['brand'],d['model'])]+=1
leads=[]
for (b,m),n in models.items():
    for r in c.execute("select id,brand,model from devices where lower(model)=lower(?)",(m,)): leads.append(('model',b,m,r))
    for r in c.execute("select record_id,kind,value from identifiers where lower(value)=lower(?)",(m,)): leads.append(('ident',b,m,r))
print('distinct (brand,model)',len(models),'distinct model',len({m for _,m in models}),'leads',len(leads)); 
for l in leads: print(l)

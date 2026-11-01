import sys,json,os
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec'
sys.path.insert(0,S); sys.path.insert(0,S+'/w9')
from match import vend
from fetch9 import fetch,LOG
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-spec/'
R=json.load(open(S+'/cpu_rows.json'))
have=set(os.listdir(RAW+'cpu2017'))|set(os.listdir(RAW+'wave8/cpu2017'))
rows=[r for r in R if vend(r['test_sponsor']+' || '+r['hw_model'])[:1] in (['Inspur'],['IEIT'])]
fetched={(r['test_sponsor'],r['hw_model']) for r in rows if r['_links']['Text'].split('/')[-1] in have}
d={}
for r in sorted(rows,key=lambda r:r['_links']['Text'].split('/')[-1],reverse=True):
    k=(r['test_sponsor'],r['hw_model'])
    if k in fetched: continue
    d.setdefault(k,r)
sel=sorted(d.values(),key=lambda r:r['_links']['Text'].split('/')[-1],reverse=True)
print(len(sel),flush=True)
os.makedirs(os.path.dirname(LOG),exist_ok=True)
for r in sel[:150]:
    u='https://www.spec.org'+r['_links']['Text']
    rec=fetch(u,'wave9/cpu2017/'+u.split('/')[-1]); print(rec['status'],u,flush=True)
    if rec['status']==429: break

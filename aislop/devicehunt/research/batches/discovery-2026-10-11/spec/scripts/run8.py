import sys,json,os
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec'
sys.path.insert(0,S); sys.path.insert(0,S+'/w8')
from match import vend
from fetch8 import fetch,LOG
R=json.load(open(S+'/cpu_rows.json'))
have=set(os.listdir('/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-spec/cpu2017'))
q=[r for r in R if vend(r['test_sponsor']+' || '+r['hw_model'])[:1] in (['Quanta'],['Oracle/Sun']) and r['_links']['Text'].split('/')[-1] not in have]
q.sort(key=lambda r:(vend(r['test_sponsor']+r['hw_model'])[0]!='Oracle/Sun', r['_links']['Text'].split('-')[1]),reverse=False)
# oracle first? no: quanta first, newest first
qq=[r for r in q if vend(r['test_sponsor']+r['hw_model'])[0]=='Quanta']; qq.sort(key=lambda r:r['_links']['Text'].split('/')[-1],reverse=True)
oo=[r for r in q if vend(r['test_sponsor']+r['hw_model'])[0]!='Quanta']
print(len(qq),len(oo),flush=True)
os.makedirs(os.path.dirname(LOG),exist_ok=True)
fetch('https://www.spec.org/robots.txt','wave8/robots/robots.txt')
for r in qq+oo:
    u='https://www.spec.org'+r['_links']['Text']
    rec=fetch(u,'wave8/cpu2017/'+u.split('/')[-1]); print(rec['status'],u,flush=True)
    if rec['status']==429: break

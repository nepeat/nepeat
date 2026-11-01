import sys,json,collections
sys.path.insert(0,'/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec')
from match import vend
from fetch import fetch,LOG
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec'
R=json.load(open(S+'/cpu_rows.json'))
q=[]
for r in R:
    v=vend(r['test_sponsor']+' || '+r['hw_model'])
    if v: r['_v']=v[0]; r['_d']=r['_links']['Text'].split('-')[1]+r['_links']['Text'].split('-')[2]; q.append(r)
sel=[r for r in q if r['_v']=='Oracle/Sun']
def newest_distinct(vn):
    d={}
    for r in sorted([r for r in q if r['_v']==vn],key=lambda r:r['_d'],reverse=True):
        d.setdefault((r['test_sponsor'],r['hw_model']),r)
    return sorted(d.values(),key=lambda r:r['_d'],reverse=True)
sel+=newest_distinct('Quanta')+newest_distinct('IEIT')[:78]+newest_distinct('Inspur')[:79]
json.dump([r['_links']['Text'] for r in sel],open(S+'/cpu_sel.json','w'))
done={json.loads(l)['url'] for l in open(LOG)}
for r in sel:
    u='https://www.spec.org'+r['_links']['Text']
    if u in done: continue
    rec=fetch(u,'cpu2017/'+u.split('/')[-1])
    print(rec['status'],u,flush=True)
    if rec['status']==429: break

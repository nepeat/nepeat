import sys,json,os
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec'
sys.path.insert(0,S); sys.path.insert(0,S+'/w10')
from match import vend
from fetch10 import fetch,LOG
R=json.load(open(S+'/jbb_rows.json'))
V=lambda r:vend(r['test.testedBy']+' || '+r['product.SUT.hw.system.hw_1.name'])[:1]
fn=lambda r:r['_links']['HTML'].split('/')[-1]
sel=[]
for v in (['Oracle/Sun'],['Quanta'],['IEIT'],['Inspur']):
    sel+=sorted([r for r in R if V(r)==v],key=fn,reverse=True) if v!=['Oracle/Sun'] else [r for r in R if V(r)==v]
done={json.loads(l)['url'] for l in open(LOG)}
json.dump([r['_links']['HTML'] for r in sel],open(S+'/w10/sel.json','w'))
for r in sel:
    u='https://www.spec.org'+r['_links']['HTML']
    if u in done: continue
    try: rec=fetch(u,'wave10/jbb2015/'+u.split('/')[-1])
    except SystemExit as e: print('stop',e,u,flush=True); break
    print(rec['status'],u,flush=True)
    if rec['status']==429: break

import sys,json
sys.path.insert(0,'/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec')
from match import vend
from fetch import fetch,LOG
import os
R=json.load(open(sys.argv[1]+'/power_rows.json'))
q=[r for r in R if vend(r['wkld_ssj_global_config_hw_vendor']+' || '+r['wkld_ssj_global_config_hw_model'])]
done={json.loads(l)['url'] for l in open(LOG)}
for r in q:
    u='https://www.spec.org'+r['_links']['Text']
    if u in done: continue
    rel='power_ssj2008/'+u.split('/')[-1]
    rec=fetch(u,rel)
    print(rec['status'],u,flush=True)
    if rec['status'] in (429,): break

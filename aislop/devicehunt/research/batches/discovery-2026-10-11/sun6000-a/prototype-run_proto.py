import sys, json, sqlite3, tempfile
from pathlib import Path
sys.dont_write_bytecode=True
sys.path.insert(0,'/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/proto')
from devicehunt import review_queue as rq, rra, campaign_sources
from devicehunt.catalog import Catalog
LIVE='/home/erin/nepeat/aislop/devicehunt/data/devices.sqlite'
SID='rra-official-query-41e4ca912b33'
cand=Path('/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/proto/data/raw/rra-celestica-official/bundle-composite-cells.json')
base=rq.catalog_projection(LIVE,SID,'rra_equipment')
cp=rq.candidate_projection('rra',cand,SID)
d=rq.diff(base,cp)
print({k:(len(v) if isinstance(v,list) else v) for k,v in d.items() if k!='source_fields_changed'}, list(d['source_fields_changed']))
for r in d['records_added']:
    print(r['source_key'], r['device_id'], r['affiliations'], r['identifiers'])
print(json.dumps(d['records_added'][0]['facts'],indent=1,ensure_ascii=False)[:4000])
print('coverage diff:', d['source_fields_changed']['coverage']['after'][-700:])
out=campaign_sources.RraSavedAdapter().records(cand,'data/raw/rra-celestica-official/bundle-composite-cells.json')
print(out['stats'], out['record_sets'])
sun=[o for o in out['occurrences'] if o['certificate']=='SUN-6000']; print(sun[0]['eligible_manufacturer'], sun[0]['projected_source_keys'], sun[0]['logical_sha256'])

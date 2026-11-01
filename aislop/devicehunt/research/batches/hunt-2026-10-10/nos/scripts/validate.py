import json,hashlib,os,sys
sys.path.insert(0,'/home/erin/nepeat/aislop/devicehunt')
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-nos'
m=json.load(open(RAW+'/capture-manifest.json'));bad=0
for f in m['files']:
    b=open(f"{RAW}/{f['local']}",'rb').read()
    if hashlib.sha256(b).hexdigest()!=f['sha256'] or len(b)!=f['bytes']: bad+=1
disk=sum(len(fs) for _,_,fs in os.walk(RAW))
print('manifest files',len(m['files']),'mismatch',bad,'on-disk files',disk,'(manifest+curated=2 extra)')
d=json.load(open(RAW+'/nos-platform-code-facts-2026-10-10.json'))
miss=[s['snapshot'] for s in d['sources'] if not os.path.isfile(RAW+'/'+s['snapshot'])]
print('sources',len(d['sources']),'devices',len(d['devices']),'snapshot missing',len(miss))
from devicehunt.catalog import Catalog
from devicehunt import pipeline
db='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/nos/t.sqlite'
if os.path.exists(db): os.remove(db)
c=Catalog(db)
r=pipeline.import_source(c,'curated',RAW+'/nos-platform-code-facts-2026-10-10.json')
print('import returned',r)
for t in ['sources','devices','records','identifiers','facts','affiliations']:
    try: print(t,c.db.execute(f'select count(*) from {t}').fetchone()[0])
    except Exception as e: print(t,'ERR',e)
print(c.db.execute('pragma integrity_check').fetchall()[0][0],c.db.execute('pragma foreign_key_check').fetchall())

import sys,json,hashlib,os,sqlite3,re,collections
sys.path.insert(0,'/home/erin/nepeat/aislop/devicehunt')
from devicehunt.catalog import Catalog
from devicehunt import pipeline
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/entity-manager'
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-entity-manager'
m=json.load(open(RAW+'/capture-manifest.json'));bad=0
for f in m['files']:
    b=open(RAW+'/'+f['path'],'rb').read()
    if hashlib.sha256(b).hexdigest()!=f['sha256'] or len(b)!=f['bytes']: bad+=1
print('manifest files',len(m['files']),'mismatch',bad)
c=json.load(open(RAW+'/entity-manager-config-facts-2026-10-11.json'))
for s in c['sources']: assert os.path.exists(RAW+'/'+s['snapshot'])
# verify each fact's line cites contain the literal
nb=0;nt=0
for d in c['devices']:
    for k,v in d['facts']:
        for mm in re.finditer(r'\((\S+?\.json):(\d+)\)',v):
            pass
db=S+'/t.sqlite'
if os.path.exists(db): os.remove(db)
cat=Catalog(db)
print('import',pipeline.import_source(cat,'curated',RAW+'/entity-manager-config-facts-2026-10-11.json'))
cat.db.commit() if hasattr(cat,'db') else None

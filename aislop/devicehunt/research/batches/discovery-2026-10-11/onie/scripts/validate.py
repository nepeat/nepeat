import json,hashlib,os,sys,sqlite3,re
sys.path.insert(0,'/home/erin/nepeat/aislop/devicehunt')
ROOT='/home/erin/nepeat/aislop/devicehunt'
RAW=ROOT+'/data/raw/discovery-2026-10-11-onie'
m=json.load(open(RAW+'/capture-manifest.json'));bad=0
for f in m['files']:
    b=open(f"{RAW}/{f['path']}",'rb').read()
    if hashlib.sha256(b).hexdigest()!=f['sha256'] or len(b)!=f['bytes']: bad+=1
disk=sum(len(fs) for _,_,fs in os.walk(RAW))
print('manifest files',len(m['files']),'mismatch',bad,'on-disk',disk,'(+2 = manifest+candidate)')
d=json.load(open(RAW+'/onie-machine-facts-2026-10-11.json'))
miss=[s['snapshot'] for s in d['sources'] if not os.path.isfile(RAW+'/'+s['snapshot'])]
print('sources',len(d['sources']),'devices',len(d['devices']),'snapshot missing',len(miss),'unique models',len({x['model'] for x in d['devices']}))
nf=sum(len(x['facts']) for x in d['devices']);mx=max(len(f[1]) for x in d['devices'] for f in x['facts']);print('facts',nf,'max fact len',mx)
from devicehunt.catalog import Catalog
from devicehunt import pipeline
db=S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/onie/t.sqlite'
if os.path.exists(db): os.remove(db)
c=Catalog(db)
print('import returned',pipeline.import_source(c,'curated',RAW+'/onie-machine-facts-2026-10-11.json'))
for t in ['sources','devices','records','identifiers','facts','affiliations']:
    print(t,c.db.execute(f'select count(*) from {t}').fetchone()[0])
print(c.db.execute('pragma integrity_check').fetchall()[0][0],c.db.execute('pragma foreign_key_check').fetchall())
# crossref read-only
r=sqlite3.connect('file:'+ROOT+'/data/devices.sqlite?mode=ro',uri=True)
print([ (t,[x[1] for x in r.execute(f'pragma table_info({t})')]) for t in ['identifiers','devices','device_facts']])

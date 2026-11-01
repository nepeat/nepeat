import json,hashlib,os,sys,shutil,sqlite3
sys.path.insert(0,'/home/erin/nepeat/aislop/devicehunt')
ROOT='/home/erin/nepeat/aislop/devicehunt'
RAW=ROOT+'/data/raw/discovery-2026-10-11-sonic-pddf'
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/sonic-pddf'
m=json.load(open(RAW+'/capture-manifest.json'));bad=0
for f in m['files']:
    b=open(f"{RAW}/{f['path']}",'rb').read()
    if hashlib.sha256(b).hexdigest()!=f['sha256'] or len(b)!=f['bytes']: bad+=1
disk=sum(len(fs) for _,_,fs in os.walk(RAW))
print('manifest files',len(m['files']),'mismatch',bad,'on-disk',disk)
cur=RAW+'/sonic-pddf-facts-2026-10-11.json'
d=json.load(open(cur))
print('snap missing',[s['snapshot'] for s in d['sources'] if not os.path.isfile(RAW+'/'+s['snapshot'])])
from devicehunt.catalog import Catalog
from devicehunt import pipeline
src=sqlite3.connect('file:%s/data/devices.sqlite?mode=ro'%ROOT,uri=True)
print('live db SONiC platform devices:',src.execute("select count(*) from devices where brand='SONiC platform'").fetchone(), 'cols', [r[1] for r in src.execute('pragma table_info(devices)')])
src.close()
def q(c,sql): return c.db.execute(sql).fetchall()
def run(name,files,copy=False):
    db=S+'/'+name
    if os.path.exists(db): os.remove(db)
    if copy: shutil.copyfile(ROOT+'/data/devices.sqlite',db)
    c=Catalog(db)
    before=q(c,"select count(*) from devices")[0][0]
    for f in files: print(' import',os.path.basename(f),pipeline.import_source(c,'curated',f))
    after=q(c,"select count(*) from devices")[0][0]
    return c,before,after
sonic=ROOT+'/data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json'
nos=ROOT+'/data/raw/hunt-2026-10-10-nos/nos-platform-code-facts-2026-10-10.json'
for label,files,copy in (('A fresh pddf only',[cur],False),('B fresh sonic+nos+pddf',[sonic,nos,cur],False),('C copy of live db + pddf',[cur],True)):
    print('==',label)
    c,b,a=run('t_%s.sqlite'%label[0],files,copy)
    print(' devices before',b,'after',a)
    print(' SONiC platform devices',q(c,"select count(*) from devices where brand='SONiC platform'")[0][0])
    pl=[x['model'] for x in d['devices']]
    dup=q(c,"select brand,model,count(*) from devices where brand='SONiC platform' group by 1,2 having count(*)>1")
    print(' duplicate brand/model identities',dup)
    for t in ['sources','devices','records','identifiers','facts','affiliations']:
        try: print(' ',t,q(c,f'select count(*) from {t}')[0][0])
        except Exception as e: print(' ',t,'ERR',e)
    # pddf facts attached to preexisting device?
    rows=q(c,"select d.id, d.model, count(distinct r.id) from devices d join records r on r.device_id=d.id where d.brand='SONiC platform' and d.model='x86_64-accton_as7726_32x-r0' group by d.id")
    print(' as7726 device rows (id, model, records)',rows)
    print(' integrity',q(c,'pragma integrity_check')[0][0],'fk',q(c,'pragma foreign_key_check'))

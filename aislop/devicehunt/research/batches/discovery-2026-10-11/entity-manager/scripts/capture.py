import json,os,re,hashlib,shutil,sys,subprocess
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad'
CL=S+'/openbmc/entity-manager'
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-entity-manager'
commit=subprocess.check_output(['git','-C',CL,'rev-parse','HEAD'],text=True).strip()
clone=open(S+'/openbmc/clone-time.txt').read().strip()
TERMS=re.compile(r'quanta|qct|inventec|wiwynn|wistron|celestica|ingrasys|foxconn|hon hai|honhai|inspur|hyve|accton|edgecore|edge-core|oracle|amazon',re.I)
VENDOR_DIRS={'foxconn-industrial-internet','ieisystem'}  # listed target names appearing as a vendor subdirectory
allf=[]
for r,d,f in os.walk(CL+'/configurations'):
    for n in f:
        if n.endswith('.json'): allf.append(os.path.relpath(os.path.join(r,n),CL))
allf.sort()
sel={}
for p in allf:
    hits=[]
    for i,l in enumerate(open(CL+'/'+p,encoding='utf-8',errors='replace'),1):
        for m in TERMS.finditer(l): hits.append((m.group(0),i))
    parts=p.split('/')
    vd=parts[1] if len(parts)>2 else None
    rules=[]
    if vd in VENDOR_DIRS: rules.append(('vendor_dir',vd))
    if hits: rules.append(('string_hits',hits))
    if rules: sel[p]=rules
json.dump({'total':len(allf),'all':allf,'selected':{k:v for k,v in sel.items()}},open('selection.json','w'),indent=1)
files=[]
extra=['LICENCE']
os.makedirs(RAW,exist_ok=True)
for p in list(sel)+extra:
    dst=RAW+'/'+p
    os.makedirs(os.path.dirname(dst),exist_ok=True)
    shutil.copyfile(CL+'/'+p,dst)
    b=open(dst,'rb').read()
    files.append({'path':p,'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b),'permalink':f'https://github.com/openbmc/entity-manager/blob/{commit}/{p}'})
man={'repo':'https://github.com/openbmc/entity-manager','commit':commit,'clone_utc':clone,
 'clone_utc_second':open(S+'/openbmc/clone-time2.txt').read().strip()+' (fresh sparse clone, same commit, to add configurations/ieisystem; first clone deleted)','clone_utc_third':open(S+'/openbmc/clone-time3.txt').read().strip()+' (fresh sparse clone, same commit, for all-config-paths.txt via git ls-tree; no files copied from it beyond a re-copy of the same bytes)','clone_method':'git clone --depth 1 --filter=blob:none --sparse; sparse-checkout set configurations (cone mode also materialises top-level files)',
 'license':'Apache-2.0 (top-level file is named LICENCE, "Copyright 2018 Intel Corporation ... Apache License, Version 2.0")',
 'configurations_total_json_files':len(allf),'selected_files':len(sel),
 'note':'The task named a top-level LICENSE; the repo file is spelled LICENCE.','files':files}
json.dump(man,open(RAW+'/capture-manifest.json','w'),indent=1)
print(len(allf),len(sel),commit)

import os,re,json,shutil,hashlib,collections,subprocess,sys
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad'
P='/home/erin/nepeat/aislop/devicehunt'
RAW=P+'/data/raw/discovery-2026-10-11-nos2'
repos={
 'stratum':dict(owner='stratum',name='stratum',clone=S+'/stratum/c',url='https://github.com/stratum/stratum',time=open(S+'/nos2/stratum-clone-time').read().strip()),
 'dent':dict(owner='dentproject',name='dentOS',clone=S+'/dentproject/c',url='https://github.com/dentproject/dentOS',time=open(S+'/nos2/dent-clone-time').read().strip()),
}
for r in repos.values():
    r['commit']=subprocess.check_output(['git','-C',r['clone'],'rev-parse','HEAD'],text=True).strip()
    r['license']=subprocess.check_output(['git','-C',r['clone'],'show','HEAD:LICENSE'],text=True,stderr=subprocess.DEVNULL).splitlines()[0:3] if True else None
SIZE_CAP=50*1024
man={'captured_by':'nos2 worker (Claude Sonnet 5.5)','repos':[],'files':[],'skipped_large':[]}
def cap(rk,rel):
    r=repos[rk]; src=os.path.join(r['clone'],rel)
    b=os.path.getsize(src)
    if b>SIZE_CAP:
        man['skipped_large'].append(dict(repo=r['url'],path=rel,bytes=b)); return None
    loc=f"{r['owner']}/{r['name']}/{r['commit']}/{rel}"
    dst=os.path.join(RAW,loc); os.makedirs(os.path.dirname(dst),exist_ok=True); shutil.copyfile(src,dst)
    data=open(dst,'rb').read()
    man['files'].append(dict(repo=r['url'],path=loc,upstream_path=rel,sha256=hashlib.sha256(data).hexdigest(),bytes=len(data),
      permalink=f"{r['url']}/blob/{r['commit']}/{rel}"))
    return loc
for k,r in repos.items():
    man['repos'].append(dict(url=r['url'],commit=r['commit'],clone_utc=r['time'],license_head=r['license']))
# ---------- stratum
SC='stratum/hal/config'
sel_prefix=('x86-64-accton-','x86-64-inventec-','x86-64-quanta-')
allcfg=sorted(d for d in os.listdir(repos['stratum']['clone']+'/'+SC) if os.path.isdir(repos['stratum']['clone']+'/'+SC+'/'+d))
import sqlite3
db0=sqlite3.connect('file:'+P+'/data/devices.sqlite?mode=ro',uri=True)
cat0={}
for k,v in db0.execute("select kind,value from identifiers where kind in ('onl_platform','onie_platform','onie_machine','sonic_platform')"): cat0.setdefault(v,[]).append(k)
sel=[d for d in allcfg if d.startswith(sel_prefix)]
devices=[];sources=[];ext={}
def lines(path): return open(path,encoding='utf-8').read().split('\n')
def L(fn,i): return f"{fn}:{i}"
for d in sel:
    base=f'{SC}/{d}'; fs={}
    for f in ('chassis_config.pb.txt','phal_config.pb.txt','base_bcm_chassis_map.pb.txt'):
        p=repos['stratum']['clone']+'/'+base+'/'+f
        if os.path.exists(p): fs[f]=cap('stratum',base+'/'+f)
    facts=[('stratum_commit',repos['stratum']['commit']),('stratum_dir_as_printed',d)]
    present=[f for f,v in fs.items() if v]
    facts.append(('stratum_files_captured',json.dumps(present)))
    skipped=[x['path'].split('/')[-1] for x in man['skipped_large'] if x['path'].startswith(base+'/')]
    allfiles=sorted(os.listdir(repos['stratum']['clone']+'/'+base))
    facts.append(('stratum_files_in_dir',json.dumps(allfiles)))
    if skipped: facts.append(('stratum_files_not_captured_over_50KB',json.dumps(skipped)))
    if fs.get('chassis_config.pb.txt'):
        fn='chassis_config.pb.txt'; ls=lines(os.path.join(RAW,fs[fn]))
        for i,l in enumerate(ls,1):
            m=re.match(r'description: (".*")$',l)
            if m: facts.append(('stratum_chassis_description',f'{m.group(1)} ({L(fn,i)})'))
            m=re.match(r'  platform: (\S+)$',l)
            if m: facts.append(('stratum_chassis_platform_enum',f'{m.group(1)} ({L(fn,i)})'))
            m=re.match(r'  name: (".*")$',l)
            if m and i<12: facts.append(('stratum_chassis_name',f'{m.group(1)} ({L(fn,i)})'))
        n=[i for i,l in enumerate(ls,1) if l=='nodes {']
        sp=[i for i,l in enumerate(ls,1) if l=='singleton_ports {']
        facts.append(('stratum_node_count',f'{len(n)} ({L(fn,n[0]) if n else fn})'))
        sp_speed=collections.Counter(); names=[]
        for i in sp:
            blk=[]
            j=i
            while ls[j]!='}': blk.append(ls[j]); j+=1
            for b in blk:
                m=re.match(r'  speed_bps: (\d+)',b)
                if m: sp_speed[m.group(1)]+=1
                m=re.match(r'  name: (".*")',b)
                if m: names.append(m.group(1).strip('"'))
        facts.append(('stratum_singleton_port_count',f'{len(sp)} (first {L(fn,sp[0])}, last {L(fn,sp[-1])})'))
        facts.append(('stratum_singleton_port_speeds_bps',json.dumps(dict(sp_speed))+f' ({fn} speed_bps lines in singleton_ports)'))
        facts.append(('stratum_singleton_port_names_first_last',f'{names[0]} .. {names[-1]} ({L(fn,sp[0])})'))
    if fs.get('phal_config.pb.txt'):
        fn='phal_config.pb.txt'; ls=lines(os.path.join(RAW,fs[fn]))
        top={}
        for i,l in enumerate(ls,1):
            if re.match(r'^[a-z_]+ \{$',l): top[l[:-2]]=i
        keys=list(top)
        def section(k):
            s=top[k]; e=next((top[x] for x in keys if top[x]>s),len(ls)+1); return s,e
        if 'cards' in top:
            s,e=section('cards'); ty=collections.Counter(); np=0
            for l in ls[s:e-1]:
                if re.match(r'  ports \{',l): np+=1
                m=re.match(r'    physical_port_type: (\S+)',l)
                if m: ty[m.group(1)]+=1
            facts.append(('stratum_phal_card_port_count',f'{np} ({L(fn,s)})'))
            facts.append(('stratum_phal_physical_port_types',json.dumps(dict(ty))+f' ({fn}:{s}-{e-1})'))
        for k,fk,pat in (('fan_trays','stratum_phal_fan_count',r'  fans \{'),('psu_trays','stratum_phal_psu_count',r'  psus \{'),('led_groups','stratum_phal_led_count',r'  leds \{'),('thermal_groups','stratum_phal_thermal_count',r'  thermals \{')):
            if k in top:
                s,e=section(k); c=sum(1 for l in ls[s:e-1] if re.match(pat,l))
                facts.append((fk,f'{c} ({L(fn,s)})'))
        facts.append(('stratum_phal_top_level_sections',json.dumps(keys)+f' ({fn})'))
    if fs.get('base_bcm_chassis_map.pb.txt'):
        fn='base_bcm_chassis_map.pb.txt'; ls=lines(os.path.join(RAW,fs[fn]))
        for i,l in enumerate(ls,1):
            m=re.match(r'  id: (".*")$',l)
            if m: facts.append(('stratum_bcm_chassis_map_id',f'{m.group(1)} ({L(fn,i)})'))
        ct=collections.Counter(); first={}
        for i,l in enumerate(ls,1):
            m=re.match(r'    type: (\S+)$',l)
            if m and re.match(r'  bcm_chips \{',ls[i-2]):
                ct[m.group(1)]+=1; first.setdefault(m.group(1),i)
        if ct: facts.append(('stratum_bcm_chip_types',json.dumps(dict(ct))+f' (first {fn}:{min(first.values())})'))
    if d in cat0: facts.append(('stratum_lead_equals_catalog_identifier',json.dumps(sorted(cat0[d]))+' (exact string match on identifiers.value; lead only, no merge)'))
    sid='stratum-platform-'+d
    sources.append(dict(id=sid,url=f"{repos['stratum']['url']}/tree/{repos['stratum']['commit']}/{base}",tier='project',
      license='Apache-2.0 / upstream rights; factual extraction',
      coverage=f"Software platform/chassis config at stratum/stratum commit {repos['stratum']['commit']}; not a hardware datasheet, vendor support statement, installed configuration or proof of a model's existence or variant. Counts are what the Stratum config files declare; example-config names/descriptions are as printed upstream. Directory names imply no company role.",
      snapshot=fs['chassis_config.pb.txt']))
    devices.append(dict(source=sid,key=d,brand='Stratum platform',model=d,title='Stratum platform '+d,kind='network switch software platform config',
      affiliations=[],identifiers=[['stratum_platform',d]],facts=[list(x) for x in facts]))
# ---------- dent
DC=repos['dent']['clone']+'/packages/platforms'
onl=set(x['model'] for x in json.load(open(P+'/data/raw/hunt-2026-10-10-nos/nos-platform-code-facts-2026-10-10.json'))['devices'] if 'ONL' in x['brand'])
sonic_ids=set(x['model'] for x in json.load(open(P+'/data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json'))['devices'])
idf=[l.strip() for l in open(S+'/nos2/dent-identity-files.txt') if 'accton/' in l]
byplat=collections.defaultdict(dict)
for rel in idf:
    full='packages/platforms/'+rel
    kind='yml' if rel.endswith('.yml') else 'py'
    byplat[rel.split('/platform-config/')[0]][kind]=full
dent_rows=[];overlap=[];dent_only=[]
import sqlite3
db=sqlite3.connect('file:'+P+'/data/devices.sqlite?mode=ro',uri=True)
cat={}
for k,v in db.execute("select kind,value from identifiers where kind in ('onl_platform','onie_platform','onie_machine','sonic_platform')"): cat.setdefault(v,[]).append(k)
seen={}
for pdir,fl in sorted(byplat.items()):
    locs={k:cap('dent',v) for k,v in fl.items()}
    facts=[('dent_commit',repos['dent']['commit']),('dent_vendor_dir_as_printed','accton'),('dent_platform_dir',pdir),('dent_files_captured',json.dumps([fl[k] for k in sorted(fl)]))]
    plat=None
    if locs.get('py'):
        fn=os.path.basename(fl['py']) ; fn='platform-config/'+fl['py'].split('/platform-config/')[1]
        ls=lines(os.path.join(RAW,locs['py']))
        for i,l in enumerate(ls,1):
            m=re.match(r"\s+PLATFORM\s*=\s*(.+?)\s*$",l)
            if m: plat=m.group(1).strip('\'"'); facts.append(('dent_platform_literal',f'{m.group(1)} ({fn}:{i})'))
            m=re.match(r"\s+MODEL\s*=\s*(.+?)\s*$",l)
            if m: facts.append(('dent_model_literal',f'{m.group(1)} ({fn}:{i})'))
            m=re.match(r"\s+SYS_OBJECT_ID\s*=\s*(.+?)\s*$",l)
            if m: facts.append(('dent_sys_object_id',f'{m.group(1)} ({fn}:{i})'))
            m=re.match(r"\s+PORT_COUNT\s*=\s*(.+?)\s*$",l)
            if m: facts.append(('dent_port_count_literal',f'{m.group(1)} ({fn}:{i})'))
            m=re.match(r"class (\w+)\((.*)$",l)
            if m:
                bases=[m.group(2)]
                j=i
                while ')' not in ls[j-1] and j<len(ls): j+=1; bases.append(ls[j-1].strip())
                txt=' '.join(bases); pc=re.findall(r'OnlPlatformPortConfig_\w+',txt)
                facts.append(('dent_python_class',f'{m.group(1)} ({fn}:{i})'))
                if pc: facts.append(('dent_port_config_class',f'{pc[0]} ({fn}:{i})'))
    if locs.get('yml'):
        fn='platform-config/'+fl['yml'].split('/platform-config/')[1]
        ls=lines(os.path.join(RAW,locs['yml']))
        for i,l in enumerate(ls,1):
            m=re.match(r'([a-z0-9_-]+):\s*$',l)
            if m and not l.startswith(' ') and m.group(1)!='---': facts.append(('dent_yml_platform_key',f'{m.group(1)} ({fn}:{i})')); plat=plat or m.group(1)
            m=re.match(r'\s*#\s*platform-config for (.+?)\s*$',l)
            if m: facts.append(('dent_yml_header_comment',f'{m.group(0).strip()} ({fn}:{i})'))
            m=re.match(r'\s+--speed=(\d+)',l)
            if m: facts.append(('dent_grub_serial_speed',f'{m.group(1)} ({fn}:{i})'))
    if not plat: raise SystemExit('no platform '+pdir)
    arch=pdir.split('/')[1]
    facts.append(('dent_arch_dir',arch))
    status='overlap_onl_nos' if plat in onl else ('not_in_nos_capture_arch_outside_x86-64' if arch!='x86-64' else 'dent_only_x86-64')
    facts.append(('dent_vs_onl_nos_capture',status))
    if plat in cat: facts.append(('dent_lead_equals_catalog_identifier',json.dumps(sorted(cat[plat]))+' (exact string match on identifiers.value; lead only, no merge)'))
    if plat in seen:
        seen[plat]['facts']+= [list(x) for x in facts if x[0] in ('dent_platform_dir','dent_files_captured','dent_arch_dir')]
        continue
    (overlap if plat in onl else dent_only).append(plat)
    lead=[]
    if plat in sonic_ids: lead.append('sonic')
    sid='dent-platform-'+plat
    sources.append(dict(id=sid,url=f"{repos['dent']['url']}/tree/{repos['dent']['commit']}/{pdir}",tier='project',license='Eclipse Public License 1.0 / Apache-2.0 (ONL-derived); upstream rights; factual extraction',
      coverage=f"Software platform definition at dentproject/dentOS commit {repos['dent']['commit']} (ONL-derived); not a hardware datasheet, vendor support statement, installed configuration or proof of a model's existence or variant. Directory/vendor names imply no company role.",
      snapshot=(locs.get('yml') or locs['py'])))
    devices.append(dict(source=sid,key=plat,brand='DENT platform',model=plat,title='DENT platform '+plat,kind='network switch software platform definition',
      affiliations=[],identifiers=[['dent_platform',plat]],facts=[list(x) for x in facts]))
    seen[plat]=devices[-1]
json.dump(man,open(RAW+'/capture-manifest.json','w'),indent=1)
json.dump(dict(sources=sources,devices=devices),open(RAW+'/nos2-platform-facts-2026-10-11.json','w'),indent=1)
json.dump(dict(stratum_all=allcfg,stratum_sel=sel,dent_overlap=overlap,dent_only=dent_only),open(S+'/nos2/summary.json','w'),indent=1)
print(len(sources),len(devices),sum(len(d['facts']) for d in devices),len(man['files']),man['skipped_large'])
print('overlap',len(overlap),'other',dent_only)

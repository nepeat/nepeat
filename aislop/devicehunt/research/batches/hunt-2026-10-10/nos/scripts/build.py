import json,re,collections,sys,os
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-nos'
OUTR='/home/erin/nepeat/aislop/devicehunt/research/batches/hunt-2026-10-10/nos'
man=json.load(open(RAW+'/capture-manifest.json'))
items=json.load(open(sys.argv[1]))['items']
acc=json.load(open('/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json'))
ids={d['model'] for d in acc['devices']}
commits={r['repo']:r['commit'] for r in man['repos']}
SC=commits['sonic-net/sonic-buildimage'];OC=commits['opencomputeproject/OpenNetworkLinux']
fmeta={f['path']:f for f in man['files']}
def plat(path):
    q=path.split('/')
    if path.startswith('platform/'): return ('sonic',q[2].replace('sonic-platform-modules-',''),q[3],'/'.join(q[:4]))
    return ('onl',q[2],q[4],'/'.join(q[:5]))
groups=collections.defaultdict(list); gfiles=collections.defaultdict(list)
for f in man['files']:
    gfiles[plat(f['path'])].append(f['path'])
for o in items: groups[plat(o['path'])].append(o)
AIRCTX=re.compile(r'airflow|direction|fan_dir',re.I);MODCTX=re.compile(r'model|part|prod|vendor|manufact|brand',re.I)
JUNK={'N/A','Unknown','','NA'}
def rel(o,base): return o['path'][len(base)+1:]
def loc(o,base): return f"{rel(o,base)}:{o['line']}"
def buildfacts(key,base,its):
    F=collections.OrderedDict()
    def put(n,lst):
        lst=list(dict.fromkeys(lst))
        if lst: F[n]=json.dumps(lst,ensure_ascii=False)
    L=lambda o:loc(o,base)
    put('nos_count_constants',[f"{o['name']} = {o['value']} ({L(o)})" for o in its if o['kind'] in('count_const','count_define')])
    mp=[];pm=[];ps=[]
    for o in its:
        k=o['kind']
        if k=='compare_literal' and o['path'].endswith('/fan.py') and o['against']=='model' and 'cel' in o['path']:
            pm.append(f"\"{o['literal']}\" compared with `model` in the is_psu_fan branch (model from psu.get_model() / FRU 'Board Part Number'; i.e. PSU model) ({L(o)})")
        elif k=='compare_literal' and o['against']=='prod_name': ps.append(f"\"{o['literal']}\" compared with `prod_name` (substring/equality test on product name read in psu.py) ({L(o)})")
        elif k=='compare_literal' and MODCTX.search(o['against']) and o['literal'].strip(): mp.append(f"\"{o['literal']}\" compared with `{o['against']}` ({L(o)})")
        elif k=='c_compare_literal' and re.search(r'model|Model|Serial',o['statement']+o['literal']) : mp.append(f"\"{o['literal']}\" in `{o['statement']}` ({L(o)})")
        elif k=='c_string_array' and not (o['array']=='path_name'): mp.append(f"\"{o['literal']}\" in array `{o['array']}` ({L(o)})")
    put('nos_model_part_literals',mp)
    put('nos_psu_model_literals_in_fan_direction_code',pm)
    put('nos_psu_name_substring_matched',ps)
    al=[];cov_=set()
    for o in its:
        if o['kind'] in('compare_literal','c_compare_literal') : cov_.add((o['path'],o['line'],o['literal']))
        if o['kind']=='assign_literal': cov_.add((o['path'],o['line'],o['literal']))
    for o in its:
        k=o['kind']
        if k=='compare_literal' and AIRCTX.search(o['against']) : al.append(f"\"{o['literal']}\" compared with `{o['against']}` ({L(o)})")
        elif k=='c_compare_literal' and o['literal'] in('B2F','F2B','AFO','AFI'): al.append(f"\"{o['literal']}\" in `{o['statement']}` ({L(o)})")
        elif k=='airflow_literal' and (o['path'],o['line'],o['literal']) not in cov_: al.append(f"\"{o['literal']}\" in `{o['line_text']}` ({L(o)})")
        elif k=='assign_literal' and o['literal'] not in JUNK and not o['literal'].startswith('/') and (AIRCTX.search(o['target'])): al.append(f"{o['target']} = \"{o['literal']}\" ({L(o)})")
    put('nos_airflow_literals',al)
    sym=collections.OrderedDict()
    for o in its:
        if o['kind']=='airflow_token_line': sym.setdefault((o['path'],o['text'][:140]),[]).append(o)
    put('nos_airflow_symbol_lines',[f"`{t}` ({v[0]['path'].split('/')[-1]}:{','.join(str(x['line']) for x in v)})" for (p,t),v in sym.items()])
    def clab(o):
        t=o['text']
        if re.match(r'#?\s*[#]?\s*[\w.\[\]]+\s*=\s*[\w.(]',t) or re.match(r'#\s*(if|elif|return|for)\b',t): return 'commented-out code'
        oth=[x for x in re.findall(r'AS\d{4}',t,re.I) if x.lower() not in key[2].lower().replace('-','')]
        if oth: return 'comment naming a different platform ('+oth[0]+')'
        return 'comment'
    put('nos_airflow_comments',[f"{clab(o)}: {o['text']} ({L(o)})" for o in its if o['kind']=='comment_airflow'])
    nu=[rel(o,base) for o in its if o['kind']=='non_utf8_file']
    if nu: F['nos_non_utf8_files']=json.dumps([x+' (bytes >0x7f shown as \\xNN escapes in quoted text)' for x in nu])
    ee=[]
    for o in its:
        if o['kind']=='assign_literal' and o['path'].endswith('/eeprom.py') and o['target']=='name' and o['literal'] not in JUNK: ee.append(f"name = \"{o['literal']}\" ({L(o)})")
        elif o['kind']=='c_string_array' and o['array']=='path_name': ee.append(f"\"{o['literal']}\" in array `path_name` ({L(o)})")
        elif o['kind']=='assign_literal' and re.search(r'FRU|KEY',o['target']): ee.append(f"{o['target']} = \"{o['literal']}\" ({L(o)})")
    put('nos_eeprom_field_literals',ee)
    dc=[]
    for o in its:
        k=o['kind']
        if k=='assign_literal' and o['target'] in('PLATFORM','MODEL','SYS_OBJECT_ID'): dc.append(f"{o['target']} = \"{o['literal']}\" ({L(o)})")
        elif k=='c_platform_literal': dc.append(f"\"{o['literal']}\" in `{o['statement']}` ({L(o)})")
        elif k=='define_literal': dc.append(f"#define {o['name']} \"{o['literal']}\" ({L(o)})")
        elif k=='assign_literal' and o['literal'] not in JUNK and MODCTX.search(o['target']) and not AIRCTX.search(o['target']) and not o['path'].endswith('/eeprom.py'): dc.append(f"{o['target']} = \"{o['literal']}\" ({L(o)})")
        elif k=='c_model_literal_line' and re.search(r'model|serial|part|vendor|mfr|manufact|Model:',o['literal'],re.I) and '%[' not in o['literal'] and 'path' not in o['literal'].lower(): dc.append(f"\"{o['literal']}\" in `{o['statement']}` ({L(o)})")
    put('nos_declared_literals',dc)
    return F
LIC={'sonic':'Apache-2.0 (sonic-net/sonic-buildimage repository LICENSE)',
     'onl':'EPL-1.0 (repository LICENSE: "Unless otherwise noted, the following license applies": Eclipse Public License, Version 1.0; Debian/Broadcom-derived components keep their own licences; many captured files carry EPL-1.0 headers)'}
def gpl_files(files):
    r=[]
    for p in files:
        if 'GNU General Public License' in open(f"{RAW}/{fmeta[p]['local']}",errors='replace').read(): r.append(p.split('/')[-1])
    return r
sources=[];devices=[];ext=[];unmapped=[];dirs_nothing=[]
for key in sorted(gfiles):
    kind,vend,d,base=key
    its=groups.get(key,[])
    files=sorted(gfiles[key])
    if kind=='sonic':
        pid=f"x86_64-{vend}_{d.replace('-','_')}-r0"
        if pid not in ids:
            unmapped.append({'vendor_module':vend,'dir':d,'files':len(files),'items':len(its)}); continue
        model=pid;brand='SONiC platform';repo='sonic-net/sonic-buildimage';c=SC;extra=[]
        sid=f'nos-sonic-{pid}'
    else:
        pl=sorted({re.search(r'"([^"]+)"',x['literal'] and f'"{x["literal"]}"').group(1) for x in its if x['kind']=='assign_literal' and x['target']=='PLATFORM'})
        if not pl: unmapped.append({'onl_dir':base,'reason':'no PLATFORM literal'}); continue
        model=pl[0];extra=pl[1:];brand='ONL platform';repo='opencomputeproject/OpenNetworkLinux';c=OC
        sid=f'nos-onl-{model}'
    bd=base
    F=buildfacts(key,bd,its)
    pref=[p for p in files if p.endswith(('/psu.py','/psui.c'))] or files
    snap=f"{repo.split('/')[1]}/{c}/{pref[0]}"
    facts=[['nos_repo_commit',f'{repo}@{c}'],['nos_source_files',json.dumps([p[len(bd)+1:] for p in files])]]+[[k,v] for k,v in F.items()]
    cov=(f"Platform-support code at {repo} commit {c}, directory {bd}: the strings, constants and symbol lines below are what the code matches or declares, "
         "not vendor specifications, installed configurations, or proof that a given unit ships with that PSU or fan. Only the listed files were captured; "
         "a fact is omitted when nothing of that kind was extracted from them (silent, not zero). Extraction is literal and role-based (comparison, array, assignment or define); unmapped strings are not interpreted. "
         "Directory and vendor-module names imply no company role.")
    sources.append({'id':sid,'url':f'https://github.com/{repo}/tree/{c}/{bd}','tier':'project','license':LIC[kind]+(('; captured files with GPL notice headers: '+', '.join(gpl_files(files))) if gpl_files(files) else ''),'coverage':cov,'snapshot':snap})
    ident=[['sonic_platform' if kind=='sonic' else 'onl_platform',model]]+[['onl_platform',e] for e in extra]
    devices.append({'source':sid,'key':model,'brand':brand,'model':model,'title':f'{brand} {model}','kind':'network switch software platform definition','affiliations':[],'identifiers':ident,'facts':facts})
    ext.append({'platform':model,'source':sid,'dir':bd,'files':len(files),'facts':{k:len(json.loads(v)) for k,v in F.items()}})
json.dump({'sources':sources,'devices':devices},open(RAW+'/nos-platform-code-facts-2026-10-10.json','w'),indent=1,ensure_ascii=False)
os.makedirs(OUTR,exist_ok=True)
json.dump({'commits':commits,'extractor':'scripts/extract.py + build.py (scratchpad nos/scripts, run python3 -I)','unmapped_or_skipped_platform_dirs':unmapped,'per_platform_summary':ext,'items':items},open(OUTR+'/extraction.json','w'),indent=0)
print(len(sources),len(unmapped),unmapped)
c=collections.Counter(); 
for d in devices:
    for k,v in d['facts']: c[k]+=1
print(c)

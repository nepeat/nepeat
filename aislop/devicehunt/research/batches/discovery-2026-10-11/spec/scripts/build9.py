import sys,json,re,glob,os,hashlib,collections
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec'
sys.path.insert(0,S)
from parse_txt import parse
R='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-spec'
reqs={}
for l in open(R+'/wave9/requests-wave9.jsonl'):
    d=json.loads(l); reqs[d['url']]=d
def san(l):
    l=l.replace('(s)','s').replace('#','number'); l=re.sub(r'[^a-z0-9]+','_',l.lower()).strip('_'); return l
def lines(p): return open(p,encoding='utf-8',errors='replace').read().split('\n')
sources=[];devices=[];skipped=[]
sys.path.insert(0,'/home/erin/nepeat/aislop/devicehunt')
from devicehunt.catalog import target_for
KW=re.compile(r'intel|amd|xeon|epyc|hygon|ampere|sparc|ghz|\bcores?\b|opteron|processor|\bcpu\b',re.I)
STRIPPED=[];KEPT_PAREN=[]
def ml_for(facts,evid):
    for e in evid:
        if e['fact_name'] in ('spec_model',): return e['line']

LIC="SPEC published result (full disclosure text); original rights of SPEC and the submitting company retained; local factual extraction"
def mk(sid,url,path,bench,field_cov,brand,model,facts,evid,idx_note):
    sha=hashlib.sha256(open(R+'/'+path,'rb').read()).hexdigest()
    printed_model=model
    fd=dict(facts); sponsor=re.sub(r'\s+',' ',fd.get('spec_test_sponsor','')).strip()
    role='brand' if sponsor==re.sub(r'\s+',' ',brand).strip() else 'reported_system_vendor'
    canon=target_for(brand) or brand
    base=printed_model
    m=re.search(r'\s*\(([^()]*)\)\s*$',printed_model)
    if m and KW.search(m.group(1)) and m.start()>0:
        base=printed_model[:m.start()]; STRIPPED.append((printed_model,base))
    elif '(' in printed_model: KEPT_PAREN.append(printed_model)
    if not any(n=='spec_system_name' for n,_ in facts):
        facts.append(['spec_system_name',printed_model]); evid.append({'fact_name':'spec_system_name','literal_quote':printed_model,'line':ml_for(facts,evid),'source_url':url,'capture_sha256':sha})
    facts[:]=[f for f in facts if f[1]!='None']
    evid[:]=[e for e in evid if any(f[0]==e['fact_name'] for f in facts)]
    kind='virtual machine configuration' if printed_model.startswith('1-Chip VM') else 'server'
    allone=[v for n,v in facts if n.startswith('spec_result_specrate') or n.startswith('spec_result_specspeed')]
    note=''
    if allone and all(v=='1.00' for v in allone): note=' All printed result ratios for this result are 1.00 as printed; the text does not state why, and they are not a performance claim for the system.'
    if kind!='server': note+=' The printed configuration is a virtual machine (Orderable: 1-16 CMIOU (on host)), not a hardware product.'
    if any(n.startswith('spec_shared_') for n,_ in facts): note+=' Multi-node result: spec_shared_* facts come from the SHARED HARDWARE block (enclosure); the per-node block repeats the PSU quantity/rating.'
    rule=("Identity rules: brand = the catalog's reviewed TARGETS alias name when the printed Hardware Vendor is a reviewed alias, otherwise the Hardware Vendor exactly as printed"+(f" (here printed '{brand}' -> '{canon}')" if canon!=brand else f" (here '{brand}' as printed)")+f"; model = the printed system name with only a trailing parenthesised CPU/config group removed (here '{printed_model}' -> '{base}'; the full printed name is kept as fact and identifier spec_system_name); affiliation = [Hardware Vendor as printed, '{role}'] where role is brand when Test Sponsor equals Hardware Vendor, else reported_system_vendor. This is a submitter's printed system/vendor label for this result, not a manufacturer, OEM or operator claim and not proof of manufacture or deployment.")
    sources.append({'id':sid,'url':url,'tier':'vendor','license':LIC,'snapshot':path,
      'coverage':f"SPEC {bench} full-disclosure text result as published at {url} (HTTP 200, capture sha256 {sha}). Vendor-submitted system description published by SPEC; values quoted as printed with wrapped lines joined by single spaces; fact line numbers are 1-based lines of the captured .txt (see evidence); fields printed as None are omitted. {field_cov} {rule} Test sponsor and Tested by are recorded as facts, not roles.{note} {idx_note}"})
    devices.append({'source':sid,'key':printed_model,'brand':canon,'model':base,'title':canon+' '+base,'kind':kind,
      'affiliations':[[brand,role]],'identifiers':[['spec_system_name',printed_model]],'facts':facts,'evidence':evid})
# ---- power
idx=json.load(open(S+'/power_rows.json'))
byurl={r['_links']['Text']:r for r in idx}
for p in sorted([]):
    fn=os.path.basename(p); rel='power_ssj2008/'+fn
    url=[u for u in reqs if u.endswith('/'+fn)][0]
    if reqs[url]['status']!=200: skipped.append((url,reqs[url]['status'])); continue
    L=lines(p); o=parse(p); st=o['sets'][0]; F={l:(v,i) for l,v,i in st['fields']}
    hdr={}
    lastk=None
    for i,l in enumerate(L[:25],1):
        m=re.match(r'^\s+(Test Sponsor|Tested By|Hardware Availability|Test Method|System Designation|Publication|Test Date):\s{2}(.*?)\s*$',l)
        if m: hdr[m.group(1)]=(m.group(2),i); lastk=m.group(1)
        elif lastk and re.match(r'^ {20,}\S',l) and ':' not in l[:40]: hdr[lastk]=(hdr[lastk][0]+' '+l.strip(),hdr[lastk][1])
        else: lastk=None
    head=None
    for i,l in enumerate(L[:12],1):
        if l.startswith('SPECpower_ssj2008 ='): head=(l.strip(),i)
    brand,bl=F['Hardware Vendor']; model,ml=F['Model']
    facts=[];ev=[]
    def add(name,val,line):
        facts.append([name,val]); ev.append({'fact_name':name,'literal_quote':val,'line':line,'source_url':url,'capture_sha256':reqs[url]['sha256']})
    add('spec_benchmark','SPECpower_ssj2008',1)
    add('spec_result_headline',head[0],head[1])
    add('spec_hardware_vendor',brand,bl); add('spec_model',model,ml)
    for k,n in [('Test Sponsor','spec_test_sponsor'),('Tested By','spec_tested_by'),('Hardware Availability','spec_hardware_availability'),('System Designation','spec_system_designation'),('Test Method','spec_test_method')]:
        if k in hdr: add(n,*hdr[k])
    for k in ['# of Identical Nodes','Form Factor','CPU Name','CPU Characteristics','CPU(s) Enabled','CPU(s) Orderable','Hardware Threads','Memory Amount (GB)','# and size of DIMM','Memory Details','Power Supply Quantity and Rating (W)','Power Supply Details']:
        if k in F: add('spec_'+san(k),*F[k])
    sh=[i for i,l in enumerate(L,1) if l.strip()=='SHARED HARDWARE']
    if sh:
        k=sh[0]; pend=None; blk=[]
        while k<len(L) and not (blk and L[k].strip()=='' and L[k+1].strip()==''):
            l=L[k]; k+=1
            m=re.match(r'^\s+(\S.*?):\s{2,}(\S.*?)\s*$',l)
            if m: blk.append([m.group(1),m.group(2),k]); continue
            m=re.match(r'^\s+(\S.*?)\s{2,}(\S.*?)\s*$',l)
            if m and not l.strip().endswith(':'): blk.append([m.group(1),m.group(2),k]); continue
            m=re.match(r'^\s+(\(W\)):\s*$',l)
            if m and blk: blk[-1][0]+=' (W)'
        for lab,v,i in blk:
            if lab in ('Enclosure','Form Factor','Power Supply Quantity and Rating (W)','Power Supply Details'):
                add('spec_shared_'+san(lab),v,i)
    add('spec_result_url',url,0)
    mk('spec-'+fn[:-4],url,rel,'SPECpower_ssj2008','Fields extracted: hardware vendor, model, test sponsor, tested by, availability, nodes, form factor, CPU, memory, power supply quantity/rating and details, headline metric. Power measurements per load level, software, notes and power-analyzer data are not extracted.',brand,model,facts,ev,
       f"Listed in the SPEC all-results index (index/power_ssj2008-dir.html) under hardware vendor / sponsor cell '{byurl['/'+url.split('.org/')[1]]['wkld_ssj_global_config_hw_vendor']}'.")
# ---- cpu2017
cidx=json.load(open(S+'/cpu_rows.json'))
cby={r['_links']['Text']:r for r in cidx}
for p in sorted(glob.glob(R+'/wave9/cpu2017/*.txt')):
    fn=os.path.basename(p); rel='wave9/cpu2017/'+fn
    url=[u for u in reqs if u.endswith('/'+fn)][0]
    if reqs[url]['status']!=200: skipped.append((url,reqs[url]['status'])); continue
    row=cby['/'+url.split('.org/')[1]]; hw_model=row['hw_model']
    L=lines(p)
    title=L[0].strip()
    blk=[]
    for l in L[1:8]:
        if l.strip().startswith('Test Sponsor:') or not l.strip(): break
        blk.append(l.strip())
    sysline=re.sub(r'\s+',' ',' '.join(blk)); hw_model=re.sub(r'\s+',' ',hw_model)
    if not sysline.endswith(hw_model) or len(sysline)==len(hw_model):
        skipped.append((url,'title/index model mismatch: '+sysline+' | '+hw_model)); continue
    brand=sysline[:-len(hw_model)].rstrip()
    if not sysline.startswith(brand+' '): skipped.append((url,'prefix')); continue
    model=hw_model
    facts=[];ev=[]
    def add(name,val,line):
        facts.append([name,val]); ev.append({'fact_name':name,'literal_quote':val,'line':line,'source_url':url,'capture_sha256':reqs[url]['sha256']})
    add('spec_benchmark',title,1)
    add('spec_system_title_line',sysline,2 if len(blk)==1 else '2-%d'%(1+len(blk)))
    ls2=2 if len(blk)==1 else '2-%d'%(1+len(blk)); add('spec_hardware_vendor',brand,ls2); add('spec_system_name',model,ls2)
    # header lines 3-8
    for i,l in enumerate(L[:10],1):
        for m in re.finditer(r'(Test Sponsor|Test sponsor|Tested by|Hardware availability|Test date):\s+(.*?)(?=\s{3,}\S+[ a-z]*:\s|\s*$)',l):
            pass
    for i,l in enumerate(L[:10],1):
        m=re.search(r'Test sponsor:\s+(.*?)\s{2,}Hardware availability:\s+(.*?)\s*$',l)
        if m: add('spec_test_sponsor',m.group(1),i); add('spec_hardware_availability',m.group(2),i)
        m=re.search(r'Tested by:\s+(.*?)\s{2,}Software availability:\s+(.*?)\s*$',l)
        if m: add('spec_tested_by',m.group(1),i)
    # sections
    sec=None;last=None
    for i,l in enumerate(L,1):
        s=l.rstrip()
        m=re.fullmatch(r'\s+(HARDWARE|SOFTWARE|POWER|POWER SETTINGS|POWER-RELEVANT HARDWARE|POWER ANALYZER|TEMPERATURE METER|CONTROLLER|NOTES|[A-Z][A-Z -]+)',s)
        if m and L[i:i+1] and set(L[i].strip())=={'-'}: sec=m.group(1); last=None; continue
        if sec not in ('HARDWARE','POWER-RELEVANT HARDWARE'): continue
        m=re.match(r'^\s*([^:]{1,24}):\s?(.*)$',s)
        if m and len(s)-len(s.lstrip())<22 and m.group(1).strip():
            lab=m.group(1).strip(); val=m.group(2).strip()
            last=[lab,val,i,sec]
            if sec=='HARDWARE' and lab in ('CPU Name','Enabled','Orderable','Memory','Max MHz','Nominal','Storage'):
                facts.append(['spec_'+san(lab),val]); ev.append({'fact_name':'spec_'+san(lab),'literal_quote':val,'line':i,'source_url':url,'capture_sha256':reqs[url]['sha256']}); last=facts[-1]
            elif sec=='POWER-RELEVANT HARDWARE' and lab in ('Power Supply','Details'):
                n='spec_power_supply' if lab=='Power Supply' else 'spec_power_supply_details'
                facts.append([n,val]); ev.append({'fact_name':n,'literal_quote':val,'line':i,'source_url':url,'capture_sha256':reqs[url]['sha256']}); last=facts[-1]
            else: last=None
        elif last is not None and s.strip() and len(s)-len(s.lstrip())>=22:
            last[1]=last[1]+' '+s.strip()
            for e in ev:
                if e['fact_name']==('spec_'+san(last[0]) if False else e['fact_name']): pass
        else: last=None if not s.strip() else last
    # sync evidence quotes with joined values
    vals={}
    for n,v in facts: vals[n]=v
    for e in ev: e['literal_quote']=vals[e['fact_name']] if e['fact_name'] in vals and e['fact_name'] not in ('spec_test_sponsor','spec_tested_by','spec_hardware_availability') else e['literal_quote']
    for i,l in enumerate(L,1):
        m=re.match(r'^\s*(SPEC(?:rate|speed)\(R\)2017_\w+)\s+(\S.*?)\s*$',l)
        if m and 'Not Run' not in m.group(2) and m.group(2)!='--':
            n='spec_result_'+san(m.group(1)); facts.append([n,m.group(2)]); ev.append({'fact_name':n,'literal_quote':l.strip(),'line':i,'source_url':url,'capture_sha256':reqs[url]['sha256']})
    facts.append(['spec_result_url',url]); ev.append({'fact_name':'spec_result_url','literal_quote':url,'line':0,'source_url':url,'capture_sha256':reqs[url]['sha256']})
    mk('spec-'+fn[:-4],url,rel,'CPU2017','Fields extracted: benchmark title, system line, test sponsor, tested by, hardware availability, CPU name/clock/cores/chips, memory, storage, power supply rows when the result has a power section, rate/speed result lines. The text result prints the hardware vendor and system name on one line; they are split using the SPEC all-results index hw_model field (the printed system line ends with that exact string).',brand,model,facts,ev,
       f"Index test_sponsor cell: '{row['test_sponsor']}'; index hw_model: '{hw_model}'.")
json.dump({'sources':sources,'devices':devices},open(R+'/spec-results-wave9-2026-10-11.json','w'),indent=1,ensure_ascii=False)
json.dump({'stripped':STRIPPED,'kept_paren':KEPT_PAREN},open(S+'/w9/strip.json','w'),indent=1)
print(len(sources),len(devices),sum(len(d['facts']) for d in devices),'skipped',skipped)

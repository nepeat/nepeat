import sys,json,re,glob,os,hashlib,collections
S='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/spec'
sys.path.insert(0,S); sys.path.insert(0,S+'/w10'); sys.path.insert(0,'/home/erin/nepeat/aislop/devicehunt')
from jbbparse import parse
from devicehunt.catalog import target_for
R='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-spec'
reqs={}
for l in open(R+'/wave10/requests-wave10.jsonl'):
    d=json.loads(l); reqs[d['url']]=d
idx={r['_links']['HTML'].split('/')[-1]:r for r in json.load(open(S+'/jbb_rows.json'))}
KW=re.compile(r'intel|amd|xeon|epyc|hygon|ampere|sparc|ghz|\bcores?\b|opteron|processor|\bcpu\b',re.I)
def san(l):
    l=l.replace('(s)','s').replace('#','number'); return re.sub(r'[^a-z0-9]+','_',l.lower()).strip('_')
LIC="SPEC published result (full disclosure HTML); original rights of SPEC and the submitting company retained; local factual extraction"
sources=[];devices=[];skipped=[];STRIPPED=[]
HW=['Name','Vendor','Model','Form Factor','CPU Name','CPU Characteristics','CPU Frequency (MHz)','Nodes Per System','Number of Systems','Chips Per System','Cores Per System','Cores Per Chip','Threads Per System','Memory Amount (GB)','# and size of DIMM(s)','Memory Details','Power Supply Quantity and Rating (W)','Cabinet/Housing/Enclosure','Shared Description','Shared Comment','Available']
for p in sorted(glob.glob(R+'/wave10/jbb2015/*.html')):
    fn=os.path.basename(p); rel='wave10/jbb2015/'+fn
    url=[u for u in reqs if u.endswith('/'+fn)][0]
    if reqs[url]['status']!=200: skipped.append((url,reqs[url]['status'])); continue
    sha=reqs[url]['sha256']; P=parse(p); rows=P.rows
    facts=[];ev=[]
    def add(name,val,loc,quote=None):
        facts.append([name,val]); ev.append({'fact_name':name,'literal_quote':quote or val,'locator':loc,'source_url':url,'capture_sha256':sha})
    r0=rows[0]; title_cell=r0[0]
    hwi=[i for i,r in enumerate(rows) if len(r)==1 and re.fullmatch(r'Hardware hw_\d+',r[0])]
    if not hwi: skipped.append((url,'no hw block')); continue
    nblocks=len(hwi); i0=hwi[0]; blk={}
    for k,r in enumerate(rows[i0+1:],i0+1):
        if len(r)==1: break
        if len(r)==2 and r[0] not in blk: blk[r[0]]=(r[1],k)
    hv=blk['Vendor'][0]; name=blk['Name'][0]
    tnote='' if title_cell==hv+' '+name else f" The page header title line prints '{title_cell}', which is not literally Vendor + ' ' + Name; identity uses the hw_1 Vendor and Name fields."
    hdr={}
    for r in rows[1:4]:
        for c in r:
            m=re.match(r'^(Tested by|Test Sponsor|Test location|Test date|SPEC license #|Hardware Availability|Software Availability|Publication):\s*(.*)$',c)
            if m: hdr[m.group(1)]=c,m.group(2)
    add('spec_benchmark',re.sub(r'\s+',' ',(P.title or '').strip()).split(' : ')[0].split(' Report')[0] or 'SPECjbb2015','page title','SPECjbb2015')
    add('spec_system_title_line',title_cell,'page header, row 0 cell 0')
    add('spec_hardware_vendor',hv,'SUT Description > Hardware hw_1 > Vendor'); add('spec_system_name',name,'SUT Description > Hardware hw_1 > Name')
    for k,n in [('Test Sponsor','spec_test_sponsor'),('Tested by','spec_tested_by'),('Hardware Availability','spec_hardware_availability')]:
        if k in hdr: add(n,hdr[k][1],'page header > '+k,hdr[k][0])
    for lab in HW[2:]:
        if lab in blk and blk[lab][0]!='None':
            add('spec_'+san(lab),blk[lab][0],'SUT Description > Hardware hw_1 > '+lab)
    m=re.search(r'(\d+) SPECjbb2015-(\S+) max-jOPS (\d+) SPECjbb2015-\S+ critical-jOPS',r0[1])
    mode=None
    if m:
        mode=m.group(2)
        add('spec_jbb_mode',mode,'page header, row 0 cell 1',f"SPECjbb2015-{mode}")
        a=f"{m.group(1)} SPECjbb2015-{mode} max-jOPS"; b=f"{m.group(3)} SPECjbb2015-{mode} critical-jOPS"
        add('spec_result_max_jops',a,'page header, row 0 cell 1',a)
        add('spec_result_critical_jops',b,'page header, row 0 cell 1',b)
    facts.append(['spec_result_url',url])
    sponsor=hdr.get('Test Sponsor',('',''))[1]
    role='brand' if re.sub(r'\s+',' ',sponsor)==re.sub(r'\s+',' ',hv) else 'reported_system_vendor'
    canon=target_for(hv) or hv; base=name
    m=re.search(r'\s*\(([^()]*)\)\s*$',name)
    if m and KW.search(m.group(1)) and m.start()>0: base=name[:m.start()]; STRIPPED.append((name,base))
    note=" The Hardware block hw_1 is used."
    if nblocks>1:
        h2=hwi[1]; b2={}
        for r in rows[h2+1:]:
            if len(r)==1: break
            if len(r)==2 and r[0] not in b2: b2[r[0]]=r[1]
        note=f" The Hardware block hw_1 is used. The page also lists Hardware hw_2 (Name '{b2.get('Name')}', Vendor '{b2.get('Vendor')}') and refers to it under a separate configuration ('Hardware Description: hw_2'); hw_2 is a different system, is NOT extracted, and none of its fields is attributed to this identity."
    rule=("Identity rules: brand = the catalog's reviewed TARGETS alias name when the Vendor field of Hardware hw_1 is a reviewed alias, otherwise that Vendor exactly as printed"+(f" (here '{hv}' -> '{canon}')" if canon!=hv else f" (here '{hv}' as printed)")+f"; model = the hw_1 Name with only a trailing parenthesised CPU/config group removed (here '{name}' -> '{base}'; the full printed name is kept as fact and identifier spec_system_name); affiliation = [hw_1 Vendor as printed, '{role}'] where role is brand when the page's Test Sponsor equals that Vendor, else reported_system_vendor (SPECjbb2015 pages have Tested by / Test Sponsor and a Hardware hw_1 Vendor; there is no separate Hardware Vendor line in the header). This is the submitter's printed system/vendor label for this result, not a manufacturer, OEM or operator claim and not proof of manufacture or deployment.")
    sources.append({'id':'spec-'+fn[:-5],'url':url,'tier':'vendor','license':LIC,'snapshot':rel,
      'coverage':f"SPEC SPECjbb2015 full-disclosure HTML result as published at {url} (HTTP 200, capture sha256 {sha}). Vendor-submitted system description published by SPEC; cell text parsed with the Python standard-library html.parser and quoted exactly (whitespace normalised); each fact's evidence gives the table/row-label locator instead of a line number; fields printed as None are omitted. Fields extracted: page header (vendor+system title, tested by, test sponsor, hardware availability, max-jOPS and critical-jOPS), and the Hardware hw_1 table (vendor, name, model, form factor, CPU, nodes/chips/cores/threads, memory, power supply row, enclosure/shared rows, available). Software, JVM, tuning, topology and RT-curve sections are not extracted.{note}{tnote} {rule} Test sponsor and Tested by are recorded as facts, not roles. spec_result_max_jops and spec_result_critical_jops are benchmark throughput results for the stated SPECjbb2015 mode (spec_jbb_mode: MultiJVM, Composite or Distributed; modes are not comparable with each other), printed with their mode label, not hardware specifications of the system. Index row: tested by '{idx[fn]['test.testedBy']}', system '{idx[fn]['product.SUT.hw.system.hw_1.name']}'."})
    devices.append({'source':'spec-'+fn[:-5],'key':name,'brand':canon,'model':base,'title':canon+' '+base,'kind':'server','affiliations':[[hv,role]],'identifiers':[['spec_system_name',name]],'facts':facts,'evidence':ev})
json.dump({'sources':sources,'devices':devices},open(R+'/spec-results-wave10-2026-10-11.json','w'),indent=1,ensure_ascii=False)
json.dump(STRIPPED,open(S+'/w10/strip.json','w'),indent=1)
print(len(sources),sum(len(d['facts']) for d in devices),'stripped',len(STRIPPED),'skipped',skipped)

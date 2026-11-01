import json,re,collections,os
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-entity-manager'
man=json.load(open(RAW+'/capture-manifest.json'));commit=man['commit']
objs=json.load(open('objects.json'))
sel=[r for r in objs if r['hits'] or r['vendor_dir']]
excl=[r for r in objs if not(r['hits'] or r['vendor_dir'])]
def stem(p): return os.path.basename(p)[:-5]
cnt=collections.Counter(r['name'] for r in sel)
used=collections.Counter()
FRU=re.compile(r"FruDevice\(\{(.*?)\}\)");PAIR=re.compile(r"'([A-Z_]+)'\s*:\s*'((?:[^'\\]|\\.)*)'")
FAN=re.compile(r'fan|tach|pwm',re.I)
PSU=re.compile(r'PowerSupplyUnit|PowerShelf|BatteryBackup|CapacitorBank|PowerMonitorModule|psu',re.I)
HSC=re.compile(r'^(ADM127[258]|ADM1281|LM5066I?|MP5990|MP5926|MP5998|MP5023|LTC428[27]|TPS25990|XDP7\d\d)$|hsc|efuse|hotswap|hot_swap',re.I)
def short(s): return s if len(s)<=200 else s[:200]+'...'
sources=[];devices=[];seen_src=set()
nf=0
for r in sel:
    p=r['path'];b=os.path.basename(p);loc=lambda l:f'({b}:{l})'
    name=r['name']
    lit=[a['f']['Model'][0] for a in r['assets'] if 'Model' in a['f'] and not a['f']['Model'][0].startswith('$') and a['f']['Model'][0]!='NOT_SUPPORTED']
    tag=stem(p)+(': '+lit[0] if name.startswith('$Name') and lit else '')
    model=name if cnt[name]==1 else f"{name} [{tag}]"
    used[model]+=1
    if used[model]>1: model=f"{name} [{tag}#{r['idx']+1}]"
    sid='entity-manager-'+p[len('configurations/'):-5].replace('/','--').lower()
    if sid not in seen_src:
        seen_src.add(sid)
        sources.append({'id':sid,'url':f'https://github.com/openbmc/entity-manager/blob/{commit}/{p}','tier':'project',
         'license':'Apache-2.0 (repo LICENCE) / upstream rights; factual extraction',
         'coverage':f'OpenBMC entity-manager configuration file at openbmc/entity-manager commit {commit}. Software detection configuration: Probe expressions are FRU-field match patterns (some are regular expressions) and Exposes lists what OpenBMC would instantiate once matched; neither is a datasheet, bill of materials, vendor support statement or proof that a physical unit with these FRU strings exists. Vendor subdirectory names and manufacturer strings in Probe are facts about the file, not company roles. Tokens starting with `$` (in Names, bus values and em_asset_* values) are template variables / FRU-field references printed as in the file, not literal values; em_asset_* facts are labelled reference, literal or placeholder. Facts omitted when the file lacks the key.',
         'snapshot':p})
    F=[]
    def add(k,v): 
        if [k,v] not in F: F.append([k,v])
    add('em_repo_commit',commit)
    add('em_config_file',f'{p}')
    rules=[]
    if r['vendor_dir']: rules.append("dir:ieisystem (selected by directory name on coordinator request; no vendor string match)" if r['vendor_dir'][0]=='ieisystem' else f"vendor subdirectory {r['vendor_dir'][0]} (repo path component)")
    if r['hits']:
        agg=collections.OrderedDict()
        for w,l in r['hits']: agg.setdefault(w.lower(),[]).append(l)
        rules.append('string hits in object: '+', '.join(f"{w} L{','.join(map(str,sorted(set(ls)))[:6] if False else map(str,sorted(set(ls))[:6]))}" for w,ls in agg.items()))
    add('em_selection_rule','; '.join(rules)+f' ({b})')
    if r['nobj']>1: add('em_object_position',f"object {r['idx']+1} of {r['nobj']} top-level objects, lines {r['line']}-{r['end']} ({b}:{r['line']})")
    add('em_config_name',f"{name} {loc(r['name_line'])}")
    if r['type'] is not None: add('em_config_type',f"{r['type']} {loc(r['type_line'])}")
    for l,pr in r['probes']:
        add('em_probe',f'{pr} {loc(l)}')
        for m in FRU.finditer(pr):
            for k,v in PAIR.findall(m.group(1)):
                add('em_probe_'+k.lower(),f'{v} {loc(l)}')
    ex=r['exposes']
    if ex:
        tc=collections.Counter(e['type'] for e in ex)
        add('em_exposes_count',f"{len(ex)} {'entry' if len(ex)==1 else 'entries'} ({b}:{ex[0]['line']})")
        add('em_exposes_types',json.dumps(dict(tc.most_common(40)))+f" (first entry {b}:{ex[0]['line']})")
        def fmt(e):
            s=f"{e['type']}: {e['name']}"
            parts=[]
            if e['bus'] is not None: parts.append(f"bus {e['bus']}")
            if e['addr'] is not None: parts.append(f"addr {e['addr']}")
            if e.get('serial') is not None: parts.append(f"SerialPort {e['serial']}")
            if parts: s+=" ["+" ".join(parts)+"]"
            return s+f" (L{e['line']})"
        for key,rx,fn in (('em_exposes_fan_entries',FAN,lambda e:FAN.search(str(e['type']))),
                          ('em_exposes_psu_entries',PSU,lambda e:PSU.search(str(e['type']))),
                          ('em_exposes_hsc_entries',HSC,lambda e:HSC.search(str(e['type']))or HSC.search(str(e['name'])))):
            hit=[e for e in ex if fn(e) and e['type'] not in('Pid','Pid.Zone','Stepwise')]
            if hit:
                v='; '.join(fmt(e) for e in hit[:25])+(f'; ... {len(hit)-25} more' if len(hit)>25 else '')
                add(key,v+f' ({b})')
        pm=[e for e in ex if e['type'] in('pmbus',)]
    for a in r['assets']:
        for k,fk in (('Manufacturer','em_asset_manufacturer'),('Model','em_asset_model'),('PartNumber','em_asset_part_number')):
            if k in a['f']:
                v,l=a['f'][k];lab=(f'FRU-field reference {v} (not a literal value)' if v.startswith('$') else f'placeholder {v} (the file prints this token)' if v=='NOT_SUPPORTED' else f'literal "{v}"')
                add(fk,f'{lab} {loc(l)}')
    for k,v in F:
        assert len(v)<4000,(k,len(v))
    nf+=len(F)
    devices.append({'source':sid,'key':f"{p}#{r['idx']}",'brand':'OpenBMC entity-manager config','model':model,
      'title':f'OpenBMC entity-manager config {model}','kind':'software configuration (BMC hardware detection)',
      'affiliations':[],'identifiers':[['entity_manager_config',model]],'facts':F})
assert len({d['model'] for d in devices})==len(devices)
cand={'sources':sources,'devices':devices}
json.dump(cand,open(RAW+'/entity-manager-config-facts-2026-10-11.json','w'),indent=1,ensure_ascii=False)
print(len(sources),len(devices),nf,len(excl),max(len(v) for d in devices for k,v in d['facts']))
json.dump([[r['path'],r['idx'],r['name'],r['probes'][0]] for r in excl],open('excluded_objects.json','w'),indent=1)

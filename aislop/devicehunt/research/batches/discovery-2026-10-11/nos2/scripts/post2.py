import json,re,glob,hashlib,os
R='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-nos2/'
c=json.load(open(R+'nos2-platform-facts-2026-10-11.json'))
DC='dentproject/dentOS/8a74afadfe7c3e0e22819dfe6ff982075efc50c6'
for s in c['sources']:
    if '/dentOS/tree/' in s['url']:
        s['url']=s['url'].replace('/tree/8a74afad','/tree/8a74afad',1)
        s['url']=re.sub(r'(/tree/[0-9a-f]{40}/)(?!packages/)',r'\1packages/platforms/',s['url'])
        s['license']='dentOS LICENSE (ONL-derived; opens with Debian-style third-party licence notes), not resolved to a single SPDX id; the captured files carry no license header; upstream rights; factual extraction'
        if 'arm-accton-as4610' in s['id']: s['coverage']+=' URL points to the armel directory; the same platform id also exists under armhf (see dent_platform_dir facts).'
    else:
        s['coverage']=s['coverage'].replace('example-config names/descriptions are as printed upstream.','example-config names/descriptions are as printed upstream (some are placeholder values, e.g. "asdf", "asdf leaf 2", "Stratum device device:s204" in x86-64-accton-as6712-32x-r0, flagged in the fact text); counts are counted repeated entries (line ranges cited), not printed numbers.')
def blocks(path):
    ls=open(path).read().split('\n'); out={}
    i=0
    while i<len(ls):
        m=re.match(r'^([a-z_]+) \{$',ls[i])
        if m:
            j=i
            while ls[j]!='}': j+=1
            out.setdefault(m.group(1),[]).append((i+1,j+1)); i=j
        i+=1
    return out
for x in c['devices']:
    if x['brand']!='Stratum platform': continue
    d=[f[1] for f in x['facts'] if f[0]=='stratum_dir_as_printed'][0]
    base=glob.glob(R+f'stratum/stratum/*/stratum/hal/config/{d}/')[0]
    cb=blocks(base+'chassis_config.pb.txt') ; pb=blocks(base+'phal_config.pb.txt') if os.path.exists(base+'phal_config.pb.txt') else {}
    new=[];seen_name=False
    for n,v in x['facts']:
        m=re.search(r'\(chassis_config\.pb\.txt:(\d+)\)$',v)
        if n=='stratum_chassis_name' and m and m.group(1)!='4':
            n='stratum_node_name'
        if n=='stratum_node_count':
            a,b=cb['nodes'][0][0],cb['nodes'][-1][1]; v=f"counted {len(cb['nodes'])} repeated nodes entries (chassis_config.pb.txt:{a}-{b})"
        elif n=='stratum_singleton_port_count':
            a,b=cb['singleton_ports'][0][0],cb['singleton_ports'][-1][1]; v=f"counted {len(cb['singleton_ports'])} repeated singleton_ports entries (chassis_config.pb.txt:{a}-{b})"
        elif n=='stratum_phal_card_port_count':
            a,b=pb['cards'][0]; k=int(v.split()[0]); v=f"counted {k} repeated ports entries (phal_config.pb.txt:{a}-{b})"
        elif n in('stratum_phal_fan_count','stratum_phal_psu_count','stratum_phal_led_count','stratum_phal_thermal_count'):
            sec={'fan':'fan_trays','psu':'psu_trays','led':'led_groups','thermal':'thermal_groups'}[n.split('_')[2]]
            a,b=pb[sec][0]; k=int(v.split()[0]); v=f"counted {k} repeated entries (phal_config.pb.txt:{a}-{b})"
        if d=='x86-64-accton-as6712-32x-r0':
            if n=='stratum_chassis_description' or (n=='stratum_chassis_name') or (n=='stratum_node_name'): v+=' [printed placeholder value]'
        new.append([n,v])
    x['facts']=new
json.dump(c,open(R+'nos2-platform-facts-2026-10-11.json','w'),indent=1)
print(len(c['devices']),sum(len(x['facts']) for x in c['devices']))

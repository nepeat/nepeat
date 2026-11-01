import json
R='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-nos2/'
c=json.load(open(R+'nos2-platform-facts-2026-10-11.json'))
def st(x): return dict((f[0],f[1]) for f in x['facts']).get('dent_vs_onl_nos_capture')
keep=lambda x: x['brand']!='DENT platform' or st(x)!='overlap_onl_nos'
dropped=[x['model'] for x in c['devices'] if not keep(x)]
ids={x['source'] for x in c['devices'] if keep(x)}
c['devices']=[x for x in c['devices'] if keep(x)]; c['sources']=[s for s in c['sources'] if s['id'] in ids]
json.dump(c,open(R+'nos2-platform-facts-2026-10-11.json','w'),indent=1)
json.dump(dict(note='same id as ONL; not imported',ids=sorted(dropped)),open(R+'dent-overlap-not-imported.json','w'),indent=1)
snaps={s['snapshot'] for s in c['sources']}
refs=set(snaps)
for x in c['devices']:
    for f in x['facts']:
        if f[0]=='dent_files_captured':
            for p in json.loads(f[1]): refs.add('dentproject/dentOS/8a74afadfe7c3e0e22819dfe6ff982075efc50c6/'+p)
        if f[0]=='stratum_files_captured':
            d=[f2[1] for f2 in x['facts'] if f2[0]=='stratum_dir_as_printed'][0]
            for p in json.loads(f[1]): refs.add(f'stratum/stratum/f8f71301cb602ac663818f2b185d57771b27d8e2/stratum/hal/config/{d}/{p}')
m=json.load(open(R+'capture-manifest.json'))
for f in m['files']: f['referenced_by_candidate']=f['path'] in refs
m['referenced_count']=sum(f['referenced_by_candidate'] for f in m['files'])
json.dump(m,open(R+'capture-manifest.json','w'),indent=1)
print(len(c['devices']),len(dropped),m['referenced_count'],len(m['files']),sum(len(x['facts']) for x in c['devices']))

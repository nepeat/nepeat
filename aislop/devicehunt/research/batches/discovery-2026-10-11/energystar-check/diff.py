import csv,sys,re,json
R='/home/erin/nepeat/aislop/devicehunt/data/raw/'
N=R+'discovery-2026-10-11-energystar-check/'
pairs=[('servers',R+'energystar-servers/servers.csv',N+'servers.csv'),
('storage-block',R+'energystar-storage-block/equipment.csv',N+'storage-block.csv'),
('storage-file',R+'energystar-storage-file/equipment.csv',N+'storage-file.csv'),
('network',R+'energystar-network/equipment.csv',N+'network.csv')]
T=re.compile(r'quanta|\bqct\b|inspur|ieit|inventec|wiwynn|foxconn|hon hai|\bfii\b|fulian|hyve|celestica|oracle|amazon|accton|edgecore|synnex',re.I)
def load(p):
    with open(p,newline='',encoding='utf-8') as f:
        r=csv.reader(f); h=next(r); rows=[x for x in r]
    return h,rows
for name,o,n in pairs:
    ho,ro=load(o);hn,rn=load(n)
    print('==',name,'old',len(ro),'new',len(rn),'headers same',ho==hn, 'hdr only old',set(ho)-set(hn),'only new',set(hn)-set(ho))
    d_o={x[0]:dict(zip(ho,x)) for x in ro}; d_n={x[0]:dict(zip(hn,x)) for x in rn}
    print(' dup ids old/new',len(ro)-len(d_o),len(rn)-len(d_n))
    add=[k for k in d_n if k not in d_o]; rem=[k for k in d_o if k not in d_n]
    ch=[k for k in d_n if k in d_o and d_n[k]!=d_o[k]]
    print(' added',len(add),'removed',len(rem),'changed',len(ch))
    for k in ch[:10]:
        print('  chg',k,{c:(d_o[k].get(c),d_n[k].get(c)) for c in d_n[k] if d_o[k].get(c)!=d_n[k].get(c)})
    def tgt(d): return T.search(' '.join([d.get('ENERGY STAR Partner',''),d.get('Brand Name','')]))
    print(' target rows old/new', sum(1 for d in d_o.values() if tgt(d)), sum(1 for d in d_n.values() if tgt(d)))
    print(' ADDED:')
    for k in add:
        d=d_n[k]; print('  ',k,'|',d.get('ENERGY STAR Partner'),'|',d.get('Brand Name'),'|',d.get('Model Name'),'|',d.get('Model Number'),'|T' if tgt(d) else '')
    print(' REMOVED:')
    for k in rem:
        d=d_o[k]; print('  ',k,'|',d.get('ENERGY STAR Partner'),'|',d.get('Brand Name'),'|',d.get('Model Number'))
    if name=='servers':
        json.dump({'added':[d_n[k] for k in add],'removed':[d_o[k] for k in rem]},open(sys.argv[1],'w'),indent=1)
        # brand distribution among additions
        from collections import Counter
        print(' add brands',Counter(d_n[k]['Brand Name'] for k in add).most_common(20))

import json,re,collections,sys
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-entity-manager'
sel=json.load(open('selection.json'))
man=json.load(open(RAW+'/capture-manifest.json'))
class D:  # dict with key lines
    def __init__(s): s.items=[];s.line=0;s.end=0
class L:
    def __init__(s): s.items=[];s.line=0;s.end=0
class P:
    def __init__(s,t): s.t=t;s.i=0;s.ln=1
    def ws(s):
        while s.i<len(s.t):
            c=s.t[s.i]
            if c=='\n': s.ln+=1;s.i+=1
            elif c in ' \t\r': s.i+=1
            elif s.t.startswith('//',s.i):
                while s.i<len(s.t) and s.t[s.i]!='\n': s.i+=1
            elif s.t.startswith('/*',s.i):
                j=s.t.index('*/',s.i)+2;s.ln+=s.t.count('\n',s.i,j);s.i=j
            else: break
    def val(s):
        s.ws();c=s.t[s.i]
        if c=='{':
            d=D();d.line=s.ln;s.i+=1
            while True:
                s.ws()
                if s.t[s.i]=='}': s.i+=1;break
                kl=s.ln;k=s.val();s.ws();assert s.t[s.i]==':',(s.ln);s.i+=1
                v=s.val();d.items.append((k,kl,v));s.ws()
                if s.t[s.i]==',': s.i+=1
            d.end=s.ln;return d
        if c=='[':
            d=L();d.line=s.ln;s.i+=1
            while True:
                s.ws()
                if s.t[s.i]==']': s.i+=1;break
                ln=s.ln;v=s.val();d.items.append((ln,v));s.ws()
                if s.t[s.i]==',': s.i+=1
            d.end=s.ln;return d
        if c=='"':
            j=s.i+1
            while s.t[j]!='"':
                j+=2 if s.t[j]=='\\' else 1
            r=json.loads(s.t[s.i:j+1]);s.i=j+1;return r
        m=re.compile(r'-?\d+(\.\d+)?([eE][-+]?\d+)?|true|false|null').match(s.t,s.i)
        assert m,(s.ln,s.t[s.i:s.i+20]);s.i=m.end();return json.loads(m.group(0))
def get(d,k):
    for kk,ln,v in d.items:
        if kk==k: return ln,v
    return None,None
def walk(node):
    """yield (key,keyline,value,parent)"""
    if isinstance(node,D):
        for k,ln,v in node.items:
            yield k,ln,v,node
            yield from walk(v)
    elif isinstance(node,L):
        for ln,v in node.items: yield from walk(v)
def s(x): return x if isinstance(x,str) else None
FRU=re.compile(r"FruDevice\(\{(.*?)\}\)")
PAIR=re.compile(r"'([A-Z_]+)'\s*:\s*'((?:[^'\\]|\\.)*)'")
out=[];seen=collections.Counter()
for path,rules in sel['selected'].items():
    t=open(RAW+'/'+path,encoding='utf-8').read()
    root=P(t).val()
    objs=[(1,root)] if isinstance(root,D) else root.items
    for idx,(ln,o) in enumerate(objs):
        rec={'path':path,'idx':idx,'nobj':len(objs),'line':o.line,'end':o.end}
        nl,name=get(o,'Name');rec['name']=name;rec['name_line']=nl
        tl,ty=get(o,'Type');rec['type']=ty;rec['type_line']=tl
        pl,pr=get(o,'Probe')
        probes=[]
        if isinstance(pr,str): probes=[(pl,pr)]
        elif isinstance(pr,L): probes=[(l,v) for l,v in pr.items if isinstance(v,str)]
        rec['probes']=probes
        ex=get(o,'Exposes')[1]
        exl=[]
        if isinstance(ex,L):
            for l,e in ex.items:
                if isinstance(e,D):
                    _,et=get(e,'Type');_,en=get(e,'Name');_,bus=get(e,'Bus');_,ad=get(e,'Address');_,sp=get(e,'SerialPort')
                    exl.append({'line':l,'type':et,'name':en,'bus':bus,'addr':ad,'serial':sp if isinstance(sp,str) else None})
        rec['exposes']=exl
        assets=[]
        for k,kl,v,par in walk(o):
            if k=='xyz.openbmc_project.Inventory.Decorator.Asset' and isinstance(v,D):
                assets.append({'line':kl,'f':{kk:(vv,ll) for kk,ll,vv in v.items if kk in('Manufacturer','Model','PartNumber') and isinstance(vv,str)}})
        rec['assets']=assets
        rec['hits']=[(w,l) for r in rules if r[0]=='string_hits' for w,l in r[1] if o.line<=l<=o.end]
        rec['vendor_dir']=[r[1] for r in rules if r[0]=='vendor_dir']
        out.append(rec)
json.dump(out,open('objects.json','w'),indent=1)
print(len(out),'objects',sum(1 for r in out if not r['hits'] and not r['vendor_dir']),'without own hits',sum(1 for r in out if r['name'] is None),'no name',sum(1 for r in out if not r['probes']),'no probe')
c=collections.Counter(r['name'] for r in out);print([k for k,v in c.items() if v>1][:20],sum(1 for v in c.values() if v>1))

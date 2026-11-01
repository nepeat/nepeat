import re
def parse(path):
    """returns dict: header(lines), top fields, sets: list of (setid, fields[(label,value,lineno)])"""
    L=open(path,encoding='utf-8',errors='replace').read().split('\n')
    out={'title':None,'metric':None,'top':[],'sets':[]}
    cur=None; last=None; sec=None; stop=False
    # header: first lines
    for i,l in enumerate(L[:12]):
        pass
    for i,l in enumerate(L,1):
        s=l.rstrip()
        if not s.strip(): last=None if False else last; continue
        head=s[:35]; val=s[35:].strip() if len(s)>35 else ''
        hs=head.strip()
        if stop and not re.match(r"^\s*Set: '",s): continue
        if re.match(r"^\s*Set: '",s):
            stop=False
            cur={'id':re.search(r"Set: '([^']*)'",s).group(1),'fields':[]}; out['sets'].append(cur); last=None; continue
        if re.fullmatch(r'\s*(HARDWARE|SOFTWARE|Benchmark Results Summary|System Under Test|Boot Firmware Settings|Management Firmware Settings|System Under Test Notes)\s*',s):
            if s.strip() in ('HARDWARE','SOFTWARE'): sec=s.strip()
            else: cur=None; stop=True
            last=None; continue
        if hs and val and (hs.endswith(':') or re.match(r'.*\S {2,}$',head+' ') or True) and len(s)>35 and s[33:35]!='  ' or False:
            pass
        if hs and val and s[34:35]==' ' and (len(s)<=35 or s[35]!=' '):
            last=[hs,val,i]
            tgt=cur['fields'] if cur is not None else out['top']
            tgt.append(last); continue
        if hs and not val and last is not None and not hs.startswith('*'):
            last[0]=(last[0]+' '+hs).strip(); continue
        if not hs and val and last is not None:
            last[1]=last[1]+' '+val; continue
    for t in [out['top']]+[s['fields'] for s in out['sets']]:
        for f in t:
            f[0]=f[0].rstrip(':').strip()
    return out
if __name__=='__main__':
    import sys,json
    o=parse(sys.argv[1]); print(json.dumps(o['top'],indent=0)[:1500])
    for s in o['sets']: print(s['id']); [print(' ',f) for f in s['fields'][:45]]

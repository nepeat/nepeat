"""minimal JSON parser tracking line numbers. objects -> list of (key, keyline, node); node = (value_or_children, line, kind)"""
import re
WS=re.compile(r'[ \t\r\n]*')
NUM=re.compile(r'-?\d+(\.\d+)?([eE][+-]?\d+)?')
class P:
    def __init__(s,t): s.t=t; s.i=0
    def line(s,i=None): return s.t.count('\n',0,s.i if i is None else i)+1
    def ws(s): s.i=WS.match(s.t,s.i).end()
    def val(s):
        s.ws(); c=s.t[s.i]; ln=s.line()
        if c=='{':
            s.i+=1; items=[]; s.ws()
            if s.t[s.i]=='}': s.i+=1; return ('obj',ln,items)
            while True:
                s.ws(); kl=s.line(); k=s.string(); s.ws(); assert s.t[s.i]==':'; s.i+=1
                v=s.val(); items.append((k,kl,v)); s.ws()
                if s.t[s.i]==',': s.i+=1; continue
                assert s.t[s.i]=='}'; s.i+=1; return ('obj',ln,items)
        if c=='[':
            s.i+=1; items=[]; s.ws()
            if s.t[s.i]==']': s.i+=1; return ('arr',ln,items)
            while True:
                items.append(s.val()); s.ws()
                if s.t[s.i]==',': s.i+=1; continue
                assert s.t[s.i]==']'; s.i+=1; return ('arr',ln,items)
        if c=='"': return ('str',ln,s.string())
        for lit,v in (('true',True),('false',False),('null',None)):
            if s.t.startswith(lit,s.i): s.i+=len(lit); return ('lit',ln,v)
        m=NUM.match(s.t,s.i); s.i=m.end(); return ('num',ln,float(m.group()) if '.' in m.group() else int(m.group()))
    def string(s):
        assert s.t[s.i]=='"'; j=s.i+1
        while s.t[j]!='"': j+= 2 if s.t[j]=='\\' else 1
        import json; r=json.loads(s.t[s.i:j+1]); s.i=j+1; return r
def parse(path):
    t=open(path,encoding='utf-8').read(); p=P(t); v=p.val(); p.ws(); assert p.i==len(t); return v
def get(node,key):
    for k,kl,v in node[2]:
        if k==key: return v
def plain(node):
    k=node[0]
    if k=='obj': return {a:plain(c) for a,_,c in node[2]}
    if k=='arr': return [plain(c) for c in node[2]]
    return node[2]

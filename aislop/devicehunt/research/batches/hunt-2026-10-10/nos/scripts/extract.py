import warnings;warnings.simplefilter('ignore')
import ast,io,json,re,sys,tokenize,os
RAW='/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-nos'
man=json.load(open(RAW+'/capture-manifest.json'))
AIR_TOK=re.compile(r'(?<![A-Za-z0-9])(F2B|B2F|AFO|AFI)(?![a-z0-9])|_(F2B|B2F|AFO|AFI)\b|\b(F2B|B2F|AFO|AFI)_')
AIR_WORD=re.compile(r'^(f2b|b2f|afo|afi|intake|exhaust|front[ _-]?to[ _-]?back|back[ _-]?to[ _-]?front)$',re.I)
CTX=re.compile(r'model|part|pn\b|_pn|vendor|manufact|brand|prod|airflow|direction|fan_dir|serial|type|name',re.I)
COUNT_NAME=re.compile(r'(NUM(?!BER)|COUNT|TOTAL|MAX_(PSU|FAN|THERMAL)|(PSU|FAN|THERMAL)S?_MAX)',re.I)
DECL=re.compile(r'(?=.*(PSU|FAN|THERMAL))(?=.*(NUM(?!BER)|COUNT|TOTAL|MAX))',re.I)
COUNT_EXCL=re.compile(r'POWER|WATT|LEN|PATH|SIZE|BUF|STR|SPEED|RPM|PWM|DUTY|THRESH|DELAY|TIMEOUT|INTERVAL|RETRY|OFFSET|ADDR|BUS',re.I)
FIELD=re.compile(r'part|model|serial|product|vendor|airflow|manufact',re.I)
out=[]
def add(f,line,kind,**kw):
    out.append(dict(file=f['local'],path=f['path'],line=line,kind=kind,**kw))
def platdir(f):
    p=f['path'];q=p.split('/')
    if f['repo'].endswith('sonic-buildimage'):
        return ('sonic', q[2].replace('sonic-platform-modules-',''), q[3])
    return ('onl', q[2], q[4])
def py_file(f,src):
    lines=src.split('\n')
    try: tree=ast.parse(src)
    except Exception as e:
        add(f,0,'parse_failed',error=repr(e)[:120])
        # regex fallback
        for i,l in enumerate(lines,1):
            s=l.strip()
            if s.startswith('#'):
                if AIR_TOK.search(s): add(f,i,'comment_airflow',text=s)
                continue
            m=re.match(r'\s*(?:self\.)?([A-Za-z_]\w*)\s*=\s*(\d+)\s*(#.*)?$',l)
            if m and DECL.search(m[1]) and not COUNT_EXCL.search(m[1]) and (not l[:1].isspace() or 'self.' in l): add(f,i,'count_const',name=m[1],value=int(m[2]),via='regex_fallback')
            m=re.match(r'\s*(?:self\.)?([A-Za-z_]\w*)\s*=\s*([\'"])(.*?)\2',l)
            if m and (CTX.search(m[1]) or m[1] in('PLATFORM','MODEL','SYS_OBJECT_ID') or AIR_WORD.match(m[3])): add(f,i,'assign_literal',target=m[1],literal=m[3],via='regex_fallback')
        return
    # docstrings
    doc=set()
    for n in ast.walk(tree):
        if isinstance(n,(ast.Module,ast.ClassDef,ast.FunctionDef,ast.AsyncFunctionDef)) and n.body and isinstance(n.body[0],ast.Expr) and isinstance(getattr(n.body[0],'value',None),ast.Constant) and isinstance(n.body[0].value.value,str):
            doc.add(id(n.body[0].value))
    countok=set()
    def mark(body,inclass):
        for st in body:
            if isinstance(st,(ast.Assign,ast.AnnAssign)): countok.add(id(st))
            elif isinstance(st,ast.ClassDef): mark(st.body,True)
            if inclass and isinstance(st,ast.FunctionDef) and st.name=='__init__':
                for x in ast.walk(st):
                    if isinstance(x,(ast.Assign,ast.AnnAssign)):
                        tg=x.targets if isinstance(x,ast.Assign) else [x.target]
                        if any(isinstance(t,ast.Attribute) and isinstance(t.value,ast.Name) and t.value.id=='self' for t in tg): countok.add(id(x))
    mark(tree.body,False)
    seg=lambda n:(ast.get_source_segment(src,n) or '').replace('\n',' ')[:160]
    isstr=lambda n:isinstance(n,ast.Constant) and isinstance(n.value,str)
    seen=set()
    def emit(line,kind,**kw):
        k=(line,kind,json.dumps(kw,sort_keys=True))
        if k in seen: return
        seen.add(k); add(f,line,kind,**kw)
    for n in ast.walk(tree):
        if isinstance(n,(ast.Assign,ast.AnnAssign)):
            tg=n.targets if isinstance(n,ast.Assign) else [n.target]
            v=n.value
            for t in tg:
                name=t.id if isinstance(t,ast.Name) else (t.attr if isinstance(t,ast.Attribute) else None)
                if not name or v is None: continue
                if isinstance(v,ast.Constant) and type(v.value) is int and DECL.search(name) and not COUNT_EXCL.search(name) and id(n) in countok:
                    emit(n.lineno,'count_const',name=name,value=v.value)
                if isinstance(v,(ast.List,ast.Tuple,ast.Set)) and CTX.search(name):
                    for e in v.elts:
                        if isstr(e): emit(e.lineno,'list_literal',target=name,literal=e.value)
                if isstr(v) and (CTX.search(name) or re.fullmatch(r'PLATFORM|MODEL|SYS_OBJECT_ID',name) or AIR_WORD.match(v.value)):
                    emit(n.lineno,'assign_literal',target=name,literal=v.value)
        elif isinstance(n,ast.Dict):
            for k,v in zip(n.keys,n.values):
                if k is None: continue
                ks=k.value if isinstance(k,ast.Constant) else None
                vs=v.value if isinstance(v,ast.Constant) else None
                if not (isinstance(ks,(str,int)) and (isinstance(vs,(str,int,float)) or vs is None)): continue
                if not (isinstance(ks,str) or isinstance(vs,str)): continue
                txt=' '.join(str(x) for x in (ks,vs) if isinstance(x,str))
                kind=None
                if AIR_WORD.match(str(ks)) or AIR_WORD.match(str(vs)): kind='dict_airflow'
                elif FIELD.search(txt): kind='dict_field'
                if kind: emit(k.lineno,kind,key=ks,value=vs)
        elif isinstance(n,ast.Compare):
            parts=[n.left]+n.comparators
            for p in parts:
                if isstr(p) and id(p) not in doc:
                    others=[q for q in parts if q is not p]
                    ctx=' | '.join(seg(q) for q in others)
                    if CTX.search(ctx) or AIR_WORD.match(p.value): emit(p.lineno,'compare_literal',literal=p.value,against=ctx)
                elif isinstance(p,(ast.Tuple,ast.List,ast.Set)):
                    for e in p.elts:
                        if isstr(e):
                            others=[q for q in parts if q is not p]
                            ctx=' | '.join(seg(q) for q in others)
                            if CTX.search(ctx) or AIR_WORD.match(e.value): emit(e.lineno,'compare_literal',literal=e.value,against=ctx)
        elif isinstance(n,ast.Call) and isinstance(n.func,ast.Attribute) and n.func.attr in('startswith','endswith','find','index','count') :
            ctx=seg(n.func.value)
            for a in n.args:
                if isstr(a) and (CTX.search(ctx) or AIR_WORD.match(a.value)): emit(a.lineno,'compare_literal',literal=a.value,against=f'{ctx}.{n.func.attr}()')
        if isstr(n) and id(n) not in doc and AIR_WORD.match(n.value):
            emit(n.lineno,'airflow_literal',literal=n.value,line_text=lines[n.lineno-1].strip()[:200])
    # comments
    try:
        for t in tokenize.generate_tokens(io.StringIO(src).readline):
            if t.type==tokenize.COMMENT and (AIR_TOK.search(t.string) or re.search(r'airflow|AFO|AFI',t.string)): emit(t.start[0],'comment_airflow',text=t.string.strip())
    except Exception: pass
def strip_c(l): return l
STR=re.compile(r'"((?:[^"\\]|\\.)*)"')
def c_file(f,src):
    lines=src.split('\n')
    incom=False
    for i,l in enumerate(lines,1):
        code=l;com=''
        # crude comment split honoring block comments
        buf='';j=0;instr=False
        while j<len(l):
            if incom:
                k=l.find('*/',j)
                if k<0: com+=l[j:];j=len(l)
                else: com+=l[j:k];incom=False;j=k+2
                continue
            c=l[j]
            if instr:
                buf+=c
                if c=='\\' and j+1<len(l): buf+=l[j+1];j+=1
                elif c=='"': instr=False
                j+=1;continue
            if c=='"': instr=True;buf+=c;j+=1;continue
            if l.startswith('/*',j): incom=True;j+=2;continue
            if l.startswith('//',j): com+=l[j+2:];j=len(l);continue
            buf+=c;j+=1
        code=buf.strip();com=com.strip()
        if com and re.search(r'F2B|B2F|AFO|AFI|airflow|front.to.back|back.to.front',com,re.I): add(f,i,'comment_airflow',text=com[:200])
        if not code: continue
        if AIR_TOK.search(code): add(f,i,'airflow_token_line',text=code[:200])
        m=re.match(r'#\s*define\s+([A-Za-z_]\w*)\s+(.*)$',code)
        if m:
            nm,val=m[1],m[2].strip()
            if re.fullmatch(r'-?\d+|0x[0-9a-fA-F]+',val) and DECL.search(nm) and not COUNT_EXCL.search(nm): add(f,i,'count_define',name=nm,value=val)
            sm=re.fullmatch(r'"((?:[^"\\]|\\.)*)"',val)
            if sm and (CTX.search(nm) or AIR_WORD.match(sm[1]) or re.match(r'x86-64-',sm[1])): add(f,i,'define_literal',name=nm,literal=sm[1])
            continue
        lits=STR.findall(code)
        if not lits: continue
        am=re.search(r'(\w+)\s*\[\s*\w*\s*\]\s*=\s*\{(.*)$',code)
        if am and (CTX.search(am[1]) or re.match(r'(ac|dc)_',am[1],re.I)) and STR.search(am[2]):
            for s_ in STR.findall(am[2]): add(f,i,'c_string_array',array=am[1],literal=s_,statement=code[:220])
            continue
        if re.search(r'\b(strcmp|strncmp|strcasecmp|strncasecmp|strstr|memcmp)\s*\(',code):
            for s in dict.fromkeys(lits): add(f,i,'c_compare_literal',literal=s,statement=code[:220])
        elif re.search(r'model|part_?n|part_?number|\bpn\b|manufact|vendor|prod(uct)?[_-]?name|airflow|direction',code,re.I) and not re.search(r'AIM_LOG|printf\(".*(Unable|Debug)',code):
            for s in lits:
                if s.strip(): add(f,i,'c_model_literal_line',literal=s,statement=code[:220])
        elif os.path.basename(f['path'])=='sysi.c' and any(re.match(r'x86-64-',s) for s in lits):
            for s in lits:
                if re.match(r'x86-64-',s): add(f,i,'c_platform_literal',literal=s,statement=code[:220])
for f in man['files']:
    raw=open(f"{RAW}/{f['local']}",'rb').read()
    try: src=raw.decode('utf-8')
    except UnicodeDecodeError:
        src=''.join(c if ord(c)<128 else '\\x%02x'%ord(c) for c in raw.decode('latin-1'))
        add(f,0,'non_utf8_file',note='bytes >0x7f rendered as literal \\xNN escapes (byte-faithful, latin-1 mapping)')
    if f['path'].endswith('.c'): c_file(f,src)
    else: py_file(f,src)
json.dump({'manifest_files':len(man['files']),'items':out},open(sys.argv[1],'w'),indent=0)
import collections
print(len(out),collections.Counter(o['kind'] for o in out))

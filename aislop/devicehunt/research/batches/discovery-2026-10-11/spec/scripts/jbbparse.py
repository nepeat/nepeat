import re
from html.parser import HTMLParser
class P(HTMLParser):
    def __init__(s):
        super().__init__(convert_charrefs=True); s.rows=[]; s.stack=[]; s.title=None; s.intitle=False; s.divs=[]
        s.skip=0; s.texts=[]
    def handle_starttag(s,tag,a):
        if tag in('script','style'): s.skip+=1
        if tag=='tr': s.stack.append([]) ; s.cell=None
        if tag in('td','th') and s.stack: s.stack[-1].append(''); s.cell=True
        if tag=='br' and s.stack and s.stack[-1]: s.stack[-1][-1]+=' '
        if tag=='title': s.intitle=True
    def handle_endtag(s,tag):
        if tag in('script','style'): s.skip-=1
        if tag=='tr' and s.stack:
            r=s.stack.pop(); s.rows.append([re.sub(r'\s+',' ',c).strip() for c in r])
        if tag=='title': s.intitle=False
    def handle_data(s,d):
        if s.skip: return
        if s.intitle: s.title=(s.title or '')+d
        if s.stack and s.stack[-1]: s.stack[-1][-1]+=d
        s.texts.append(d)
def parse(path):
    p=P(); p.feed(open(path,encoding='utf-8',errors='replace').read()); return p
if __name__=='__main__':
    import sys
    p=parse(sys.argv[1]); print(p.title)
    for i,r in enumerate(p.rows):
        if r and any(r): print(i,r[:6])

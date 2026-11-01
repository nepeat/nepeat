import re,sys,json,html
from html.parser import HTMLParser
def rows(path):
    t=open(path,encoding='utf-8').read()
    out=[]
    for m in re.finditer(r'<tr[^>]*>(.*?)</tr>',t,re.S):
        r=m.group(1)
        if '<td' not in r: continue
        cells=re.findall(r'<td class="([^"]*)"[^>]*>(.*?)</td>',r,re.S)
        d={}
        for c,v in cells:
            links=re.findall(r'href="([^"]+)">([^<]+)<',v)
            txt=re.sub(r'<span class="disclosures">.*?</span>','',v,flags=re.S)
            txt=html.unescape(re.sub(r'<[^>]+>','|',txt)); txt=re.sub(r'\s+',' ',txt); txt=re.sub(r'(\| ?)+','|',txt).strip(' |')
            d[c]=txt
            if links: d['_links']={l[1]:l[0] for l in links}; d['_date']=re.sub(r'\s+',' ',re.sub(r'<[^>]+>.*','',re.sub(r'.*?<span class="disclosures">','',v,flags=re.S),flags=re.S)).strip(' |')
        out.append(d)
    return out
if __name__=='__main__':
    R=rows(sys.argv[1]); print(len(R)); print(R[0]); json.dump(R,open(sys.argv[2],'w'))

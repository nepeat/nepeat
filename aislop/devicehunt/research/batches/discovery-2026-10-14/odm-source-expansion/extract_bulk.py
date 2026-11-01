#!/usr/bin/env python3
"""Offline, stdlib bulk extraction. No network, database writes, or imports."""
from __future__ import annotations
import argparse
from collections import Counter
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re

VOID = set('area base br col embed hr img input link meta param source track wbr'.split())
TARGET_PARTNERS = {'Inventec', 'QCT', 'Quanta', 'Wiwynn', 'Inspur', 'MiTAC', 'MITAC', 'Wistron', 'PEGATRON', 'Foxconn', 'Ingrasys', 'Hyve', 'Supermicro'}

def sha(data): return hashlib.sha256(data).hexdigest()
def dump(path, data): path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def walk(node, tag=None):
    for child in node.children:
        if isinstance(child, Node):
            if tag is None or child.tag == tag: yield child
            yield from walk(child, tag)

class Node:
    def __init__(self, tag, attrs=(), parent=None, start=0, line=1, column=0):
        self.tag, self.attrs, self.parent = tag, dict(attrs), parent
        self.children, self.start, self.end = [], start, start
        self.line, self.column = line, column
    def text(self):
        return ''.join(c.text() if isinstance(c, Node) else c for c in self.children)
    def display(self):
        # Block boundaries retain distinct values and prevent </p><p> concatenation.
        def parts(n):
            for c in n.children:
                if isinstance(c, Node):
                    if c.tag in {'br','p','li','div','dt','dd','h2','h3'}: yield ' '
                    yield from parts(c)
                    if c.tag in {'p','li','div','dt','dd','h2','h3'}: yield ' '
                else: yield c
        return ' '.join(''.join(parts(self)).split())
    def ancestor(self, predicate):
        n=self.parent
        while n is not None:
            if predicate(n): return n
            n=n.parent
        return None

class Document(HTMLParser):
    def __init__(self, raw):
        super().__init__(convert_charrefs=True)
        self.raw=raw; self.text_source=raw.decode('utf-8')
        self.lines=[0]+[m.end() for m in re.finditer('\n', self.text_source)]
        self.root=Node('document'); self.stack=[self.root]
        self.feed(self.text_source); self.close()
        if len(self.stack)!=1: raise ValueError('unclosed HTML elements')
    def source_offset(self):
        line,col=self.getpos(); return self.lines[line-1]+col
    def handle_starttag(self, tag, attrs):
        line,col=self.getpos(); n=Node(tag,attrs,self.stack[-1],self.source_offset(),line,col)
        n.end=n.start+len(self.get_starttag_text());self.stack[-1].children.append(n)
        if tag not in VOID:self.stack.append(n)
    def handle_startendtag(self,tag,attrs):
        self.handle_starttag(tag,attrs)
        if tag not in VOID:self.stack.pop()
    def handle_endtag(self, tag):
        for i in range(len(self.stack)-1,0,-1):
            if self.stack[i].tag==tag:
                end=self.text_source.find('>',self.source_offset())+1
                for n in self.stack[i:]: n.end=end
                del self.stack[i:];return
    def handle_data(self,data): self.stack[-1].children.append(data)
    def proof(self,n):
        fragment=self.text_source[n.start:n.end].encode('utf-8')
        return {'line':n.line,'column_zero_based':n.column,
                'char_start':n.start,'char_end':n.end,
                'byte_start':len(self.text_source[:n.start].encode('utf-8')),
                'byte_end':len(self.text_source[:n.end].encode('utf-8')),
                'fragment_sha256':sha(fragment)}

def cell(doc,n):
    return {'tag':n.tag,'attributes':n.attrs,'text_raw':n.text(),'text':n.display(),
            'links':[{'text':a.display(),'attributes':a.attrs} for a in walk(n,'a')],
            'superscripts':[a.text() for a in walk(n,'sup')], 'provenance':doc.proof(n)}

def extract_tables(doc):
    out=[]; headings=list(walk(doc.root))
    headings=[n for n in headings if n.tag in {'h1','h2','h3','h4'}]
    for ti,t in enumerate(walk(doc.root,'table'),1):
        table_id=t.attrs.get('id',f'ordinal-{ti}')
        captions=[n.display() for n in walk(t,'caption')]
        before=[n for n in headings if n.start<t.start]
        rows=[]
        for ri,tr in enumerate(walk(t,'tr'),1):
            if tr.ancestor(lambda n:n.tag=='table') is not t:continue
            cells=[cell(doc,n) for n in tr.children if isinstance(n,Node) and n.tag in {'td','th'}]
            rows.append({'row_ordinal':ri,'header':bool(cells) and all(c['tag']=='th' for c in cells),
                         'attributes':tr.attrs,'cells':cells,'provenance':doc.proof(tr)})
        out.append({'table_ordinal':ti,'table_id':table_id,'attributes':t.attrs,'captions':captions,
                    'nearest_heading':before[-1].display() if before else None,
                    'headers':[[c['text'] for c in r['cells']] for r in rows if r['header']],
                    'rows':rows,'provenance':doc.proof(t)})
    return out

def extract_cards(doc):
    headings=list(walk(doc.root,'h2'));out=[]
    for h in walk(doc.root,'h3'):
        slide=h.ancestor(lambda n:'swiper-slide' in n.attrs.get('class','').split())
        if slide is None:continue
        fields=[]
        for dt in walk(slide,'dt'):
            siblings=dt.parent.children
            idx=siblings.index(dt)
            dd=next((n for n in siblings[idx+1:] if isinstance(n,Node) and n.tag=='dd'),None)
            fields.append({'label':cell(doc,dt),'value':cell(doc,dd) if dd else None})
        before=[n for n in headings if n.start<slide.start]
        out.append({'card_ordinal':len(out)+1,'family_heading':before[-1].display() if before else None,
                    'model':h.display(),'model_raw':h.text(),'model_provenance':doc.proof(h),
                    'fields':fields,'links':[{'text':a.display(),'attributes':a.attrs} for a in walk(slide,'a')],
                    'text':slide.display(),'provenance':doc.proof(slide)})
    return out

def extract_amd(sidecar):
    if sidecar['pdf_page_count']!=len(sidecar['pages']):raise ValueError('incomplete PDF extraction')
    if [p['page'] for p in sidecar['pages']]!=list(range(1,sidecar['pdf_page_count']+1)):raise ValueError('bad page sequence')
    pages=[]
    for p in sidecar['pages']:
        lines=p['text'].splitlines(); tables=[]; current=None
        for i,line in enumerate(lines):
            header=re.search(r'\bPARTNER\s+PRODUCT\b',line)
            if header:
                product_start=line.index('PRODUCT')
                current={'table_ordinal':len(tables)+1,'header_line':i+1,'header_raw':line,
                         'product_column_start':product_start,'lines':[]}
                tables.append(current);continue
            if current is None:continue
            if 'SOLUTION CATALOG:' in line or 'FOR MORE INFORMATION' in line:current=None;continue
            if not line.strip():continue
            # Preserve every token group and continuation line without inferring merged cells.
            spans=[{'start':m.start(),'end':m.end(),'text':m.group()} for m in re.finditer(r'\S(?:.*?\S)?(?= {2,}|$)',line)]
            product=next((s['text'] for s in spans if s['start']==current['product_column_start']),None)
            current['lines'].append({'line':i+1,'raw':line,'spans':spans,
                'partner_column_raw':line[:current['product_column_start']],
                'product_fragment':product,'identity_status':'unreviewed PDF layout; fragment may be continuation',
                'raw_line_sha256':sha(line.encode())})
        pages.append({'page':p['page'],'text_sha256':sha(p['text'].encode()),'tables':tables})
    return {'pages':pages,'partner_mapping':'not inferred; vertically centered and multiline labels retained literally'}

def source_records(base):
    receipts=[json.loads(s) for s in (base/'requests.jsonl').read_text().splitlines()]
    sources=[]
    for r in receipts:
        # Deliberately exclude the empty HTTP301 body, regardless of its .pdf extension.
        if r['is_policy'] or r['status']!=200:continue
        data=(base/r['body_path']).read_bytes()
        if sha(data)!=r['body_sha256']:raise ValueError('capture hash mismatch: '+r['body_path'])
        headers=(base/r['header_path']).read_bytes()
        if sha(headers)!=r['header_sha256']:raise ValueError('header hash mismatch')
        fmt='pdf' if data.startswith(b'%PDF-') else 'html' if b'<html' in data[:3000].lower() else 'unknown'
        sources.append({'dataset':r['dataset'],'url':r['url'],'path':r['body_path'],
                        'sha256':sha(data),'bytes':len(data),'format':fmt,
                        'captured_at':r['finished_at_utc'],'request_sequence':r['sequence']})
    return sources

def extract(base):
    outputs=[]
    for s in source_records(base):
        raw=(base/s['path']).read_bytes()
        if s['format']=='html':
            doc=Document(raw)
            data={'cards':extract_cards(doc)} if s['dataset'].startswith('mitac') else {'tables':extract_tables(doc)}
        elif s['format']=='pdf':
            side=json.loads((base/'amd-instinct-pages.json').read_text())
            if side['pdf_sha256']!=s['sha256']:raise ValueError('PDF/sidecar hash mismatch')
            data=extract_amd(side)
            s['sidecar_sha256']=sha((base/'amd-instinct-pages.json').read_bytes())
            s['extraction_method']=side['extraction_method']
        else:raise ValueError('unknown capture content')
        outputs.append({'source':s,**data})
    return {'schema_version':1,'claim_scope':'literal source discovery; no inferred manufacture, deployment, alias, current support or fitment',
            'sources':outputs}

def discovery_rows(result):
    out=[]
    for ds in result['sources']:
        s=ds['source'];base={'dataset':s['dataset'],'source_url':s['url'],'source_sha256':s['sha256'],'source_path':s['path']}
        for t in ds.get('tables',[]):
            headers=t['headers'][0] if len(t['headers'])==1 else []
            for row in t['rows']:
                if row['header']:continue
                texts=[c['text'] for c in row['cells']]
                safe=len(headers)==len(texts) and len(texts)>=2 and headers[0] in {'Partner','Manufacturer'}
                out.append({**base,'locator':{'table_id':t['table_id'],'row_ordinal':row['row_ordinal']},
                    'partner':texts[0] if safe else None,'partner_role_as_printed':headers[0] if safe else None,
                    'model':texts[1] if safe else None,'cells':texts,
                    'identity_scope':'exact whole printed cell; grouped names remain grouped',
                    'has_spans':any('rowspan' in c['attributes'] or 'colspan' in c['attributes'] for c in row['cells']),
                    'has_identity_superscripts':any(c['superscripts'] for c in row['cells'][:2]),
                    'target_partner_exact':safe and texts[0] in TARGET_PARTNERS})
        for c in ds.get('cards',[]):
            out.append({**base,'locator':{'card_ordinal':c['card_ordinal']},'partner':'MiTAC',
                        'partner_role_as_printed':'product listed on MiTAC Computing index',
                        'model':c['model'],'family':c['family_heading'],'fields':c['fields'],
                        'identity_scope':'exact product card heading; publisher association only','target_partner_exact':True})
        for p in ds.get('pages',[]):
            for t in p['tables']:
                for line in t['lines']:
                    if line['product_fragment']:
                        out.append({**base,'locator':{'page':p['page'],'table_ordinal':t['table_ordinal'],'line':line['line']},
                        'partner':None,'partner_role_as_printed':None,'model':line['product_fragment'],
                        'identity_scope':'unreviewed PDF product fragment; may be continuation',
                        'raw_line':line['raw'],'target_partner_exact':False})
    return out

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--base',type=Path,default=Path(__file__).resolve().parent)
    ap.add_argument('--output',type=Path);args=ap.parse_args();dest=args.output or args.base
    dest.mkdir(parents=True,exist_ok=True)
    result=extract(args.base);rows=discovery_rows(result)
    dump(dest/'bulk-extracted.json',result);dump(dest/'discovery-rows.json',rows)
    summary={'sources':len(result['sources']),'rows':len(rows),'by_dataset':dict(Counter(r['dataset'] for r in rows)),
             'html_tables':sum(len(s.get('tables',[])) for s in result['sources']),
             'html_cards':sum(len(s.get('cards',[])) for s in result['sources']),
             'amd_pages':sum(len(s.get('pages',[])) for s in result['sources'])}
    dump(dest/'extraction-summary.json',summary);print(json.dumps(summary))
if __name__=='__main__':main()

"""Offline acceptance checks for this finite research handoff; no imports/network."""
from pathlib import Path
import hashlib
import json
from urllib import robotparser
from urllib.parse import urljoin, urlsplit
from devicehunt.documents import read_capture
from devicehunt.importers import prepare_curated
from observe import Article

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
def sha(b): return hashlib.sha256(b).hexdigest()
def norm(s): return ' '.join(s.split())

def main():
    records=[json.loads(x) for x in (HERE/'requests.jsonl').read_text().splitlines()]
    actual=[r for r in records if r['status']!=0]
    local_failures=[r for r in records if r['status']==0]
    assert len(actual)==6<=8 and len(local_failures)==1
    assert local_failures[0]['curl_returncode']==6 and local_failures[0]['body_path'] is None
    for r in records:
        assert not r['automatic_redirects'] and r['retries']==0
        for key,hash_key in [('body_path','body_sha256'),('header_path','header_sha256')]:
            if r[key]:assert sha((HERE/r[key]).read_bytes())==r[hash_key]
        if r['policy']:
            p=r['policy'];robots=robotparser.RobotFileParser()
            body=(HERE/p['body_path']).read_bytes()
            assert sha(body)==p['sha256']
            robots.parse(body.decode('utf-8').splitlines())
            assert robots.can_fetch(r['user_agent'],r['url']) and not robots.crawl_delay(r['user_agent'])
        if r['authorization']['kind']=='literal same-origin redirect':
            parent=next(x for x in records if x['sequence']==r['authorization']['parent_sequence'])
            assert r['url']==urljoin(parent['url'],parent['location_not_followed'])
            assert urlsplit(r['url']).netloc==urlsplit(parent['url']).netloc
    pdf=HERE/'sd4100-known_captured_target_checked.pdf'
    pages,_=read_capture((HERE/'sd4100-pages.json').read_bytes(),sha(pdf.read_bytes()))
    assert len(pages)==2
    renders=json.loads((HERE/'render-provenance.json').read_text())
    assert renders['pdf_sha256']==sha(pdf.read_bytes()) and renders['local_render_count']==2<=6
    for r in renders['renders']:assert sha((HERE/r['output']).read_bytes())==r['sha256']
    articles={}
    for key in ('zs3-2','zs3-4'):
        parser=Article();parser.feed((HERE/(key+'-internal-components.html')).read_text())
        saved=json.loads((HERE/(key+'-article.json')).read_text())
        assert parser.tables==saved['tables'] and parser.blocks==saved['blocks']
        articles[key]=parser
    candidate=json.loads((HERE/'candidate.json').read_text())
    sources={s['id']:s for s in candidate['sources']}
    assert len(prepare_curated(HERE/'candidate.json'))==3
    fact_count=0
    for d in candidate['devices']:
        source=sources[d['source']]
        assert sha((HERE/source['snapshot']).read_bytes())==source['original_sha256']
        assert all(role=='documentation_vendor' for _,role in d['affiliations'])
        assert len(d['facts'])==len(d['evidence'])
        for n,(fact,e) in enumerate(zip(d['facts'],d['evidence'])):
            assert e['fact_index']==n and e['fact_name']==fact[0]
            assert e['capture_sha256']==source['original_sha256'] and e['source_url']==source['url']
            loc=e['locator']
            if 'pdf_page' in loc:
                lines=pages[loc['pdf_page']-1]['text'].splitlines()
                assert [lines[i-1] for i in loc['lines_1based']]==loc['raw_lines']
                assert norm(e['literal_quote']) in norm(' '.join(loc['raw_lines']))
                assert sha((ROOT/e['visual_original']).read_bytes())==e['visual_sha256']
            else:
                key='zs3-2' if 'zs3-2' in d['source'] else 'zs3-4'
                a=articles[key]
                if 'literal_cells' in loc:
                    t=next(t for t in a.tables if t['start_line']==loc['table_start_line'])
                    r=next(r for r in t['rows'] if r['start_line']==loc['html_start_line'])
                    assert [c['text'] for c in r['cells']]==loc['literal_cells']
                    assert e['literal_quote']==' '.join(loc['literal_cells'])
                else:
                    b=next(b for b in a.blocks if b['start_line']==loc['html_start_line'])
                    assert b['text']==e['literal_quote'] and b['end_line']==loc['html_end_line']
            fact_count+=1
    assert fact_count==60 and len(candidate['devices'])==4
    order=next(d for d in candidate['devices'] if d['model']=='R4086-F9001-01')
    assert len(order['facts'])==3 and all(e['locator']['pdf_page']==2 for e in order['evidence'])
    assert next(d for d in candidate['devices'] if d['model']=='Oracle ZFS Storage ZS3-2')['identifiers']==[]
    assert next(d for d in candidate['devices'] if d['model']=='Oracle ZFS Storage ZS3-4')['identifiers']==[]
    result=dict(status='passed',actual_HTTP_exchanges=len(actual),request_attempts=len(records),sandbox_DNS_failures=len(local_failures),local_PDF_renders=2,complete_PDF_pages=len(pages),HTML_tables=sum(len(a.tables) for a in articles.values()),candidate_sources=3,candidate_records=4,candidate_facts=fact_count,candidate_sha256=sha((HERE/'candidate.json').read_bytes()),checks=['all capture/header SHA-256 receipts','fresh policy exact paths','literal same-origin redirect','complete source-bound PDF text sidecar','two local render hashes','HTML article/table reproducibility','curated preflight only; no database/import','60 fact evidence locators and source scopes','exact order row receives only its own p2 facts; no component identities/parent identifiers'],network_calls_by_check=0,database_writes_by_check=0)
    (HERE/'checks.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))

if __name__=='__main__':main()

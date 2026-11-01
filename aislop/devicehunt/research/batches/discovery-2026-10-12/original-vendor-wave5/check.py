"""Offline provenance/locator checks; no requests, imports or source mutations."""
from collections import Counter
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import sys
from urllib import robotparser
from urllib.parse import urljoin,urlsplit

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
sys.path.insert(0,str(ROOT))
from devicehunt.documents import read_capture


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    rows=[json.loads(line) for line in (HERE/'requests.jsonl').read_text().splitlines()]
    report=json.loads((HERE/'capture-report.json').read_text())
    plan=json.loads((HERE.parent/'next-leads-r2/plan.json').read_text())
    assert len(rows)==report['actual_HTTP']==9 and len(rows)<=12
    assert [r['sequence'] for r in rows]==list(range(1,len(rows)+1))
    assert len({r['url'] for r in rows})==len(rows)
    for name,key in [('scripts/extract_pdf_pages.py','extractor_sha256'),('devicehunt/documents.py','document_reader_sha256')]:
        assert sha(ROOT/name)==report[key],name
    for name,expected in plan['prior_input_hashes'].items():assert sha(ROOT/name)==expected,name
    assert sha(HERE.parent/'next-leads-r2/plan.json')==report['plan_sha256']
    host_previous={}
    policies={}
    policy_checks=[]
    for row in rows:
        host=urlsplit(row['url']).netloc
        assert row['status'] in (200,302) and row['curl_returncode']==0 and not row['login_marker']
        assert row['automatic_redirects'] is False and row['retries']==0
        assert row['argv'][-1]==row['url'] and '-L' not in row['argv'] and '--location' not in row['argv']
        assert row['argv'][row['argv'].index('--retry')+1]=='0'
        assert sha(HERE/row['body_path'])==row['body_sha256'] and (HERE/row['body_path']).stat().st_size==row['body_bytes']
        assert sha(HERE/row['header_path'])==row['header_sha256']
        headers=(HERE/row['header_path']).read_text()
        assert re.findall(r'^HTTP/\S+ (\d+)',headers,re.M)==[str(row['status'])]
        if row['is_policy']:
            parser=robotparser.RobotFileParser()
            parser.parse((HERE/row['body_path']).read_text().splitlines())
            policies[host]=(parser,row)
        else:
            parser,pin=policies[host]
            assert parser.can_fetch(row['user_agent'],row['url'])
            delay=parser.crawl_delay(row['user_agent']) or parser.crawl_delay('*') or 0
            assert delay==(row['policy']['crawl_delay_seconds'] or 0)
            assert row['policy']['sha256']==pin['body_sha256'] and row['policy']['exact_path_allowed']
            gap=(dt.datetime.fromisoformat(row['started_at_utc'])-dt.datetime.fromisoformat(host_previous[host]['finished_at_utc'])).total_seconds()
            assert gap>=delay
            policy_checks.append(dict(sequence=row['sequence'],URL=row['url'],policy_sha256=pin['body_sha256'],allowed=True,declared_delay_seconds=delay,actual_gap_since_prior_same_host_completion_seconds=gap))
        auth=row['authorization']
        if auth['kind']=='manual saved redirect':
            parent=rows[auth['parent_sequence']-1]
            assert parent['status']==302 and urljoin(parent['url'],parent['location_not_followed'])==row['url']
            assert urlsplit(parent['url']).netloc==host
        host_previous[host]=row
    assert len(policies)==2 and Counter(r['status'] for r in rows)==Counter({200:7,302:2})
    source_meta={}
    page_text={}
    for pdf,sidecar in [('as6700-quick-start.pdf','as6700-pages.json'),('as5710-as5712-ds-r09.pdf','as5712-pages.json')]:
        body=(HERE/sidecar).read_bytes()
        pages,metadata=read_capture(body,sha(HERE/pdf))
        assert metadata['extractor_version']==metadata['pdfinfo_version']=='26.06.0'
        page_text[pdf]=pages
        source_meta[pdf]=dict(sha256=sha(HERE/pdf),bytes=(HERE/pdf).stat().st_size,sidecar=sidecar,sidecar_sha256=sha(HERE/sidecar),pages=len(pages),metadata=metadata)
    assert sum(s['pages'] for s in source_meta.values())==6
    renders=json.loads((HERE/'render-provenance.json').read_text())
    assert renders['local_rendered_pages']==sum(len(r['pages']) for r in renders['renders'])==7 and renders['local_rendered_pages']<=8
    images=[]
    for render in renders['renders']:
        assert sha(HERE/render['source'])==render['pdf_sha256'] and render['returncode']==0
        for image in render['images']:
            assert sha(HERE/image['path'])==image['sha256'];images.append(image['path'])
    assert len(images)==7 and len(set(images))==7
    findings=json.loads((HERE/'findings.json').read_text())
    html=json.loads((HERE/'oracle-html-observations.json').read_text())
    for observation in findings['observations']:
        if 'pdf_page' in observation:
            lines=page_text[observation['source']][observation['pdf_page']-1]['text'].splitlines()
            assert [lines[n-1] for n in observation['lines_1based']]==observation['raw_lines']
            assert observation['visual_page'] in images
        elif 'raw_block' in observation:
            assert html['blocks'][observation['locator']['article_block_ordinal_zero_based']]['text']==observation['raw_block']
        else:assert observation['raw_text'] in html['article_text']
    for row in findings['ordering_rows']:
        text=page_text[row['source']][row['pdf_page']-1]['text'].splitlines()
        assert text[row['line_1based']-1]==row['raw_line']
        assert re.split(r'\s{2,}',row['raw_line'].strip())==row['raw_cells_from_embedded_text']
        assert row['visual_page'] in images
    assert len(findings['observations'])==36 and len(findings['ordering_rows'])==38
    assert sum(r['pdf_page']==3 for r in findings['ordering_rows'])==24
    assert sum(r.get('model_text_render_conflict',False) for r in findings['ordering_rows'])==2
    source_meta['oracle7420-specifications.html']=dict(sha256=sha(HERE/'oracle7420-specifications.html'),bytes=(HERE/'oracle7420-specifications.html').stat().st_size,method='original article/table markup; stdlib text aid, no screenshot')
    checks=dict(at_utc=dt.datetime.now(dt.timezone.utc).isoformat(),status='PASS finite discovery checks; independent review pending',actual_HTTP=9,total_HTTP_cap=12,
                statuses={'200':7,'302':2},standalone_policy_inclusive_counts={'oracle':2,'as6700':4,'as5712':4},policies=policy_checks,sources=source_meta,
                complete_PDF_pages=6,distinct_PDF_pages_visually_reviewed=6,local_renders=7,local_render_cap=8,visual_files_viewed=images,
                observations=36,ordering_rows=38,ordering_configuration_rows=24,ordering_FRU_service_accessory_rows=14,unresolved_fan_model_rows=2,
                raw_locators_verified=74,no_candidate=True,no_import=True,OCR=False,searches=0,remote_screenshots=0,retries=0,
                unchanged_frozen_utilities=True,prior_plan_inputs_unchanged=True,
                artifacts={n:sha(HERE/n) for n in ('requests.jsonl','capture-report.json','published-downloads.json','render-provenance.json','findings.json','oracle-html-observations.json','fetch.py','observe.py')},
                scope='Only new original-vendor-wave5 artifacts written; raw bytes immutable, no production/sharedDB/manifest/archive/export/currentdocs changes, network expansion, installs or delegation')
    (HERE/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(json.dumps({k:v for k,v in checks.items() if k not in ('sources','policies','artifacts','visual_files_viewed')},indent=2))


if __name__=='__main__':main()

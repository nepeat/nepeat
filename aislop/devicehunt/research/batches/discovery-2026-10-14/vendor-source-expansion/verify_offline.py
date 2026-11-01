"""Independent raw-link recomputation, deterministic parse and read-only DB checks."""
from pathlib import Path
from collections import Counter, defaultdict
import argparse
import html
import json
import re
import sqlite3
import subprocess
import sys
from urllib.parse import urlsplit, urljoin, urlunsplit
import xml.etree.ElementTree as ET
from offline_parser import HERE, ROOT, sha, build
from build_discovery_db import search


def independent_keys(config):
    result=[]
    for source in config['sources']:
        for capture in source['inputs']:
            path=ROOT/capture['path']
            assert sha(path)==capture['sha256']
            body=path.read_text()
            if capture['format']=='xml':
                links=[n.text for n in ET.fromstring(body).findall('{*}url/{*}loc')]
            else:
                links=[]
                for raw in re.findall(r'<a\b[^>]*>',body,re.I):
                    h=re.search(r'\bhref\s*=\s*[\"\']([^\"\']*)[\"\']',raw,re.I)
                    if not h: continue
                    if source.get('required_anchor_class'):
                        cl=re.search(r'\bclass\s*=\s*[\"\']([^\"\']*)[\"\']',raw,re.I)
                        if not cl or source['required_anchor_class'] not in cl[1].split(): continue
                    url=urljoin(capture['url'],html.unescape(h[1]))
                    p=urlsplit(url)
                    if p.scheme not in ('http','https'): continue
                    if source.get('same_host') and p.netloc!=urlsplit(capture['url']).netloc: continue
                    if source.get('path_pattern') and not re.search(source['path_pattern'],p.path): continue
                    links.append(url)
            result.extend((source['id'],capture['path'],u) for u in links)
    return Counter(result)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check-only',action='store_true',help='Recheck immutable existing outputs without writing artifacts')
    args=parser.parse_args()
    config=json.loads((HERE/'inputs-ready.json').read_text())
    dataset=json.loads((HERE/'discovery-ready.json').read_text())
    rawhash={c['path']:sha(ROOT/c['path']) for c in dataset['inputs']}
    before=sha(ROOT/'data/devices.sqlite')
    manifest_before=sha(ROOT/'data/import-manifest.json')
    actual=Counter((r['source'],r['capture_path'],r['url']) for r in dataset['rows'])
    independent=independent_keys(config)
    assert actual==independent, (actual-independent,independent-actual)
    assert json.loads(json.dumps(build(config),ensure_ascii=False))==dataset
    groups=defaultdict(list)
    for r in dataset['rows']:
        if r['source']=='edgecore_product_downloads': groups[r['endpoint_comparison_key']].append(r)
    inventory=[]
    for key,rows in sorted(groups.items()):
        inventory.append(dict(endpoint_comparison_key=key,raw_urls=list(dict.fromkeys(r['url'] for r in rows)),
                              printed_document_headings=list(dict.fromkeys(r['document_heading'] for r in rows)),
                              printed_document_metadata=list(dict.fromkeys(r['document_metadata'] for r in rows)),
                              catalog_endpoint_present=any(r['endpoint_in_catalog'] for r in rows),
                              restricted=any(r['restrictions'] for r in rows),
                              revision_label_url_disagreement=any(r['revision_label_url_disagreement'] for r in rows),
                              row_references=[dict(capture_path=r['capture_path'],row_number=r['row_number'],source_line=r['source_line']) for r in rows]))
    if args.check_only:
        assert json.loads((HERE/'edgecore-document-inventory.json').read_text())==inventory
    else:
        with (HERE/'edgecore-document-inventory.json').open('x') as f: json.dump(inventory,f,indent=2,ensure_ascii=False);f.write('\n')
    private=HERE/'discovery.sqlite'; private_before=sha(private)
    query_hits=search(private,'"AS7326-56X"')
    assert query_hits and all(r['source_role']=='product_page_link_publisher' for r in query_hits)
    with sqlite3.connect(private.as_uri()+'?mode=ro',uri=True) as db:
        assert db.execute('SELECT count(*) FROM links').fetchone()[0]==len(dataset['rows'])
        assert db.execute('SELECT count(*) FROM captures').fetchone()[0]==len(dataset['inputs'])
        assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
        assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
    assert sha(private)==private_before
    with sqlite3.connect((ROOT/'data/devices.sqlite').as_uri()+'?mode=ro',uri=True) as db:
        catalog_counts={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','facts']}
    test=subprocess.run([sys.executable,'-B','-m','unittest','discover','-s',str(HERE),'-p','test_offline_parser.py','-v'],capture_output=True,text=True)
    if not args.check_only:
        with (HERE/'tests-final.log').open('x') as f:f.write(test.stdout+test.stderr)
    assert test.returncode==0,test.stderr
    assert {c['path']:sha(ROOT/c['path']) for c in dataset['inputs']}==rawhash
    assert sha(ROOT/'data/devices.sqlite')==before
    assert sha(ROOT/'data/import-manifest.json')==manifest_before
    receipts=[json.loads((HERE/(json.loads(line)['path']+'.receipt.json')).read_text()) for line in (HERE/'requests.jsonl').read_text().splitlines()]
    summary_by_source={}
    for source in config['sources']:
        rows=[r for r in dataset['rows'] if r['source']==source['id']]
        summary_by_source[source['id']]=dict(rows=len(rows),raw_urls=len({r['url'] for r in rows}),
            endpoints=len({r['endpoint_comparison_key'] for r in rows}),endpoints_absent_catalog=len({r['endpoint_comparison_key'] for r in rows if not r['endpoint_in_catalog']}),
            captures_with_links=len({r['capture_path'] for r in rows}),complete_label_model_match_rows=sum(bool(r['complete_label_model_matches']) for r in rows))
    checks=dict(outcome='pass',fixture_tests=8,independent_raw_link_recomputation='equal including duplicate multiplicities',
                deterministic_reparse='equal',input_capture_hashes_verified=len(rawhash),input_capture_bytes_unchanged=True,
                private_sqlite=dict(path=str(private.relative_to(ROOT)),sha256=private_before,integrity='ok',foreign_key_violations=0,fts_probe_hits=len(query_hits),read_only_search_hash_unchanged=True),
                catalog_sha256_unchanged=before,catalog_counts=catalog_counts,manifest_sha256_unchanged=manifest_before,
                conservative_http_attempts=len(receipts),actual_http_responses=sum('status' in r for r in receipts),web_search_queries=3,
                source_summary=summary_by_source,revision_disagreement_rows=sum(r.get('revision_label_url_disagreement',False) for r in dataset['rows']))
    if args.check_only:
        assert json.loads((HERE/'checks.json').read_text())==checks
    else:
        with (HERE/'checks.json').open('x') as f:json.dump(checks,f,indent=2);f.write('\n')
    print(json.dumps(checks,indent=2))


if __name__=='__main__': main()

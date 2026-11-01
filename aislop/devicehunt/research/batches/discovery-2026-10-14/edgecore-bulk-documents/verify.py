"""Offline corpus provenance, entry validation, transport fixtures and preservation."""
from pathlib import Path
from collections import Counter
from urllib.parse import urlsplit,parse_qs
import argparse,hashlib,html,json,re,shutil,sqlite3,subprocess,sys,tempfile
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
sys.path.insert(0,str(ROOT))
from devicehunt.documents import read_capture
from devicehunt.reference_documents import prepare


def sha(path):
 with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()


def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check-only',action='store_true');args=parser.parse_args()
 plan=json.loads((HERE/'selection-plan.json').read_text());results=json.loads((HERE/'collection-results.json').read_text());extraction=json.loads((HERE/'extraction-report.json').read_text());candidate=json.loads((HERE/'candidate-reference-entries.json').read_text());baseline=json.loads((HERE/'catalog-baseline.json').read_text())
 assert results['selection_plan_sha256']==sha(HERE/'selection-plan.json')
 assert candidate['artifact_kind']=='reference-document-additions-only'
 assert results['outcome']=='finite_selection_complete' and results['stop_reason'] is None
 assert len(plan['selections'])==9 and len(candidate['documents'])==9
 ledger=[json.loads(line) for line in (HERE/'requests.jsonl').read_text().splitlines()]
 receipts=[]
 for sequence,reservation in enumerate(ledger,1):
  receipt=json.loads((HERE/(reservation['body_path']+'.receipt.json')).read_text())
  assert receipt['sequence']==sequence and receipt['url']==reservation['url'] and receipt['purpose']==reservation['purpose']
  assert receipt['state']=='completed' and receipt['automatic_redirects'] is False
  assert sha(HERE/receipt['body_path'])==receipt['sha256']
  assert (HERE/receipt['body_path']).stat().st_size==receipt['bytes']
  assert receipt['status'] in (200,302)
  if sequence>1:assert receipt['policy_sha256']==sha(HERE/'robots.txt') and receipt['exact_path_allowed'] is True
  receipts.append(receipt)
 assert len(receipts)==28 and len({r['url'] for r in receipts})==28
 assert Counter(r['status'] for r in receipts)==Counter({200:19,302:9})
 assert receipts[0]['purpose']=='policy'
 prior=json.loads((HERE/'prior-pdf-inventory.json').read_text())
 priorhashes={r['sha256'] for r in prior['local_pdfs']}|{r['sha256'] for r in prior['catalog_reference_documents']}
 oldurls=json.loads((HERE/'prior-url-inventory.json').read_text())['edgecore_urls']
 oldkeys={r['comparison_key'] for r in oldurls}
 entries=candidate['documents'];newhashes=set();pdfbytes=0;pages=0;textpages=0;warnings=[]
 for chosen,result,entry in zip(plan['selections'],results['results'],entries,strict=True):
  n=chosen['selection_number'];assert result['outcome']=='new_pdf'
  landing,download,redirect=receipts[1+(n-1)*3:1+n*3]
  assert landing['url']==chosen['landing_url'] and landing['status']==200
  published=json.loads((HERE/f'{n:02d}-published-link.json').read_text())
  assert published['landing_sha256']==landing['sha256'] and published['fresh_heading']==chosen['printed_document_heading']
  # Independent literal href extraction verifies the fresh URL was actually published.
  hrefs=[html.unescape(x) for x in re.findall(r'<a\b[^>]*\bhref=[\"\']([^\"\']+)[\"\']', (HERE/landing['body_path']).read_text(),re.I)]
  assert download['url']==published['published_download_url'] and download['url'] in hrefs
  assert download['url'] not in {r['url'] for r in chosen['source_rows']}
  assert set(parse_qs(urlsplit(download['url']).query)['wpdmdl'])==set(published['expected_wpdmdl_values'])
  loc=next(v for k,v in download['headers'].items() if k.lower()=='location')
  assert redirect['url']==loc==entry['url']==result['response_url'] and redirect['status']==200
  for r in [landing,download,redirect]:
   p=urlsplit(r['url']);assert p.scheme=='https' and p.netloc=='www.edge-core.com'
   assert p.scheme+'://'+p.netloc+p.path not in oldkeys
  pdf=HERE/entry['pdf']['path'];sidecar=HERE/entry['text']['path'];digest=sha(pdf)
  assert pdf.read_bytes().startswith(b'%PDF-') and digest==entry['pdf']['sha256']==result['sha256']==redirect['sha256']
  assert pdf.read_bytes()==(HERE/redirect['body_path']).read_bytes()
  assert digest not in priorhashes and digest not in newhashes;newhashes.add(digest)
  assert sha(sidecar)==entry['text']['sha256']
  pagelist,metadata=read_capture(sidecar.read_bytes(),digest)
  assert len(pagelist)==entry['page_count'] and [p['page'] for p in pagelist]==list(range(1,len(pagelist)+1))
  raw=json.loads(sidecar.read_text())
  warnings.extend(dict(pdf=pdf.name,argv=cmd['argv'],stderr=cmd['stderr']) for cmd in raw['provenance']['commands'] if cmd['returncode']==0 and cmd['stderr'] and '-v' not in cmd['argv'])
  pdfbytes+=pdf.stat().st_size;pages+=len(pagelist);textpages+=sum(bool(p['text'].strip()) for p in pagelist)
 assert pages==extraction['pages']==40 and textpages==extraction['pages_with_text']
 with tempfile.TemporaryDirectory(prefix='entry-verify-',dir=HERE) as temporary:
  root=Path(temporary)
  for entry in entries:
   for kind in ['pdf','text']:shutil.copyfile(HERE/entry[kind]['path'],root/entry[kind]['path'])
  bundle=root/'verify-only.json';bundle.write_text(json.dumps(dict(schema_version=1,collection='public-device-references-v1',documents=entries)))
  _,ready=prepare(bundle);assert len(ready)==9
 graphroot=ROOT/'research/batches/discovery-2026-10-14/vendor-source-expansion'
 graphfiles=0
 for line in (graphroot/'SHA256SUMS').read_text().splitlines():
  digest,name=line.split('  ',1);assert sha(graphroot/name)==digest;graphfiles+=1
 assert sha(ROOT/'data/devices.sqlite')==baseline['database_sha256']
 assert sha(ROOT/'data/import-manifest.json')==baseline['manifest_sha256']
 with sqlite3.connect((ROOT/'data/devices.sqlite').as_uri()+'?mode=ro',uri=True) as db:
  assert {t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in baseline['counts']}==baseline['counts']
 tests=subprocess.run([sys.executable,'-B','-m','unittest','discover','-s',str(HERE),'-p','test_collect.py','-v'],capture_output=True,text=True)
 assert tests.returncode==0,tests.stdout+tests.stderr
 check=dict(outcome='pass',selected_resources=9,new_unique_pdfs=9,pdf_bytes=pdfbytes,complete_sidecars=9,pages=pages,pages_with_text=textpages,actual_http_exchanges=28,conservative_attempts=28,http_status_counts={'200':19,'302':9},fresh_policy_requests=1,fresh_landings=9,fresh_published_downloads=9,explicit_same_origin_redirects=9,stale_graph_tokens_used=0,known_route_overlaps=0,prior_local_pdfs_audited=len(prior['local_pdfs']),duplicate_prior_or_batch_pdf_hashes=0,receipt_body_hashes_verified=28,all_reference_candidate_entries_prepare='pass in temporary sibling-file bundle; no import',fixture_tests=4,successful_extraction_warning_rows=warnings,previous_discovery_artifact_hashes_unchanged=graphfiles,catalog_sha256_unchanged=baseline['database_sha256'],manifest_sha256_unchanged=baseline['manifest_sha256'],catalog_counts_unchanged=baseline['counts'],visual_page_review=False,manual_device_facts=0)
 if args.check_only:assert json.loads((HERE/'checks.json').read_text())==check
 else:
  with (HERE/'checks.json').open('x') as f:json.dump(check,f,indent=2);f.write('\n')
  with (HERE/'tests.log').open('x') as f:f.write(tests.stdout+tests.stderr)
 print(json.dumps(check,indent=2))


if __name__=='__main__':main()

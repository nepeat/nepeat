"""Save a finite, offline-audited selection before any HTTP request."""
from pathlib import Path
from urllib.parse import urlsplit,urlunsplit
import hashlib,json,re,sqlite3

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
GRAPH=ROOT/'research/batches/discovery-2026-10-14/vendor-source-expansion/discovery-ready.json'

def sha(path):
 with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def save(name,obj):
 with (HERE/name).open('x') as f:json.dump(obj,f,indent=2,ensure_ascii=False);f.write('\n')

def key(url):
 p=urlsplit(url);return urlunsplit((p.scheme.lower(),p.netloc.lower(),p.path,'',''))

def walk(value,path,urls,failures):
 if isinstance(value,dict):
  candidates=[v for k,v in value.items() if k.lower() in ('url','requested_url','final_url','location') and isinstance(v,str) and urlsplit(v).netloc=='www.edge-core.com']
  if candidates:
   status=value.get('status',value.get('status_code'))
   urls.extend(dict(url=u,comparison_key=key(u),status=status,ledger=path) for u in candidates)
   if str(status) in ('402','403','404','429') or value.get('error'):
    failures.extend(dict(url=u,status=status,error=value.get('error'),ledger=path) for u in candidates)
  for v in value.values():walk(v,path,urls,failures)
 elif isinstance(value,list):
  for v in value:walk(v,path,urls,failures)


def main():
 assert sha(GRAPH)=='678a3598a2cd3677d8bf883ade34f41da8661bb89a2a94fa7d0c25ee9299e85e'
 auditfiles=[p for base in [ROOT/'data/raw',ROOT/'research/batches'] for p in base.rglob('*') if p.is_file() and not p.is_relative_to(HERE) and p.suffix in ('.json','.jsonl') and any(s in p.name for s in ['request','fetch','capture-ledger'])]
 urls,failures=[],[]
 for p in sorted(auditfiles):
  data=[json.loads(line) for line in p.read_text().splitlines() if line] if p.suffix=='.jsonl' else json.loads(p.read_text())
  walk(data,str(p.relative_to(ROOT)),urls,failures)
 urlkeys={r['comparison_key'] for r in urls}
 pdfs=[]
 for base in [ROOT/'data/raw',ROOT/'research/batches',ROOT/'sources']:
  for p in sorted(base.rglob('*.pdf')):
   if not p.is_relative_to(HERE):pdfs.append(dict(path=str(p.relative_to(ROOT)),sha256=sha(p),bytes=p.stat().st_size))
 with sqlite3.connect((ROOT/'data/devices.sqlite').as_uri()+'?mode=ro',uri=True) as db:
  db.execute('PRAGMA query_only=ON')
  catalogurls={r[0] for r in db.execute('SELECT url FROM sources')}|{r[0] for r in db.execute('SELECT source_url FROM records')}
  docrows=[dict(zip(['url','sha256','title'],r)) for r in db.execute('SELECT source_url,pdf_sha256,title FROM documents')]
  baseline=dict(database_sha256=sha(ROOT/'data/devices.sqlite'),manifest_sha256=sha(ROOT/'data/import-manifest.json'),counts={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','facts','documents']})
 graph=json.loads(GRAPH.read_text())
 slugs=['agr400-as7946-30xb-ds-r05','ags8200-datasheet-r01','ais800-32d-32o-datasheet-r03','cor550-as7926-40xkfb-ds-r07','csr440-as7535-28xb-ds-r05','dcs511-as9737-32db-datasheet-r02','ecis4500-poe-series-ds-r04','exp400-32x-datasheet-r01','saf4100i-ds-r03']
 selected=[]
 for slug in slugs:
  rows=[r for r in graph['rows'] if r['source']=='edgecore_product_downloads' and r['endpoint_comparison_key']=='https://www.edge-core.com/download/'+slug+'/']
  assert rows
  assert all(not r['restrictions'] and not r['revision_label_url_disagreement'] and r.get('document_metadata','').startswith('Datasheet') for r in rows)
  r=rows[0];landing=r['endpoint_comparison_key']
  assert landing not in urlkeys and landing not in {key(u) for u in catalogurls}
  assert not re.search(r'AS7710|AS7760|AS7761|DCS203|DCS510|AS9716|AS7312|AS4630-54NPE',r['document_heading'],re.I)
  selected.append(dict(selection_number=len(selected)+1,selection_bucket=r['document_heading'].split()[0],slug=slug,printed_document_heading=r['document_heading'],printed_document_metadata=r['document_metadata'],landing_url=landing,source_rows=[{k:x.get(k) for k in ['source_url','capture_path','capture_sha256','source_line','row_number','url','href_raw','wpdmdl_values','document_heading_raw','document_metadata_raw']} for x in rows],novelty=dict(landing_endpoint_in_catalog=False,landing_endpoint_in_audited_ledgers=False,complete_heading_in_existing_reference_titles=any(x['title']==r['document_heading'] for x in docrows)),claims='Document discovery only; printed family strings are selection buckets, not inferred aliases, manufacture or specifications.'))
 save('prior-url-inventory.json',dict(ledger_files=[dict(path=str(p.relative_to(ROOT)),sha256=sha(p)) for p in sorted(auditfiles)],edgecore_urls=urls))
 save('prior-pdf-inventory.json',dict(local_pdfs=pdfs,catalog_reference_documents=docrows))
 save('known-failure-audit.json',dict(audited_ledgers=len(auditfiles),prior_edgecore_failures=failures,notes=[dict(path=p,sha256=sha(ROOT/p)) for p in ['docs/evidence-policy.md','research/owned-hardware-gaps.md','research/as7760-certificate-lead.md','research/batches/hunt-2026-10-10/edgecore/handoff.md','research/batches/discovery-2026-10-12/next-leads/handoff.md']],excluded_patterns=['AS7710/AS7760/AS7761 quick-start and certificate families','DCS203 R09','DCS510/AS9716 R09 (entire model family avoided here)','AS7312 R02 (entire model family avoided here)','AS4630-54NPE R04 (entire model family avoided here)','all graph quarantine flags or revision-label/URL disagreements','any already captured landing/download/PDF route; all prior local PDF hashes'],selected_route_overlap_with_audited_urls=0))
 save('catalog-baseline.json',baseline)
 save('selection-plan.json',dict(owner='/root/vendor_bulk_full',scope='new edgecore-bulk-documents directory only; previous corpus and live state immutable',source_graph=dict(path=str(GRAPH.relative_to(ROOT)),sha256=sha(GRAPH)),selection_count=len(selected),maximum_resources=12,maximum_attempts=28,maximum_actual_http_exchanges=28,expected_worst_case_exchanges='one fresh robots policy + nine times (landing + fresh published download + one same-origin redirect) = 28',selection_strategy='Nine distinct printed family buckets, one datasheet per bucket; mix newer and surplus-useful families without per-device fact curation. Nine fits the strict28-exchange cap even if all download routes redirect.',transport='One explicit GET at a time, no auto redirect/retry; stop Edge-Core source on 402/403/404/timeout/transport failure, unexpected redirect or robots denial; stop entire run on429. HTML-only download outcomes retained, never counted as PDFs.',deduplication='Reject prior landing/download/PDF URLs before requests; reject any published redirect target previously captured; compare successful PDF SHA256 against all165 audited preexisting local PDFs and catalog document hashes.',extraction='Complete scripts/extract_pdf_pages.py Poppler sidecars only; candidate reference entries are additions, never a replacement complete collection; no model/spec/role inference or import.',selections=selected))
 print(json.dumps(dict(selected=len(selected),audited_ledgers=len(auditfiles),local_pdfs=len(pdfs),edgecore_url_rows=len(urls),edgecore_failures=len(failures)),indent=2))

if __name__=='__main__':main()

"""Offline exact-row parser for public Red Hat certified-system listing captures."""
from pathlib import Path
import argparse,collections,hashlib,json,sqlite3
OUT=Path(__file__).resolve().parent
ROOT=OUT.parents[3]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def parse_listing(data,partner,source):
 response=data['response'];docs=response['docs']
 if response['start']!=0 or not isinstance(response['numFound'],int) or response['numFound']<len(docs):raise ValueError('Invalid listing envelope')
 records=[];excluded=[];seen=set()
 for position,d in enumerate(docs):
  if d.get('partnerName')!=partner or d.get('documentKind')!='CertifiedHardware':raise ValueError('Partner/type mismatch')
  key=str(d['id'])
  if key in seen:raise ValueError('Duplicate catalog id')
  seen.add(key)
  types=d.get('system_types',[])
  if d.get('certified_category')!='System' or not {'Server','Edge System'}.intersection(types):
   excluded.append({'row':position,'id':key,'title':d.get('partnerProductName'),'category':d.get('certified_category'),'system_types':types,'reason':'Not an explicitly listed server or edge system'})
   continue
  model=d.get('model');title=d.get('partnerProductName');url=d.get('view_uri')
  if not all(isinstance(v,str) and v.strip() for v in (model,title,url)):raise ValueError('Missing exact identity or source URL')
  if not url.startswith('https://catalog.redhat.com/hardware/system/detail/'):raise ValueError('Unexpected hardware detail URL')
  facts=[['redhat_cert_catalog_id',key],['redhat_cert_partner_name_as_printed',partner],['redhat_cert_model_field_as_printed',model],['redhat_cert_product_name_as_printed',title],['redhat_cert_listing_category_as_printed',d['certified_category']],['redhat_cert_listing_row',str(position)],['redhat_cert_listing_capture',source['snapshot']+' sha256 '+source['original_sha256']]]
  for field,name in [('make','make_field_as_printed'),('system_types','system_types_listing'),('certification_level','certification_level_listing'),('certified_versions','certified_versions_listing'),('certified_architectures','architectures_listing'),('createdDate','listing_created'),('lastModifiedDate','listing_last_modified')]:
   if field in d:facts.append(['redhat_cert_'+name,json.dumps(d[field],ensure_ascii=False) if isinstance(d[field],(list,dict)) else str(d[field])])
  records.append({'source':source['id'],'key':key,'brand':partner,'model':model,'title':title,'kind':'server' if 'Server' in types else 'edge system','url':url,'affiliations':[[partner,'listing_holder']],'identifiers':[['redhat_catalog_id',key],['redhat_certified_product',title]],'facts':facts,'hyperscaler':[]})
 return records,{'numFound':response['numFound'],'captured':len(docs),'complete_query':len(docs)==response['numFound'],'accepted_records':len(records),'excluded':excluded}
def build(directory=OUT):
 plan=json.loads((directory/'plan.json').read_text());receipts={r['path']:r for r in map(json.loads,(directory/'requests.jsonl').read_text().splitlines())}
 candidate={'sources':[],'devices':[]};coverage=[]
 for i,partner in enumerate(plan['partners'],1):
  name=f'partner-{i:02}.json';p=directory/name;r=receipts[name]
  if r['status']!='200' or r['exit_code']!=0 or sha(p)!=r['sha256']:raise ValueError('Capture integrity mismatch')
  source={'id':f'redhat-certified-vendor-expansion-{i:02}-2026-10-14','url':r['url'],'tier':'certification_body','license':'Public factual certification listing; original upstream rights retained; local research only.','coverage':'Exact captured public certification-listing rows only. Brand is the printed listing-holder identity namespace, not physical manufacturer or product branding. make/model/title are independently preserved; no alias, parent-company, ODM, deployment, current support, hardware configuration, firmware entitlement or auction-unit claim. Server and Edge System categories retained, other rows remain raw only. No detail pages were fetched. Query completeness is recorded independently of overall historical coverage.','snapshot':name,'original_sha256':r['sha256'],'captured_at_utc':r['utc']}
  records,summary=parse_listing(json.loads(p.read_text()),partner,source)
  candidate['sources'].append(source);candidate['devices'].extend(records);coverage.append({'partner':partner,'source':source['id'],**summary})
 return candidate,coverage
if __name__=='__main__':
 ap=argparse.ArgumentParser();ap.add_argument('--output',type=Path,default=OUT/'candidate.json');args=ap.parse_args()
 candidate,coverage=build()
 for path,value in [(args.output,candidate),(OUT/'coverage.json',coverage)]:
  with path.open('x') as f:json.dump(value,f,indent=2,ensure_ascii=False);f.write('\n')
 db=ROOT/'data/devices.sqlite';before=sha(db)
 with sqlite3.connect(db.resolve().as_uri()+'?mode=ro',uri=True) as c:
  rows=[]
  for d in candidate['devices']:
   hits=c.execute('SELECT id,brand,model FROM devices WHERE lower(brand)=lower(?) AND lower(model)=lower(?)',(d['brand'],d['model'])).fetchall()
   cert=c.execute("SELECT r.id,r.source_id FROM records r JOIN identifiers i ON i.record_id=r.id WHERE i.kind='redhat_catalog_id' AND i.value=?",(d['key'],)).fetchall()
   rows.append({'brand':d['brand'],'model':d['model'],'catalog_id':d['key'],'exact_identity_hits':hits,'catalog_id_hits':cert})
 assert sha(db)==before
 report={'database_sha256_unchanged':before,'sources':len(candidate['sources']),'records':len(rows),'facts':sum(len(d['facts']) for d in candidate['devices']),'distinct_identities':len({(d['brand'],d['model'].casefold()) for d in candidate['devices']}),'matching_identity_records':sum(bool(r['exact_identity_hits']) for r in rows),'matching_catalog_ids':sum(bool(r['catalog_id_hits']) for r in rows),'coverage':coverage,'rows':rows}
 with (OUT/'novelty.json').open('x') as f:json.dump(report,f,indent=2,ensure_ascii=False);f.write('\n')
 print(json.dumps({k:v for k,v in report.items() if k not in ('coverage','rows')}))

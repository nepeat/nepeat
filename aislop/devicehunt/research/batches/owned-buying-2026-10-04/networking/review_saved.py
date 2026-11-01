"""Offline, read-only review of a finite six-model saved-evidence set."""
import csv, hashlib, html, io, json, pathlib, re, sqlite3, zipfile
from html.parser import HTMLParser
ROOT=pathlib.Path(__file__).resolve().parents[4]
OUT=ROOT/'research/batches/owned-buying-2026-10-04/networking'
MODELS=['CS8200-32X-UN-00','CS8210-32x-un-00','DMC-12-48R-AS0','7760-32X-A-AC-F','AZ4444-52X-DC-10','AS7772-32X']
def digest(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def capture(p):
 p=pathlib.Path(p);return {'local_path':str(p.resolve()),'sha256':digest(p)}
def load(p):return json.loads(pathlib.Path(p).read_text())
class Rows(HTMLParser):
 def __init__(self):super().__init__();self.rows=[];self.cells=None;self.cell=None
 def handle_starttag(self,t,a):
  if t=='tr':self.cells=[]
  if t in ('td','th') and self.cells is not None:self.cell=[]
 def handle_data(self,d):
  if self.cell is not None:self.cell.append(d)
 def handle_endtag(self,t):
  if t in ('td','th') and self.cell is not None:self.cells.append(re.sub(r'\s+',' ',' '.join(self.cell)).strip());self.cell=None
  if t=='tr' and self.cells is not None:self.rows.append(self.cells);self.cells=None
c=sqlite3.connect(f'file:{ROOT}/data/devices.sqlite?mode=ro',uri=True);c.row_factory=sqlite3.Row
before=digest(ROOT/'data/devices.sqlite')
parser=Rows();parser.feed((ROOT/'data/raw/tec-equipment.html').read_text())
z=zipfile.ZipFile(ROOT/'data/raw/anatel-products.zip')
assert digest(ROOT/'data/raw/anatel-products.zip')==load(ROOT/'data/raw/anatel-bundle.json')['files']['before']['sha256']
anatel=list(csv.DictReader(io.StringIO(z.read('Produtos_Homologados_Anatel.csv').decode('utf-8-sig')),delimiter=';'))
checks=[];results=[]
for query in MODELS:
 item={'owned_query':query,'exact_catalog_matches':[],'already_imported_statements':[],'newly_reviewed_unimported_statements':[],'unknowns':[],'excluded_near_models':[]}
 for dev in c.execute('select * from devices where lower(model)=lower(?)',(query,)):
  dd=dict(dev);dd['case_match']='literal' if dev['model']==query else 'case-insensitive only; source printed case retained';dd['records']=[]
  for r in c.execute('select * from records where device_id=? order by source_id,id',(dev['id'],)):
   src=c.execute('select * from sources where id=?',(r['source_id'],)).fetchone();raw=json.loads(r['raw_json']);p=pathlib.Path(src['cache_path']);ref={'source_id':r['source_id'],'record_id':r['id'],'source_key':r['source_key'],'source_url':r['source_url'],'source_tier':src['tier'],'capture':capture(p),'field_reference':None,'review_method':None}
   assert ref['capture']['sha256']==src['sha256']
   facts=[dict(f) for f in c.execute('select name,value from facts where record_id=? order by name,value',(r['id'],))]
   ids=[dict(i) for i in c.execute('select kind,value from identifiers where record_id=?',(r['id'],))]
   roles=[dict(a) for a in c.execute('select company,role from affiliations where record_id=?',(r['id'],))]
   sid=r['source_id'];material=None
   if sid.startswith('rra-official-query-'):
    b=load(p);d=next(d for d in b['details'] if d['certificate']==raw['certificate_number']);detail=p.parent/d['path'];assert digest(detail)==d['sha256'];txt=html.unescape(re.sub('<[^>]+>',' ',detail.read_text()));txt=re.sub(r'\s+',' ',txt);assert dev['model'] in txt
    ref['detail_capture']=capture(detail);ref['field_reference']='labelled 모델명 / 파생모델명 / 제조자 / 기타 cells';ref['review_method']='read original saved detail HTML; strip tags and collapse whitespace; compare labelled fields to imported raw_json';material=txt[txt.find('상호'):]
   elif sid=='tec-mtcte':
    row=next(row for row in parser.rows if row and row[0]==raw['cells']['S.No.']);assert dev['model'] in row;ref['field_reference']='HTML table S.No. '+row[0]+', Product Model Name / OEM Name';ref['review_method']='standard-library HTMLParser table cell extraction; compare literal model and row';material=row
   elif sid=='anatel-products':
    row=anatel[int(r['source_key'])-1];assert row['Modelo']==dev['model'];assert row['Nome do Fabricante']==raw['manufacturer'];ref['archive_capture']=capture(ROOT/'data/raw/anatel-products.zip');ref['field_reference']='Produtos_Homologados_Anatel.csv data record '+r['source_key']+', Modelo / Nome do Fabricante';ref['review_method']='read original saved ZIP CSV as UTF-8 BOM and semicolon-separated rows';material=row
   elif sid=='ntra-2026-01':
    textp=ROOT/'data/raw/ntra-2026-01-text.json';page=load(textp)['pages'][raw['page']-1];norm=re.sub(r'\s+','',page['text']);assert re.sub(r'\s+','',dev['model']) in norm;assert r['source_key'] in page['text'];ref['text_capture']=capture(textp);ref['field_reference']=f"PDF physical page {raw['page']}, row {r['source_key']}, Brand / Model";ref['review_method']='read saved embedded-text page and row; collapse extraction whitespace only for identity check';material='\n'.join(line for line in page['text'].splitlines() if r['source_key'] in line)
   elif sid.startswith('ul-'):
    b=load(p)
    if sid=='ul-celestica-cs':assert dev['model'] in b['models'];material={'Models':b['models'],'listing_holder':b['listing_holder']};ref['field_reference']='structured web-tool extract $.models and $.listing_holder'
    else:
     spans=raw['model_section_spans'];assert all(b['model_section_text'][a:e]==dev['model'] for a,e in spans);material={'matched_spans':spans,'listing_holder':b['listing_holder']};ref['field_reference']='$.model_section_text offsets '+str(spans)
    ref['review_method']='read immutable structured web-tool extract; original HTML unavailable; no completeness assumption'
   elif sid=='bahrain-tra':
    pp=ROOT/'data/raw/bahrain-equipment.json';row=next(x for x in load(pp) if x['authority_ref']==raw['authority_ref']);assert row['model_number']==dev['model'];ref['response_capture']=capture(pp);ref['field_reference']='API authority_ref '+raw['authority_ref'];ref['review_method']='read original saved JSON response row';material=row
   elif sid=='edgecore-7760-label':
    ref['field_reference']='left label Model No. / Supplier Sku / F/W / H/W / Rev.; adjacent label Model / Input Voltage Range';ref['review_method']='direct visual review via view_image, 2026-10-04; no serial/MAC transcription';material={'Model No.':'7760-32X-A-AC-F','Supplier Sku':'AS7760-32X','F/W':'V2015.02.00.30','H/W':'R011','Rev.':'R20','adjacent Model':'AS7760-32X','adjacent Input Voltage Range':'100-240~, 50 - 60Hz, 6A Max'}
   elif sid=='ecrunch-I610661':
    txt=p.read_text();txt=re.sub(r'<(script|style)\b.*?</\1>',' ',txt,flags=re.S);txt=re.sub(r'\s+',' ',html.unescape(re.sub('<[^>]+>',' ',txt)));assert dev['model'] in txt
    ref['field_reference']='product heading / SKU / Details paragraph';ref['review_method']='read saved seller HTML product Details paragraph; exclude Related Products';start=txt.index('Details I powered');end=txt.index('More Information',start);material=txt[start:end]
   else:raise AssertionError(sid)
   rec={'reference':ref,'imported_raw_fields':raw,'same_record_identifiers':ids,'company_roles':roles,'imported_facts':facts,'reviewed_saved_material':material};dd['records'].append(rec)
   if sid=='edgecore-7760-label':
    item['already_imported_statements'].append({'category':'firmware','statement':'F/W V2015.02.00.30; H/W R011; Rev. R20 on photographed unit only. No bootability, downloadable image, license or current installed state is established.','reference':ref})
    item['already_imported_statements'].append({'category':'identity_mapping','statement':'Model No. 7760-32X-A-AC-F and Supplier Sku AS7760-32X occur on the same label; adjacent compliance label repeats AS7760-32X. Mapping stays on this record.','reference':ref})
    item['newly_reviewed_unimported_statements'].append({'category':'power','proposed_fact_name':'label_observed_input_voltage_range','value':'100-240~, 50 - 60Hz, 6A Max','statement':'Adjacent AS7760-32X compliance label in the same photograph has this input marking. Photographed-unit label observation; not measured draw, a PSU inventory, or an assertion for every AS7760/full-code unit.','reference':ref})
   if sid=='ecrunch-I610661':
    for cat,name,value in [('power','seller_power_on_claim','I powered this on as shown in the pictures.'),('cooling','seller_component_handle_claim','Red handles on power supplies and fans.'),('software','seller_software_guarantee_statement','Switch hardware is guaranteed; however, as this switch is re-programmable, software is not guaranteed.')]:
     item['newly_reviewed_unimported_statements'].append({'category':cat,'proposed_fact_name':name,'value':value,'statement':'Historical seller claim in saved listing only; not independently verified, not current availability, no airflow inference from red handles, and no evidence about the owned or photographed eBay unit.','reference':ref})
   if sid.startswith('rra-') and raw.get('other_raw'):
    item['already_imported_statements'].append({'category':'power','statement':raw['other_raw'],'limits':'RRA other_raw associates this named switching power supply with this approval record. No input voltage, wattage, connector, installed count, airflow or exclusive PSU requirement is specified.','reference':ref})
  item['exact_catalog_matches'].append(dd)
 gaps={'power':'input voltage, PSU configuration/rating, connector and power-on dependencies','cooling':'airflow direction, fan configuration and operational cooling requirements','chassis':'rack form factor, mounting/rail requirements and any chassis/node dependency','firmware_boot_software':'boot loader, console/recovery procedure, exact supported NOS/image and installation requirements','licensing':'download access, license entitlement and transfer conditions'}
 for cat,detail in gaps.items():item['unknowns'].append({'category':cat,'statement':'No exact-model '+detail+' established by the reviewed collected evidence.','scope':'absence in this finite saved set; not an impossibility, unsupported-hardware or safety conclusion'})
 if query.startswith('CS8200'):item['unknowns'][0]['statement']='FSJ021 association is present; exact input voltage, PSU wattage/count, connector and operational dependencies remain absent.';item['excluded_near_models']=['CS8200-32X-DC-11','CS8201-32X-UN-00']
 elif query.startswith('CS8210'):item['excluded_near_models']=['CS8210-32X-DC-11','CS8211-32X-UN-00']
 elif query.startswith('DMC'):item['excluded_near_models']=['DMC-12-48R-AS0 48V','DMC-12-48C-AS0','DMC-12-48C-AS0 48V']
 elif query.startswith('7760'):item['unknowns'][0]['statement']='Photographed compliance label gives input range; installed PSU types/counts, connectors and actual owned-unit power dependencies remain absent.';item['unknowns'][3]['statement']='Photo firmware version and seller disclaimer are present; boot loader, supported/current NOS image, console/recovery and installation requirements remain absent.';item['excluded_near_models']=['AS7760-32X family records outside photographed-label record','AS7710-32X']
 elif query.startswith('AZ'):item['excluded_near_models']=['AZ4444-52X-AC-00','AZ4444-52X-DC-42','AZ4444-52X','AZ4444-52X-MAIN','AZ4444-52X-CPU']
 else:item['excluded_near_models']=['AS7772-32X-12V','AS7762-32X','AS7712-32X'];item['alias_limit']='RRA main/derived co-listing is not a software/hardware interchangeability alias.'
 results.append(item)
assert before==digest(ROOT/'data/devices.sqlite')
report={'review_date':'2026-10-04','scope':'six owned networking identifiers, finite existing captures only; no network/import/database writes','database_sha256_before_and_after':before,'method':'read-only SQLite provenance discovery, then original saved HTML/ZIP CSV/JSON or saved PDF text review; direct image review separately. Referenced captures SHA-256 checked against source metadata. Unknowns are collected-evidence gaps.','models':results,'newly_reviewed_unimported_count':sum(len(m['newly_reviewed_unimported_statements']) for m in results),'next_finite_leads':['For CS8200 investigate an exact UN-00 vendor hardware/installation document or FSJ021 document without lending DC-11 specs.','For CS8210 investigate an exact UN-00 hardware/installation document; current UL capture gives identity only and original UL HTML had a recorded 403.','For DMC-12-48R-AS0 investigate exact unsuffixed manual; 48V is separately printed and excluded.','For full 7760 identifier investigate official installation/software guidance explicitly identifying this full configuration; do not transfer AS7760 family claims through photo mapping.','For AZ4444-52X-DC-10 investigate exact full-code power/connector manual, without decoding DC-10.','For AS7772-32X investigate exact unsuffixed manual/support table, excluding -12V.']}
report['additional_reviewed_captures']=[]
for rel,scope in [('data/raw/ul-celestica-platforms.json','models section; exact CS8200/CS8210 identifiers, no practical facts'),('data/raw/icasa-2025-text.json','physical pages 153/224; exact DMC/AZ4444 identifiers and distinct 48V variant'),('data/raw/ntra-2022-12-text.json','physical pages 391/460; exact DMC/AS7772 identities only'),('data/raw/picos45/edgecore-accton.html','hardware table exact-identifier check; none of these six')]:
 report['additional_reviewed_captures'].append(dict(capture(ROOT/rel),review_scope=scope))
picos=Rows();picos.feed((ROOT/'data/raw/picos45/edgecore-accton.html').read_text())
assert not any(query.lower() in cell.lower() for row in picos.rows for cell in row for query in MODELS)
(OUT/'findings.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'models':len(results),'exact_records':sum(len(d['records']) for m in results for d in m['exact_catalog_matches']),'new_unimported':report['newly_reviewed_unimported_count'],'database_unchanged':True}))

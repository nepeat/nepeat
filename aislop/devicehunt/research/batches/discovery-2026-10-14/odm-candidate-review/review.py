"""Independent offline review of exact ODM candidate; writes only this directory."""
from pathlib import Path
from html.parser import HTMLParser
from collections import Counter,defaultdict
import argparse,contextlib,datetime,hashlib,importlib.util,io,json,re,shutil,sqlite3,sys,tempfile,unittest
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
AUTHOR=HERE.parent/'odm-review'
ORIGINAL=HERE.parent/'odm-source-expansion'
EXPECTED='f4bb69c73b809f5d35da8e4de58b6b933d3024136f4ed18228f473db1befcc71'
sys.path.insert(0,str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt.importers import prepare_curated,curated


def sha(path):
 with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def canon(obj):return json.dumps(obj,sort_keys=True,separators=(',',':'),ensure_ascii=False)
def norm(s):return ' '.join(s.split())
def load(path):return json.loads(Path(path).read_text())
def save(path,obj):
 with Path(path).open('x') as f:json.dump(obj,f,indent=2,ensure_ascii=False);f.write('\n')
def module(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m


class Literal(HTMLParser):
 """Small fragment reader independent of both row extraction implementations."""
 def __init__(self,raw):
  super().__init__(convert_charrefs=True);self.data=[];self.display=[];self.tags=[];self.feed(raw);self.close()
 def handle_starttag(self,t,a):
  self.tags.append((t,dict(a)))
  if t in {'br','p','li','div','dt','dd','h2','h3'}:self.display.append(' ')
 def handle_endtag(self,t):
  if t in {'br','p','li','div','dt','dd','h2','h3'}:self.display.append(' ')
 def handle_data(self,s):self.data.append(s);self.display.append(s)
 @property
 def raw_text(self):return ''.join(self.data)
 @property
 def text(self):return norm(''.join(self.display))


def proof(raw,item):
 p=item['proof'];fragment=raw[p['byte_start']:p['byte_end']]
 assert hashlib.sha256(fragment).hexdigest()==p['fragment_sha256']
 decoded=raw.decode('utf-8')
 assert decoded[p['char_start']:p['char_end']].encode('utf-8')==fragment
 assert decoded.count('\n',0,p['char_start'])+1==p['line']
 return fragment.decode('utf-8')


def cell(raw,item):
 parsed=Literal(proof(raw,item))
 assert parsed.raw_text==item['text_raw'] and parsed.text==item['text']
 assert any(t==item['tag'] for t,a in parsed.tags)
 assert bool(any(t=='sup' for t,a in parsed.tags))==item['has_superscript']
 return parsed


def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check-only',action='store_true');args=parser.parse_args()
 protected=[p for directory in [AUTHOR,ORIGINAL] for p in directory.rglob('*') if p.is_file()]
 before={str(p.relative_to(ROOT)):sha(p) for p in protected}
 database=ROOT/'data/devices.sqlite';manifest=ROOT/'data/import-manifest.json'
 live_before=sha(database);manifest_before=sha(manifest)
 assert sha(AUTHOR/'candidate.json')==EXPECTED
 candidate=load(AUTHOR/'candidate.json');accepted=load(AUTHOR/'accepted-rows.json');selection=load(AUTHOR/'selection.json')
 assert len(candidate['sources'])==5 and len(candidate['devices'])==len(accepted)==len(selection['references'])==214
 # Independently verify acquisition body/header hashes, including excluded AMD
 # and the zero-byte redirect. No content is recaptured.
 receipts=[json.loads(line) for line in (ORIGINAL/'requests.jsonl').read_text().splitlines()]
 hashes=[]
 for receipt in receipts:
  for name,hashkey in [('body_path','body_sha256'),('header_path','header_sha256')]:
   assert sha(ORIGINAL/receipt[name])==receipt[hashkey]
   hashes.append(dict(path=str((ORIGINAL/receipt[name]).relative_to(ROOT)),sha256=receipt[hashkey]))
 assert len(hashes)==20
 copied=[]
 for p in (AUTHOR/'raw').iterdir():
  assert p.read_bytes()==(ORIGINAL/p.name).read_bytes()
  copied.append(dict(path=p.name,sha256=sha(p)))
 assert len(copied)==10
 # Recompute original full extraction in memory, not through a CLI that writes
 # its source directory. This is a second implementation cross-check.
 original_parser=module('odm_original_extract',ORIGINAL/'extract_bulk.py')
 recomputed=original_parser.extract(ORIGINAL)
 assert recomputed==load(ORIGINAL/'bulk-extracted.json')
 original_rows=original_parser.discovery_rows(recomputed)
 assert original_rows==load(ORIGINAL/'discovery-rows.json') and len(original_rows)==1120
 bulk={x['source']['path']:x for x in recomputed['sources']}
 sources={x['id']:x for x in candidate['sources']}
 paths={x['snapshot'].removeprefix('raw/'):x for x in candidate['sources']}
 assert len(sources)==len(paths)==5
 source_dates=[]
 for selected in selection['sources']:
  source=paths[selected['body_path']];raw=(AUTHOR/'raw'/selected['body_path']).read_bytes()
  assert source['url']==selected['url'] and source['original_sha256']==selected['body_sha256']==hashlib.sha256(raw).hexdigest()
  assert source['captured_at_utc']==selected['finished_at_utc']
  stamp=datetime.datetime.fromisoformat(source['captured_at_utc']);assert stamp.utcoffset() is not None
  assert 'No aliases' in source['coverage'] and 'physical manufacture' in source['coverage']
  date=re.search(r'<meta name="docbuild:last-update" content="([^"]+)"',raw.decode('utf-8'))
  source_dates.append(dict(source=source['id'],captured_at_utc=source['captured_at_utc'],printed_last_update=date[1] if date else None,date_scope='capture timestamp is not publication/manufacture/current-support date'))
 # Build expected fact/role content independently from each source-bound accepted
 # row, checking every cell/field against exact original byte fragments.
 expected={};fragment_checks=0;nonidentity=[];blank_cells=0;missing_card_fields=[]
 for row,reference in zip(accepted,selection['references'],strict=True):
  assert row['source_path']==reference['source_path'] and row['locator']==reference['locator']
  assert (row['company'],row['model'],row['label'])==(reference['company'],reference['model'],reference['label'])
  discovery=original_rows[reference['discovery_row_index']]
  assert discovery['source_path']==row['source_path'] and discovery['locator']==row['locator']
  assert (discovery['partner'],discovery['model'])==(row['company'],row['model'])
  assert not discovery.get('has_spans') and not discovery.get('has_identity_superscripts')
  source=paths[row['source_path']];raw=(ORIGINAL/row['source_path']).read_bytes();loc=canon(row['locator'])
  facts=[['source_locator',loc],['source_scope',source['coverage']]]
  if 'row' in row:
   rawrow=proof(raw,row['row']);fragment_checks+=1
   rawtable=proof(raw,dict(proof=row['table_proof']));fragment_checks+=1
   tag=Literal(rawtable).tags[0]
   table_ordinal=len(re.findall(rb'<table\b',raw[:row['table_proof']['byte_start']],re.I))+1
   assert tag[0]=='table' and tag[1].get('id','ordinal-'+str(table_ordinal))==row['locator']['table_id']
   for c in row['row']['cells']:
    parsed=cell(raw,c);fragment_checks+=1
    assert not any('rowspan' in a or 'colspan' in a for t,a in parsed.tags if t in ('td','th'))
    blank_cells+=c['text']==''
   assert row['row']['cells'][0]['text']==row['company'] and row['row']['cells'][1]['text']==row['model']
   assert not any(c['has_superscript'] for c in row['row']['cells'][:2])
   if any(c['has_superscript'] for c in row['row']['cells'][2:]):nonidentity.append(dict(company=row['company'],model=row['model'],locator=row['locator'],cells=[c['text'] for c in row['row']['cells']]))
   table=next(t for t in bulk[row['source_path']]['tables'] if t['table_id']==row['locator']['table_id'])
   assert row['headers']==table['headers'][0] and row['heading']==table['nearest_heading']
   authorrow=next(r for r in table['rows'] if r['row_ordinal']==row['locator']['row_ordinal'])
   assert [(c['text'],c['text_raw']) for c in row['row']['cells']]==[(c['text'],c['text_raw']) for c in authorrow['cells']]
   role='program_partner' if row['label']=='Partner' else 'listing_holder'
   assert row['label'] in ('Partner','Manufacturer')
   key=row['locator']['table_id']+':row-'+str(row['locator']['row_ordinal'])
   kind='NVIDIA system program listing'
   facts += [['nvidia_table_row_raw_json',canon(row['row'])],['nvidia_table_headers_as_printed',canon(row['headers'])],['nvidia_table_heading_as_printed',row['heading']],['nvidia_company_column_label_as_printed',row['label']],['nvidia_row_fragment_sha256',row['row']['proof']['fragment_sha256']]]
   facts += [['nvidia_table_cell_as_printed',canon(dict(column=header,value=c['text']))+' ['+loc+']'] for header,c in zip(row['headers'],row['row']['cells'],strict=True)]
  else:
   card=row['card'];rawcard=proof(raw,card);fragment_checks+=1
   assert any(t=='div' and 'swiper-slide'in a.get('class','').split() for t,a in Literal(rawcard).tags)
   assert len(card['models'])==1;cell(raw,card['models'][0]);fragment_checks+=1
   assert card['models'][0]['text']==row['model'] and any(i.get('alt')==row['model'] for i in card['images'])
   for field in card['fields']:
    for fieldpart in ['label','value']:cell(raw,field[fieldpart]);fragment_checks+=1
   authorcard=bulk[row['source_path']]['cards'][row['locator']['card_ordinal']-1]
   assert authorcard['model']==row['model'] and authorcard['family_heading']==card['family']
   assert [(f['label']['text'],f['value']['text']) for f in card['fields']]==[(f['label']['text'],f['value']['text']) for f in authorcard['fields']]
   if len(card['fields'])==3:missing_card_fields.append(row['model'])
   role='documentation_vendor';key='card-'+str(row['locator']['card_ordinal']);kind='server product index configuration'
   facts += [['product_card_raw_json',canon(card)],['product_family_heading_as_printed',card['family']],['product_card_fragment_sha256',card['proof']['fragment_sha256']]]
   facts += [['product_card_field_as_printed',canon(dict(label=f['label']['text'],value=f['value']['text']))+' ['+loc+']'] for f in card['fields']]
  expected_record=dict(source=source['id'],key=key,brand=row['company'],model=row['model'],title=row['company']+' '+row['model'],kind=kind,affiliations=[[row['company'],role]],identifiers=[],hyperscaler=[],facts=facts)
  k=(source['id'],key);assert k not in expected;expected[k]=expected_record
 actual={(r['source'],r['key']):r for r in candidate['devices']}
 assert actual==expected and len(actual)==214
 assert len({(r['brand'],r['model']) for r in actual.values()})==154
 roles=Counter(role for r in actual.values() for company,role in r['affiliations']);assert roles==Counter(program_partner=137,listing_holder=39,documentation_vendor=38)
 assert sum(len(r['facts']) for r in actual.values())==2255 and blank_cells==27 and len(nonidentity)==2
 nvidia=(ORIGINAL/'nvidia-certified.html').read_text()
 assert '<sup>*</sup> Tested as a standalone system.' in nvidia
 ngc=(ORIGINAL/'redirect-nvidia-ngc-ready.pdf').read_text()
 assert 'only supported by NVIDIA AI Enterprise when they are equipped with NVIDIA V100 or T4 GPUs' in ngc
 assert all('conditional on NVIDIA V100 or T4' in s['coverage'] for s in candidate['sources'] if s['id'].startswith('nvidia-'))
 assert {r['model'] for r in actual.values()} >= {'Miramar - ICEMan','Miramar -ICEMan','X-wing-Flyboy (Tacoma)','B8056G68CE12HR-2T-TU-HE','B8261T85E8HR-2T-N-HE'}
 # Execute unchanged ten-test source using an owned copy of its data root. Its
 # hardcoded ROOT/tmp fixture must never write the author's directory.
 view=HERE/'test-view'
 if not args.check_only:
  view.mkdir();(view/'tmp').mkdir();shutil.copytree(AUTHOR/'raw',view/'raw')
  for name in ['candidate.json','accepted-rows.json','selection.json']:shutil.copyfile(AUTHOR/name,view/name)
 build_module=module('review_build',AUTHOR/'review_build.py');build_module.ROOT=view
 tests_module=module('odm_candidate_review_tests',AUTHOR/'test_review.py')
 log=io.StringIO()
 with contextlib.redirect_stdout(log):test_result=unittest.TextTestRunner(stream=log,verbosity=2).run(unittest.defaultTestLoader.loadTestsFromModule(tests_module))
 assert test_result.wasSuccessful() and test_result.testsRun==10,log.getvalue()
 # Independently prepare exact original candidate and import it twice into an
 # owned scratch catalog. No live or author data is opened writable.
 prepared=prepare_curated(AUTHOR/'candidate.json')
 assert len(prepared)==5 and sum(len(rows) for s,p,m,rows in prepared)==214
 assert all(m.sha256==s['original_sha256'] for s,p,m,rows in prepared)
 temporary_scratch=tempfile.TemporaryDirectory(prefix='scratch-check-',dir=HERE) if args.check_only else None
 scratch=(Path(temporary_scratch.name)/'scratch.sqlite') if temporary_scratch else HERE/'scratch.sqlite'
 assert not scratch.exists();catalog=Catalog(scratch)
 try:
  imports=[curated(catalog,AUTHOR/'candidate.json') for _ in range(2)]
  counts={t:catalog.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['sources','devices','records','facts','affiliations','identifiers','hyperscaler_evidence']}
  assert counts==dict(sources=5,devices=154,records=214,facts=2255,affiliations=214,identifiers=0,hyperscaler_evidence=0)
  assert imports==[214,214]
  stored={}
  for rawjson, in catalog.db.execute('SELECT raw_json FROM records'):
   record=json.loads(rawjson);stored[(record['source'],record['key'])]=record
  assert stored==actual
  integrity=catalog.db.execute('PRAGMA integrity_check').fetchone()[0];fk=catalog.db.execute('PRAGMA foreign_key_check').fetchall();assert integrity=='ok' and not fk
  scratchidentities=[tuple(r) for r in catalog.db.execute('SELECT id,brand,model FROM devices ORDER BY id')]
  fts=catalog.db.execute("SELECT count(*) FROM search_index WHERE search_index MATCH 'B8056G68CE12HR'").fetchone()[0];assert fts==4
 finally:
  catalog.db.close()
  if temporary_scratch:temporary_scratch.cleanup()
 with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True) as db:
  db.execute('PRAGMA query_only=ON');db.execute('BEGIN')
  live_ids={r[0]:(r[1],r[2]) for r in db.execute('SELECT id,brand,model FROM devices')}
 overlap=[dict(id=i,company=b,model=m,live_company=live_ids[i][0],live_model=live_ids[i][1]) for i,b,m in scratchidentities if i in live_ids]
 assert len(overlap)==9 and all((r['company'],r['model'])==(r['live_company'],r['live_model']) for r in overlap)
 assert before=={str(p.relative_to(ROOT)):sha(p) for p in protected}
 assert sha(database)==live_before and sha(manifest)==manifest_before
 report=dict(reviewer='/root/vendor_bulk_full',decision='accept_exact_candidate',candidate_sha256=EXPECTED,network_calls=0,shared_writes=False,original_acquisition_hashes_verified=hashes,copied_capture_hashes_verified=copied,original_extraction_rows_recomputed=1120,all_source_records_independently_verified=214,raw_fragment_checks=fragment_checks,facts_verified=2255,exact_company_model_pairs=154,extra_duplicate_references_preserved=60,roles=dict(roles),blank_table_cells_retained=blank_cells,nonidentity_footnote_rows=nonidentity,footnote_scope='Both selected * markers belong only to the GH200 module cell; source note says Tested as a standalone system. No compatibility fact is derived; marker and full snapshot remain. Historical AI Enterprise is conditional on V100/T4.',source_dates=source_dates,missing_card_fields_preserved=missing_card_fields,unchanged_author_tests_pass=10,prepare_curated_sources=5,scratch_import_and_replacement=imports,scratch_counts=counts,integrity=integrity,foreign_key_violations=0,fts_probe_hits=fts,exact_live_identity_reuses=overlap,unexpected_namespace_collisions=0,protected_input_files_unchanged=len(protected),live_catalog_sha256_unchanged=live_before,manifest_sha256_unchanged=manifest_before,limits='Empty scratch acceptance only; combined preservation, full-suite and clean rebuild belong to exclusive integration. AMD fragments remain unaccepted raw discovery. No manufacturer, alias, deployment, installed-configuration, current support or licence claim accepted.')
 if args.check_only:assert report==load(HERE/'checks.json')
 else:
  save(HERE/'checks.json',report)
  with (HERE/'tests.log').open('x') as f:f.write(log.getvalue())
  save(HERE/'row-audit.json',dict(candidate_sha256=EXPECTED,source_row_keys=[dict(source=s,key=k,company=r['brand'],model=r['model'],facts=len(r['facts']),role=r['affiliations'][0][1]) for (s,k),r in actual.items()]))
 print(json.dumps({k:v for k,v in report.items() if k not in ['original_acquisition_hashes_verified','copied_capture_hashes_verified','exact_live_identity_reuses']},indent=2))


if __name__=='__main__':main()

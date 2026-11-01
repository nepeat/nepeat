"""Build/check a finite local curated candidate; never import or use network."""
import argparse
import datetime
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import shutil
import sqlite3

HERE = Path(__file__).resolve().parent
RESEARCH = HERE.parent / 'oracle7420'
ROUTING = HERE.parent / 'oracle7420-routing' / 'routing.json'
PROJECT = HERE.parents[4]
LIMITS = ('Dated OS8.8.x manual statements only; documented configuration and service requirements, '
          'not installed auction-unit inventory, current support/entitlement or safe standalone wiring. '
          'Oracle is a documentation namespace and documentation_vendor only. No manufacturer/factory '
          'or deployment claim, alias, equivalence or attachment to existing ZFS STORAGE 7420/S7420 '
          'identities. Explicit 7420 M2 drive/cache paragraph stays separate; no general 7420 statement '
          'transfers to M2. Legend numbers are labels, not component counts.')
FILES = {'internal-components.html': 'oracle7420-internal-components-2026-10-12',
         'attached-storage.html': 'oracle7420-attached-storage-2026-10-12'}
CATEGORIES = {'component_legend':'component_legend_as_printed',
              'fru_service_restriction':'service_restriction_as_printed',
              'service_processor_power':'power_description',
              'power_distribution_topology':'power_description',
              'vertical_pdb_service_name':'physical_description',
              'power_supply_connection_positions':'power_description',
              'fan_power_boards':'cooling_description',
              'drive_backplane_service_name':'physical_description',
              'm2_system_drive_and_cache_rule':'physical_description',
              '7420_system_drive_and_cache_rule':'physical_description',
              'fan_fault_indicators':'cooling_description',
              'attached_storage_chain_and_shelf_limits':'physical_description'}

def sha(body): return hashlib.sha256(body).hexdigest()
def serialized(value): return (json.dumps(value, ensure_ascii=False, indent=2)+'\n').encode()

class Article(HTMLParser):
    def __init__(self):
        super().__init__(); self.article=0; self.p=0; self.parts=[]; self.paragraphs=[]; self.ids=[]
    def handle_starttag(self, tag, attrs):
        self.ids.extend(v for k,v in attrs if k=='id')
        if tag=='article': self.article+=1
        if tag=='p' and self.article: self.p+=1; self.parts=[]
    def handle_endtag(self, tag):
        if tag=='p' and self.article:
            self.paragraphs.append(' '.join(''.join(self.parts).split())); self.p-=1
        if tag=='article': self.article-=1
    def handle_data(self, value):
        if self.article and self.p: self.parts.append(value)

def normalized(value): return value.replace(' .','.').replace(' ,',',')

def build(check_only=False):
    findings=json.loads((RESEARCH/'findings.json').read_text())['findings']
    routing=json.loads(ROUTING.read_text())['source_claim_routes']
    ledger=json.loads((RESEARCH/'request-ledger.json').read_text())
    assert len(findings)==12 and len(routing)==12
    sources=[]; parsers={}; provenance=[]
    for filename,sid in FILES.items():
        entry=next(e for e in ledger if e.get('file')==filename)
        original=RESEARCH/filename; body=original.read_bytes()
        assert entry['status']==200 and sha(body)==entry['sha256'] and len(body)==entry['bytes']
        assert entry['url']==entry['final_url']
        parser=Article(); parser.feed(body.decode('utf-8')); parsers[entry['url']]=parser
        snapshot=HERE/'snapshots'/filename
        if snapshot.exists(): assert snapshot.read_bytes()==body
        elif check_only: raise ValueError('Missing staged snapshot')
        else:
            snapshot.parent.mkdir(exist_ok=True); shutil.copy2(original,snapshot)
            assert snapshot.read_bytes()==body
        sources.append({'id':sid,'url':entry['url'],'tier':'vendor',
                        'license':'Publicly readable factual documentation; upstream rights retained; no redistribution permission inferred.',
                        'snapshot':snapshot.relative_to(HERE).as_posix(),'coverage':LIMITS,
                        'original_sha256':sha(body),'captured_at':entry['completed_at'],
                        'final_url':entry['final_url'],'manual_as_printed':'Oracle ZFS Storage Appliance Customer Service Manual, Release OS8.8.x',
                        'document_identifier_as_printed':'F13771-18'})
        provenance.append({'source_id':sid,'original_path':original.relative_to(PROJECT).as_posix(),
                           'snapshot':snapshot.relative_to(HERE).as_posix(),'sha256':sha(body),
                           'bytes':len(body),'url':entry['url'],'final_url':entry['final_url'],
                           'captured_at':entry['completed_at']})
    rows={}
    for claim in findings:
        route=next(r for r in routing if r['claim_name']==claim['name'])
        model=route['proposed_literal_model']
        assert model==('Sun ZFS Storage 7420 M2' if claim['name']=='m2_system_drive_and_cache_rule' else 'Sun ZFS Storage 7420')
        assert route['source_url']==claim['source_url'] and route['source_sha256']==claim['source_sha256']
        assert route['selector']==claim['selector'] and route['source_text']==claim['source_text']
        source=next(s for s in sources if s['url']==claim['source_url'])
        assert source['original_sha256']==claim['source_sha256']
        parser=parsers[claim['source_url']]; selector=claim['selector']
        assert selector['section_id'] in parser.ids
        actual=[parser.paragraphs[n-1] for n in selector['article_p_ordinals']]
        assert list(map(normalized,actual))==list(map(normalized,claim['source_text']))
        key=(source['id'],model)
        if key not in rows:
            rows[key]={'source':source['id'],'key':model,'brand':'Oracle','model':model,
                       'title':model,'kind':'storage controller',
                       'affiliations':[['Oracle','documentation_vendor']],
                       'identifiers':[],'hyperscaler':[],'facts':[],'evidence':[],
                       'scope_limitations':LIMITS}
        value=claim['value']
        if claim['name']=='component_legend':
            value='Figure legend labels (not quantities): '+ '; '.join(f'{n}: {value[str(n)]}' for n in range(1,12))+'.'
        elif claim['name']=='attached_storage_chain_and_shelf_limits':
            value='Attached storage: '+value['chain_count_as_printed']+'; each chain contains '+value['per_chain_as_printed']+'. No shelf/cable model or wiring topology is stated here.'
        if claim['name'] in ('m2_system_drive_and_cache_rule','7420_system_drive_and_cache_rule'):
            value='Documented configuration/cluster rule in dated OS8.8.x manual, not installed-unit inventory: '+value
        citation=' [HTML #'+selector['section_id']+'; article paragraphs '+','.join(map(str,selector['article_p_ordinals']))+']'
        rows[key]['facts'].append([CATEGORIES[claim['name']],value+citation])
        rows[key]['evidence'].append({'observation_name':claim['name'],'source_sha256':claim['source_sha256'],
                                     'selector':selector,'source_text':claim['source_text']})
    devices=list(rows.values())
    assert len(devices)==3 and len(sources)==2 and sum(len(d['facts']) for d in devices)==12
    assert [(d['model'],len(d['facts'])) for d in devices]==[('Sun ZFS Storage 7420',10),('Sun ZFS Storage 7420 M2',1),('Sun ZFS Storage 7420',1)]
    assert len({(d['source'],d['key']) for d in devices})==3
    candidate={'sources':sources,'devices':devices,'candidate_status':'staged for independent review; no import',
               'scope_limitations':LIMITS}
    candidate_body=serialized(candidate)
    provenance_doc={'snapshots':provenance,'reviewed_findings_sha256':sha((RESEARCH/'findings.json').read_bytes()),
                    'routing_sha256':sha(ROUTING.read_bytes()),'candidate_sha256':sha(candidate_body)}
    if check_only:
        assert (HERE/'candidate.json').read_bytes()==candidate_body
        assert (HERE/'provenance.json').read_bytes()==serialized(provenance_doc)
        print('reproducibility and immutable source checks pass; candidate '+sha(candidate_body));return
    (HERE/'candidate.json').write_bytes(candidate_body)
    (HERE/'provenance.json').write_bytes(serialized(provenance_doc))
    baseline_at=datetime.datetime.now(datetime.timezone.utc).isoformat()
    db=PROJECT/'data/devices.sqlite'; before=sha(db.read_bytes())
    c=sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True)
    identity_hits={model:c.execute('SELECT id,brand,model FROM devices WHERE brand=? AND model=? COLLATE NOCASE',('Oracle',model)).fetchall()
                   for model in ['Sun ZFS Storage 7420','Sun ZFS Storage 7420 M2','ZFS STORAGE 7420','S7420']}
    c.close(); after=sha(db.read_bytes())
    checks={'passed':True,'candidate_sha256':sha(candidate_body),'source_count':2,'records':3,'distinct_models':2,
            'facts':12,'plain7420_facts':11,'m2_facts':1,'snapshot_bytes_equal':True,
            'selectors_checked_against_originals':12,'identifiers':0,'hyperscaler_claims':0,
            'only_role':'documentation_vendor','catalog_baseline_at_utc':baseline_at,
            'catalog_sha256_before_read':before,'catalog_sha256_after_read':after,
            'concurrent_baseline_changed':before!=after,'existing_identity_query':identity_hits,
            'baseline_note':'Read-only point-in-time observation; coordinator may promote another batch concurrently. Not integration/preservation acceptance.',
            'imports':0,'network_requests':0}
    (HERE/'checks.json').write_bytes(serialized(checks))
    print('staged 2 sources / 3 records / 2 exact models / 12 facts; '+sha(candidate_body))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');build(parser.parse_args().check)

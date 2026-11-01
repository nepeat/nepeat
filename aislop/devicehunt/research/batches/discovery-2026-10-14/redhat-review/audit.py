#!/usr/bin/env python3
"""Independent offline row/provenance/import audit; writes only this review directory."""
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import urllib.parse
import urllib.robotparser
ROOT=Path(__file__).resolve().parents[4]
OUT=Path(__file__).resolve().parent
SRC=OUT.parent/'redhat-vendor-expansion'
sys.path.insert(0,str(ROOT))
from devicehunt.catalog import Catalog,normalize,digest
from devicehunt.importers import prepare_curated,curated

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(name,obj):(OUT/name).write_text(json.dumps(obj,indent=2,ensure_ascii=False)+'\n')
def count(c):return {t:c.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','facts','identifiers','affiliations','hyperscaler_evidence']}

def main():
    live=ROOT/'data/devices.sqlite';live_hash_before=sha(live)
    input_files=sorted(p for p in SRC.iterdir() if p.is_file())
    input_hashes={p.name:sha(p) for p in input_files}
    candidate=json.loads((SRC/'candidate.json').read_text());plan=json.loads((SRC/'plan.json').read_text())
    basis=ROOT/plan['source_basis'];assert sha(basis)==plan['sha256']
    facet=json.loads(basis.read_text())['facet_counts']['facet_fields']['partnerName'];facet_counts=dict(zip(facet[::2],facet[1::2]))
    assert {p:facet_counts[p] for p in plan['partners']}==plan['historical_counts']
    receipts=[json.loads(x) for x in (SRC/'requests.jsonl').read_text().splitlines()]
    assert len(receipts)==plan['max_http']==10
    assert len({r['path'] for r in receipts})==len(receipts)
    rp=urllib.robotparser.RobotFileParser();rp.parse((SRC/'robots.txt').read_text().splitlines())
    ua='devicehunt-research/1.0 (public hardware catalog research; polite, rate-limited)'
    assert not rp.crawl_delay(ua)
    capture_summary=[];expected=[];excluded=[];company_differences=[];all_ids=[]
    sources={s['id']:s for s in candidate['sources']}
    assert len(sources)==len(candidate['sources'])==9
    by_path={r['path']:r for r in receipts}
    for r in receipts:
        p=SRC/r['path'];assert r['status']=='200' and r['exit_code']==0 and not r['error']
        assert p.stat().st_size==r['bytes'] and sha(p)==r['sha256']
        assert (SRC/(r['path']+'.headers')).read_text().splitlines()[0].endswith('200 OK')
        assert rp.can_fetch(ua,r['url'])
        capture_summary.append({'path':r['path'],'sha256':sha(p),'bytes':p.stat().st_size,'header_sha256':sha(SRC/(r['path']+'.headers'))})
    gaps=[(datetime.datetime.fromisoformat(b['utc'])-datetime.datetime.fromisoformat(a['utc'])).total_seconds() for a,b in zip(receipts,receipts[1:])]
    assert min(gaps)>=2
    for index,partner in enumerate(plan['partners'],1):
        name=f'partner-{index:02}.json';r=by_path[name];doc=json.loads((SRC/name).read_text());response=doc['response'];head=doc['responseHeader']
        assert head['status']==0 and head.get('partialResults',False) is False
        assert response['start']==0 and response['numFound']==len(response['docs'])<200
        params=urllib.parse.parse_qs(urllib.parse.urlparse(r['url']).query)
        assert params['fq']==['language:(en)','documentKind:CertifiedHardware','partnerName:'+json.dumps(partner)]
        assert params['rows']==['200'] and params['start']==['0']
        assert head['params']['isAuthenticated']=='false' and head['params']['isEntitled']=='false'
        assert set(params['fq'])<=set(head['params']['fq'])
        sid=f'redhat-certified-vendor-expansion-{index:02}-2026-10-14';source=sources[sid]
        assert source['snapshot']==name and source['original_sha256']==r['sha256'] and source['url']==r['url']
        assert source['tier']=='certification_body' and source['captured_at_utc']==r['utc']
        for pos,d in enumerate(response['docs']):
            assert d['partnerName']==partner and d['documentKind']=='CertifiedHardware';all_ids.append(str(d['id']))
            types=d['system_types'];assert isinstance(types,list) and all(isinstance(t,str) for t in types)
            if d['certified_category']!='System' or not any(t in {'Server','Edge System'} for t in types):
                excluded.append({'path':name,'row':pos,'id':str(d['id']),'types':types,'title':d['partnerProductName']});continue
            model=d['model'];title=d['partnerProductName'];key=str(d['id']);url=d['view_uri']
            assert isinstance(model,str) and model.strip()
            assert url=='https://catalog.redhat.com/hardware/system/detail/'+key
            facts=[['redhat_cert_catalog_id',key],['redhat_cert_partner_name_as_printed',partner],['redhat_cert_model_field_as_printed',model],['redhat_cert_product_name_as_printed',title],['redhat_cert_listing_category_as_printed','System'],['redhat_cert_listing_row',str(pos)],['redhat_cert_listing_capture',name+' sha256 '+r['sha256']]]
            for field,suffix in [('make','make_field_as_printed'),('system_types','system_types_listing'),('certification_level','certification_level_listing'),('certified_versions','certified_versions_listing'),('certified_architectures','architectures_listing'),('createdDate','listing_created'),('lastModifiedDate','listing_last_modified')]:
                assert field in d
                value=d[field];facts.append(['redhat_cert_'+suffix,json.dumps(value,ensure_ascii=False) if isinstance(value,(list,dict)) else str(value)])
            expected.append({'source':sid,'key':key,'brand':partner,'model':model,'title':title,'kind':'server' if 'Server' in types else 'edge system','url':url,'affiliations':[[partner,'listing_holder']],'identifiers':[['redhat_catalog_id',key],['redhat_certified_product',title]],'facts':facts,'hyperscaler':[]})
            if d['make']!=partner:company_differences.append({'id':key,'holder':partner,'make':d['make'],'model':model,'title':title})
    assert len(set(all_ids))==len(all_ids)==245
    assert candidate['devices']==expected
    assert len(expected)==242 and len(excluded)==3
    assert all(x['types']==['Desktop/Workstation'] for x in excluded)
    assert sum(len(x['facts']) for x in expected)==3388
    ids=[digest(json.dumps([normalize(d['brand']),d['model'].casefold().strip(),'']).encode())[:20] for d in expected]
    assert len(set(ids))==242
    prepared=prepare_curated(SRC/'candidate.json');assert len(prepared)==9
    assert sum(len(rows) for _,_,_,rows in prepared)==242
    for source,snapshot,metadata,rows in prepared:assert metadata.sha256==source['original_sha256']
    with sqlite3.connect(live.as_uri()+'?mode=ro',uri=True) as c:
        c.execute('BEGIN');base_counts=count(c)
        exact_id_hits=c.execute('SELECT id FROM devices WHERE id IN ('+','.join('?'*len(ids))+')',ids).fetchall();assert not exact_id_hits
        source_hits=c.execute('SELECT id FROM sources WHERE id IN ('+','.join('?'*len(sources))+')',list(sources)).fetchall();assert not source_hits
        cert_ids=c.execute("SELECT value FROM identifiers WHERE kind='redhat_catalog_id'").fetchall();assert not set(all_ids).intersection(x[0] for x in cert_ids)
        # SQLite backup reads one consistent transaction into the review-owned path.
        scratch=OUT/'candidate-review.sqlite'
        if scratch.exists():raise FileExistsError('preserve prior review scratch')
        target=sqlite3.connect(scratch);c.backup(target);target.close()
    cat=Catalog(scratch)
    try:
        result=curated(cat,SRC/'candidate.json');cat.db.commit();assert result==242
        after=count(cat.db)
        assert after=={k:v+{'devices':242,'records':242,'sources':9,'facts':3388,'identifiers':484,'affiliations':242,'hyperscaler_evidence':0}[k] for k,v in base_counts.items()}
        assert cat.db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
        assert not cat.db.execute('PRAGMA foreign_key_check').fetchall()
        role_rows=cat.db.execute('SELECT DISTINCT a.role,a.target FROM affiliations a JOIN records r ON a.record_id=r.id WHERE r.source_id LIKE ?',('redhat-certified-vendor-expansion-%',)).fetchall()
        assert [tuple(r) for r in role_rows]==[('listing_holder',None)]
        cat.db.execute('ATTACH DATABASE ? AS old',(str(live),))
        preservation={}
        for (t,) in cat.db.execute("SELECT name FROM old.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '%search%' ORDER BY name").fetchall():
            n=cat.db.execute('SELECT count(*) FROM (SELECT * FROM old."'+t+'" EXCEPT SELECT * FROM main."'+t+'")').fetchone()[0]
            preservation[t]=n;assert n==0
        first_counts=count(cat.db);assert curated(cat,SRC/'candidate.json')==242;cat.db.commit();assert count(cat.db)==first_counts
        assert cat.db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
    finally:cat.db.close()
    env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1',TMPDIR=str(OUT/'tmp'),PYTHONPATH=str(ROOT))
    runs=[]
    for argv in [[sys.executable,'-B',str(SRC/'test_parse.py')],[sys.executable,'-B','-m','unittest','discover','-s','tests','-p','test_curated_import.py','-v']]:
        result=subprocess.run(argv,cwd=ROOT,env=env,capture_output=True,text=True)
        runs.append({'argv':argv,'returncode':result.returncode,'stdout':result.stdout,'stderr':result.stderr});assert result.returncode==0
    assert sha(live)==live_hash_before
    assert input_hashes=={p.name:sha(p) for p in input_files}
    report={'decision':'accept bound candidate for existing curated importer','candidate_sha256':sha(SRC/'candidate.json'),'input_hashes':input_hashes,
            'capture_inputs':capture_summary,'minimum_finished_receipt_gap_seconds':min(gaps),
            'raw_rows':245,'accepted_records':242,'excluded':excluded,'facts':3388,'sources':9,
            'make_different_from_holder_count':len(company_differences),'make_different_from_holder_examples':company_differences,
            'live_database_sha256_before':live_hash_before,'live_database_sha256_after':sha(live),
            'scratch_import_before':base_counts,'scratch_import_after':after,'old_rows_missing_by_table':preservation,
            'roles_in_new_records':[list(r) for r in role_rows],'idempotent_reimport_counts_unchanged':True,
            'integrity':'ok','foreign_key_violations':0,'tests':runs,'network_requests':0}
    save('audit-results.json',report);print(json.dumps({k:v for k,v in report.items() if k not in {'input_hashes','capture_inputs','make_different_from_holder_examples','tests','old_rows_missing_by_table'}},indent=2))
if __name__=='__main__':main()

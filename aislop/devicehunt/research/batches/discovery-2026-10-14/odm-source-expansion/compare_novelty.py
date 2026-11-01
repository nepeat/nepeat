#!/usr/bin/env python3
"""Compare literal discovery strings with a pinned read-only catalog snapshot."""
from __future__ import annotations
import argparse
from collections import Counter,defaultdict
from datetime import datetime,timezone
import hashlib
import json
from pathlib import Path
import sqlite3
from extract_bulk import dump,sha

def file_sha(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1048576),b''):h.update(b)
    return h.hexdigest()

def compare(rows,devices,identifiers):
    by_model=defaultdict(list);by_identifier=defaultdict(list)
    for d in devices:by_model[d['model']].append(d)
    for i in identifiers:by_identifier[i['value']].append(i)
    out=[]
    for i,r in enumerate(rows):
        model_hits=by_model.get(r['model'],[])
        pair=[d for d in model_hits if d['brand']==r['partner']] if r['partner'] else []
        identifier_hits=by_identifier.get(r['model'],[])
        if pair:status='exact_printed_partner_and_model'
        elif model_hits:status='same_literal_model_other_or_unknown_partner'
        elif identifier_hits:status='literal_identifier_hit_only'
        else:status='no_literal_model_or_identifier_hit'
        out.append({'discovery_row_index':i,'dataset':r['dataset'],'source_url':r['source_url'],
                    'locator':r['locator'],'partner':r['partner'],'model':r['model'],
                    'identity_scope':r['identity_scope'],'status':status,
                    'exact_partner_model_hits':pair,'literal_model_hits':model_hits,
                    'literal_identifier_hits':identifier_hits})
    return out

def proposal(rows,novelty):
    grouped={};held=[]
    for i,r in enumerate(rows):
        if not r['target_partner_exact']:continue
        reason=None
        if not r['model'] or not r['partner']:reason='missing literal identity cells'
        elif r.get('has_spans'):reason='table has rowspan/colspan; requires explicit resolution'
        elif r.get('has_identity_superscripts'):reason='identity cell has superscript; do not turn footnote into identifier'
        if reason:held.append({'discovery_row_index':i,'reason':reason});continue
        key=(r['partner'],r['model'])
        if key not in grouped:
            grouped[key]={'printed_company':key[0],'printed_model':key[1],
                'identity_rule':'whole printed cell or heading; no alias splitting or corporate mapping',
                'review_status':'proposal requiring independent source review; not importer input',
                'live_exact_pair_exists':bool(novelty[i]['exact_partner_model_hits']),
                'references':[]}
        grouped[key]['references'].append({'discovery_row_index':i,'source_url':r['source_url'],
              'source_path':r['source_path'],'source_sha256':r['source_sha256'],'locator':r['locator'],
              'role_label_as_printed':r['partner_role_as_printed'],
              'source_scoped_claim':'publisher lists this complete model with this company in this row/card'})
    return {'schema_version':1,'status':'private proposal, unimported and independently unreviewed',
            'scope':'exact selected-partner NVIDIA rows and exact MiTAC product card headings only',
            'excluded_claims':['physical manufacture','deployment','alias relationships','auction-unit fitment','current qualification/support guarantee'],
            'amd_exclusion':'all PDF fragments stay raw; no merged-partner or multiline-identity inference',
            'candidates':list(grouped.values()),'held_rows':held}

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--base',type=Path,default=Path(__file__).resolve().parent)
    ap.add_argument('--database',type=Path,required=True);args=ap.parse_args()
    path=args.database.resolve(strict=True);before=file_sha(path)
    wal=Path(str(path)+'-wal')
    if wal.exists() and wal.stat().st_size:raise ValueError('nonempty WAL: need coordinator-pinned snapshot')
    con=sqlite3.connect(path.as_uri()+'?mode=ro',uri=True)
    try:
        con.execute('PRAGMA query_only=ON');con.execute('BEGIN')
        devices=[dict(zip(('id','brand','model','revision'),row)) for row in con.execute('SELECT id,brand,model,revision FROM devices ORDER BY id')]
        identifiers=[dict(zip(('device_id','kind','value','brand','model','revision'),row)) for row in con.execute('SELECT DISTINCT d.id,i.kind,i.value,d.brand,d.model,d.revision FROM identifiers i JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id ORDER BY d.id,i.kind,i.value')]
        sources=[dict(zip(('id','url','sha256'),row)) for row in con.execute('SELECT id,url,sha256 FROM sources ORDER BY id')]
        schema=con.execute('PRAGMA user_version').fetchone()[0]
    finally:con.close()
    after=file_sha(path)
    if before!=after:raise ValueError('catalog changed during snapshot; retry only after coordination')
    rows=json.loads((args.base/'discovery-rows.json').read_text());detail=compare(rows,devices,identifiers)
    urls=sorted({r['source_url'] for r in rows})
    report={'captured_at_utc':datetime.now(timezone.utc).isoformat(),'database':str(path),'database_sha256_before':before,
            'database_sha256_after':after,'stable_read_only_transaction':True,'sqlite_user_version':schema,
            'device_count':len(devices),'source_count':len(sources),
            'snapshot_rows_sha256':sha(json.dumps([devices,identifiers,sources],ensure_ascii=False,sort_keys=True).encode()),
            'discovery_rows_sha256':file_sha(args.base/'discovery-rows.json'),
            'comparison_rules':'case-sensitive literal equality, no brand aliases, no model normalization; identifier hits never merge identities',
            'counts':dict(Counter(x['status'] for x in detail)),
            'by_dataset':{ds:dict(Counter(x['status'] for x in detail if x['dataset']==ds)) for ds in sorted({r['dataset'] for r in rows})},
            'exact_source_url_hits':{u:[s for s in sources if s['url']==u] for u in urls},
            'unique_printed_partner_model_pairs':len({(r['partner'],r['model']) for r in rows if r['partner']}),
            'rows':detail}
    dump(args.base/'catalog-novelty.json',report)
    dump(args.base/'curated-proposal.json',proposal(rows,detail))
    print(json.dumps({k:v for k,v in report.items() if k not in {'rows','exact_source_url_hits','by_dataset'}},indent=2))
if __name__=='__main__':main()

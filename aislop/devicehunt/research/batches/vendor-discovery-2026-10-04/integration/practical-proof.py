"""Read-only exact-record, role-boundary and FTS acceptance proof."""
from collections import Counter
import json
from pathlib import Path
import re
import subprocess
import sys
from audit import connect, rows, sha
from devicehunt import documents
HERE=Path(__file__).resolve().parent
DB=Path('data/vendor-discovery-candidate-2026-10-04.sqlite')
INPUT=Path('data/vendor-discovery-facts-2026-10-04.json')
proposal=json.loads(INPUT.read_text()); before=sha(DB); proof={'reports':[]}
for d in proposal['devices']:
    args=[sys.executable,'-m','devicehunt','--db',str(DB),'buying-report',d['model'],'--format','json']
    report=json.loads(subprocess.run(args,check=True,capture_output=True,text=True).stdout)
    matches=[r for r in report['matched_records'] if r['source_id']==d['source']]
    assert len(matches)==1
    r=matches[0]
    assert Counter((f['name'],f['value']) for f in r['context_facts'])==Counter(tuple(f) for f in d['facts'])
    assert {(a['company'],a['role']) for a in r['company_roles']}=={('QCT','brand'),('QCT','platform_vendor')}
    filtered=json.loads(subprocess.run(args+['--vendor','Quanta'],check=True,capture_output=True,text=True).stdout)
    assert not any(r['source_id'] in {s['id'] for s in proposal['sources']} for r in filtered['matched_records'])
    assert not any(s['source_id']==d['source'] for category in filtered['categories'].values() for s in category['statements'])
    proof['reports'].append({'model':d['model'],'unfiltered':report,'quanta_filtered':filtered,'all_accepted_context_facts_preserved':len(d['facts'])})
db=connect(DB)
try:
    expected=[]
    for d in db.execute('SELECT * FROM devices ORDER BY id'):
        parts=[d[k] for k in ('title','brand','model','kind','revision')]
        for table,expression in [('affiliations',"company || ' ' || role"),('identifiers','value'),('facts','value'),('hyperscaler_evidence',"operator || ' ' || relationship || ' ' || detail"),('fcc_exhibits',"title || ' ' || document_type || ' ' || availability")]:
            parts.extend(r[0] for r in db.execute('SELECT '+expression+' FROM '+table+' x JOIN records r ON r.id=x.record_id WHERE r.device_id=? ORDER BY r.id,1',(d['id'],)))
        text=' '.join(parts); text+=' '+' '.join(re.findall(r'\d+[x×](\d+(?:\.\d+)?[kmg](?:be?)?)',text,re.I))
        expected.append((d['id'],text))
    assert Counter(expected)==rows(db,'search_index',exact=True)
    proof['device_fts_complete_content_reconstruction']=len(expected)
    hits=documents.search(db,'D51B',source='reference-pdf-qct-d51b-datasheet-mirror-2017',limit=20)
    proof['d51b_document_search']=hits
    assert hits
    proof['d51b_embedded_pages']=[dict(r) for r in db.execute("SELECT p.* FROM document_pages p JOIN documents d ON d.document_key=p.document_key WHERE d.source_id='reference-pdf-qct-d51b-datasheet-mirror-2017' ORDER BY page_number")]
    assert len(proof['d51b_embedded_pages'])==2
finally:
    db.close()
assert sha(DB)==before
proof['database_sha256']=before;proof['readonly_hash_unchanged']=True
out=HERE/'candidate-practical-proof.json';assert not out.exists();out.write_text(json.dumps(proof,indent=2,ensure_ascii=False)+'\n')
print('all38 context facts, Quanta role boundary, device FTS content and D51B page search verified')

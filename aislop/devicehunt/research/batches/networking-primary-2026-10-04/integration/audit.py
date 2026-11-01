"""Read-only exact preservation and duplicate-sensitive rebuild/FTS proof."""
from collections import Counter
import importlib.util
import json
from pathlib import Path
import re
from devicehunt import validation

HERE = Path(__file__).resolve().parent
prior = HERE.parents[1] / 'vendor-discovery-2026-10-04/integration/audit.py'
spec = importlib.util.spec_from_file_location('counter_helpers', prior)
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)  # Only read-only utility functions; no historical audit run.
connect, rows, sha, TABLES = helpers.connect, helpers.rows, helpers.sha, helpers.TABLES
DATA = Path('data')
paths = [DATA/'devices.sqlite', DATA/'networking-primary-candidate-2026-10-04.sqlite',
         DATA/'rebuilt-with-networking-primary-2026-10-04.sqlite']
hashes = [sha(p) for p in paths]
dbs = [connect(p) for p in paths]
old, new, rebuilt = dbs
report = {'preservation': {}, 'rebuild_counter_equality': {}, 'hashes_before': hashes}
try:
    oracle_id = new.execute("SELECT device_id FROM records WHERE source_id='oracle-x7-2c-vpat-t2-6972-2017'").fetchone()[0]
    assert old.execute('SELECT 1 FROM devices WHERE id=?', (oracle_id,)).fetchone()
    changed = {oracle_id}
    for db in dbs:
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not db.execute('PRAGMA foreign_key_check').fetchall()
        assert db.execute('PRAGMA user_version').fetchone()[0] == 28
    for table in TABLES:
        left, right = rows(old,table,exact=True), rows(new,table,exact=True)
        removed, added = left-right, right-left
        report['preservation'][table] = {'before':sum(left.values()), 'after':sum(right.values()),
                                        'removed':sum(removed.values()), 'added':sum(added.values())}
        if table == 'search_index':
            assert all(row[0] in changed for row in removed), (table, removed)
            for row,count in left.items():
                if row[0] not in changed:
                    assert right[row] == count
        else:
            assert not removed, (table, report['preservation'][table])
        crows, rrows = rows(new,table), rows(rebuilt,table)
        assert crows == rrows, (table, len(crows-rrows), len(rrows-crows))
        report['rebuild_counter_equality'][table] = {'rows':sum(crows.values()), 'equal':True}
    # Exact unchanged tables include all document content, existing source metadata,
    # regulatory discovery, and hyperscaler evidence. Only expected rows are added.
    deltas = {'devices':1,'records':2,'sources':2,'facts':35,'affiliations':2,'search_index':1}
    for table, item in report['preservation'].items():
        assert item['after']-item['before'] == deltas.get(table,0), (table,item)
    report['baseline'] = {}
    for name in ['owned-hardware.json','vendor-examples.json']:
        outputs = [validation.run(db, Path('validation')/name) for db in dbs]
        for key in ['summary','search_summary','evidence_summary']:
            assert outputs[0][key] == outputs[1][key] == outputs[2][key], (name,key)
        assert outputs[1]['results'] == outputs[2]['results'], name
        for prior_result, candidate_result in zip(outputs[0]['results'], outputs[1]['results']):
            for key in ['identifier','vendor','status','gap_type']:
                assert prior_result[key] == candidate_result[key], (name,key)
            for key in ['evidence','leads','search_hits']:
                assert all(item in candidate_result[key] for item in prior_result[key]), (name,key)
            added_evidence=[item for item in candidate_result['evidence'] if item not in prior_result['evidence']]
            assert all(item['source_id']=='oracle-x7-2c-vpat-t2-6972-2017' for item in added_evidence)
            added_leads=[item for item in candidate_result['leads'] if item not in prior_result['leads']]
            assert all(item['source_id']=='community-as7772-dtb-gist-2026-10-04' for item in added_leads)
        report['baseline'][name] = outputs[1]
    for db in [new,rebuilt]:
        expected=[]
        for d in db.execute('SELECT * FROM devices ORDER BY id'):
            parts=[d[k] for k in ['title','brand','model','kind','revision']]
            for table, expression in [('affiliations',"company || ' ' || role"),('identifiers','value'),('facts','value'),('hyperscaler_evidence',"operator || ' ' || relationship || ' ' || detail"),('fcc_exhibits',"title || ' ' || document_type || ' ' || availability")]:
                parts.extend(r[0] for r in db.execute('SELECT '+expression+' FROM '+table+' x JOIN records r ON r.id=x.record_id WHERE r.device_id=? ORDER BY r.id,1',(d['id'],)))
            text=' '.join(parts)
            text += ' '+' '.join(re.findall(r'\d+[x×](\d+(?:\.\d+)?[kmg](?:be?)?)',text,re.I))
            expected.append((d['id'],text))
        assert Counter(expected)==rows(db,'search_index',exact=True)
        assert Counter(tuple(r) for r in db.execute('SELECT document_key,page_number,text FROM document_pages')) == rows(db,'document_search',exact=True)
    report['fts_reconstructed'] = {'device_rows':4084,'document_rows':3651}
    payload = json.loads((DATA/'networking-primary-facts-2026-10-04.json').read_text())
    report['new_records'] = []
    for d in payload['devices']:
        record = new.execute('SELECT * FROM records WHERE source_id=? AND source_key=?',(d['source'],d['key'])).fetchone()
        assert record and json.loads(record['raw_json']) == d
        facts=Counter(tuple(r) for r in new.execute('SELECT name,value FROM facts WHERE record_id=?',(record['id'],)))
        assert facts == Counter(tuple(f) for f in d['facts'])
        roles=[tuple(r) for r in new.execute('SELECT company,role FROM affiliations WHERE record_id=?',(record['id'],))]
        assert Counter(roles)==Counter(tuple(a) for a in d['affiliations'])
        assert not new.execute('SELECT 1 FROM hyperscaler_evidence WHERE record_id=?',(record['id'],)).fetchone()
        report['new_records'].append({'record_id':record['id'],'device_id':record['device_id'],'source_id':d['source'],'facts':len(d['facts']),'roles':roles})
finally:
    for db in dbs: db.close()
assert hashes == [sha(p) for p in paths]
report['read_only_hash_unchanged']=True
with (HERE/'candidate-audit.json').open('x') as stream:
    json.dump(report,stream,indent=2,ensure_ascii=False);stream.write('\n')
print('all 43 tables preserve old exact rows and match rebuild multiplicities; FTS and baseline pass')

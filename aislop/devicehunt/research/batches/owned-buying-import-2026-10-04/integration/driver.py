"""Small reusable integration primitives; this phase runs read-only preflight only."""
import hashlib
import json
import sqlite3
import argparse
import os
import subprocess
import sys
import re
from contextlib import contextmanager
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
TAG = 'owned-buying-import-2026-10-04'
LIVE = ROOT / 'data/devices.sqlite'
BEFORE = 'ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8'
BATCH = ROOT / 'research/batches' / TAG
HERE = BATCH / 'integration'
DATA = ROOT / 'data'
CAND = DATA / 'owned-buying-import-candidate-2026-10-04.sqlite'
REBUILD = DATA / 'rebuilt-with-owned-buying-import-2026-10-04.sqlite'
LABEL = DATA / 'label-evidence-with-input-marking-2026-10-04.json'
BUNDLE = DATA / 'raw/broadcom-servers-bundle.json'


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1048576), b''):
            digest.update(chunk)
    return digest.hexdigest()


@contextmanager
def read_only(path):
    connection = sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)
    connection.row_factory = sqlite3.Row
    try:
        yield connection
    finally:
        connection.close()


def rows(connection, table, excluded=()):
    # Counter preserves duplicate multiplicity, unlike EXCEPT alone.
    columns = [row['name'] for row in connection.execute('PRAGMA table_info("' + table + '")')
               if row['name'] not in excluded]
    fields = ','.join('"' + column + '"' for column in columns)
    return Counter(tuple(row) for row in connection.execute('SELECT ' + fields + ' FROM "' + table + '"'))


def freeze(checkpoint):
    for group in ('accepted_inputs_sha256', 'code_sha256', 'test_logs_sha256'):
        assert checkpoint[group], group
        for path, expected in checkpoint[group].items():
            assert sha(ROOT / path) == expected, path
    assert checkpoint['coordinator_accepted'] is True


def buying_snapshot(database, output):
    """Explicit candidate selection; never reuse the old live-only generator."""
    from devicehunt.buying import run
    before = sha(database)
    inventory = ROOT / 'validation/owned-hardware.json'
    with read_only(database) as connection:
        reports = [{'query': item['identifier'], 'expected_vendor': item['vendor'],
                    'report': run(connection, item['identifier'])}
                   for item in json.loads(inventory.read_text())['devices']]
    assert len(reports) == 10 and sha(database) == before
    payload = dict(database=str(Path(database).relative_to(ROOT)), database_sha256=before,
                   inventory_sha256=sha(inventory), read_only_verified=True, reports=reports)
    with Path(output).open('x') as stream:
        json.dump(payload, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def preflight():
    assert sha(LIVE) == BEFORE
    manifest = json.loads((ROOT / 'data/import-manifest.json').read_text())
    assert (len(manifest['imports']), len(manifest['files'])) == (116, 2171)
    with read_only(LIVE) as connection:
        assert connection.execute('PRAGMA user_version').fetchone()[0] == 28
        counts = {table: connection.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                  for table in ('devices', 'records', 'sources', 'facts', 'documents', 'document_pages')}
    assert counts == dict(devices=4083, records=5298, sources=278, facts=44664,
                          documents=55, document_pages=3649), counts
    assert sha(LIVE) == BEFORE
    print(json.dumps(dict(database_sha256=BEFORE, counts=counts,
                         manifest_jobs=116, hashed_inputs=2171, phase='read-only preparation')))


def save(path, value):
    with Path(path).open('x') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def copy_new(source, destination):
    assert not destination.exists(), destination
    subprocess.run(['/bin/cp', '-cp', str(source), str(destination)], check=True)
    assert sha(source) == sha(destination)


def acceptance(phase):
    checkpoint = json.loads((BATCH / 'acceptance.json').read_text())
    freeze(checkpoint)
    assert checkpoint['authorized_phase'] == phase
    for path, expected in checkpoint['review_sha256'].items():
        assert sha(ROOT / path) == expected, path
    return checkpoint


def checks(database):
    from devicehunt import validation, vendor_inventory
    with read_only(database) as connection, read_only(LIVE) as previous:
        assert connection.execute('PRAGMA user_version').fetchone()[0] == 28
        assert connection.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not connection.execute('PRAGMA foreign_key_check').fetchall()
        assert connection.execute('SELECT count(*) FROM facts').fetchone()[0] == 44690
        for table, count in [('devices',4083),('records',5298),('sources',278),
                             ('documents',55),('document_pages',3649),('hyperscaler_evidence',22)]:
            assert connection.execute('SELECT count(*) FROM '+table).fetchone()[0] == count
        baseline = validation.run(connection, ROOT/'validation/owned-hardware.json')
        examples = validation.run(connection, ROOT/'validation/vendor-examples.json')
        assert baseline['summary'] == dict(total=10,hits=10,gaps=0)
        assert baseline['evidence_summary'] == dict(document_snapshot_hits=9,label_photo_only_hits=1,unresolved_identity_gaps=0)
        assert examples['summary'] == dict(total=2,hits=0,gaps=2)
        for scope in ('manufacturer','factory'):
            a = vendor_inventory.run(connection, scope=scope, limit=-1)
            b = vendor_inventory.run(previous, scope=scope, limit=-1)
            for key in ('identity_target_pairs','unique_identities','counts_by_target'):
                assert a[key] == b[key], (scope,key)
        assert connection.execute('SELECT count(*) FROM search_index').fetchone()[0] == 4083
        assert connection.execute('SELECT count(*) FROM document_search').fetchone()[0] == 3649
        expected_fts = []
        for device in connection.execute('SELECT * FROM devices ORDER BY id'):
            parts = [device[k] for k in ('title','brand','model','kind','revision')]
            for table,expression in [('affiliations',"company || ' ' || role"),('identifiers','value'),('facts','value'),('hyperscaler_evidence',"operator || ' ' || relationship || ' ' || detail"),('fcc_exhibits',"title || ' ' || document_type || ' ' || availability")]:
                parts.extend(r[0] for r in connection.execute('SELECT '+expression+' FROM '+table+' x JOIN records r ON r.id=x.record_id WHERE r.device_id=? ORDER BY r.id,1',(device['id'],)))
            text = ' '.join(parts)
            text += ' ' + ' '.join(re.findall(r'\d+[x×](\d+(?:\.\d+)?[kmg](?:be?)?)',text,re.I))
            expected_fts.append((device['id'],text))
        assert Counter(expected_fts) == rows(connection,'search_index')
        assert rows(connection,'document_search') == rows(previous,'document_search')
    return dict(owned=baseline['summary'],owned_evidence=baseline['evidence_summary'],bytedance=examples['summary'])


def preservation(checkpoint):
    from devicehunt.broadcom import bios_certification_rows
    audit = {}
    with read_only(LIVE) as old, read_only(CAND) as new:
        tables = [r[0] for r in old.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]
        for table in tables:
            if table in ('sources','records') or table.startswith(('search_index','document_search')):
                continue
            excluded = ()
            left, right = rows(old,table,excluded), rows(new,table,excluded)
            assert not left-right, (table,'old rows lost')
            if table != 'facts':
                assert left == right, (table,'unexpected change')
            audit[table] = dict(previous=sum(left.values()),current=sum(right.values()),missing=0)
        label_row = json.loads(LABEL.read_text())['devices'][0]
        literal = []
        bios_total = 0
        for row in old.execute('SELECT * FROM records ORDER BY id'):
            after = new.execute('SELECT * FROM records WHERE id=?',(row['id'],)).fetchone()
            assert after is not None
            for key in row.keys():
                if key == 'observed_at' and row['source_id'] in ('broadcom-servers','edgecore-7760-label'):
                    continue
                if key == 'raw_json' and row['source_id'] == 'edgecore-7760-label':
                    assert json.loads(after[key]) == label_row
                else:
                    assert after[key] == row[key], (row['id'],key)
            if row['source_id'] == 'broadcom-servers':
                expected = bios_certification_rows(json.loads(row['raw_json'])['detail'])
                fact = new.execute("SELECT value FROM facts WHERE record_id=? AND name='compatibility_bios_certification_rows'",(row['id'],)).fetchone()
                assert fact is not None and json.loads(fact[0]) == expected
                bios_total += len(expected)
                literal.append(dict(record_id=row['id'],raw_json_sha256=hashlib.sha256(row['raw_json'].encode()).hexdigest(),rows=expected))
        assert len(literal) == 24 and bios_total == 206
        sql = "SELECT r.source_key,count(*) AS rows FROM facts f JOIN records r ON r.id=f.record_id JOIN json_each(f.value) j WHERE f.name='compatibility_bios_certification_rows' GROUP BY r.source_key ORDER BY r.source_key"
        expanded = [dict(r) for r in new.execute(sql)]
        assert len(expanded) == 24 and sum(r['rows'] for r in expanded) == 206
        source_changes = {}
        for before in old.execute('SELECT * FROM sources'):
            after = new.execute('SELECT * FROM sources WHERE id=?',(before['id'],)).fetchone()
            changes = {key:dict(before=before[key],after=after[key]) for key in before.keys() if before[key] != after[key]}
            assert set(changes) <= set(checkpoint['source_allowed_changes'].get(before['id'],[])), (before['id'],changes)
            if changes:
                source_changes[before['id']] = changes
                assert before['id'] == 'edgecore-7760-label' and set(changes) == {'coverage'}
                assert after['coverage'] == json.loads(LABEL.read_text())['sources'][0]['coverage']
        old_facts, new_facts = rows(old,'facts'), rows(new,'facts')
        additions = list((new_facts-old_facts).elements())
        assert len(additions) == 26
        expected_label = {(name,value) for name,value in label_row['facts']} - {(name,value) for name,value in json.loads((DATA/'label-evidence.json').read_text())['devices'][0]['facts']}
        assert len(expected_label) == 2
        for rid,name,value in additions:
            source = new.execute('SELECT source_id FROM records WHERE id=?',(rid,)).fetchone()[0]
            assert (source == 'broadcom-servers' and name == 'compatibility_bios_certification_rows') or (source == 'edgecore-7760-label' and (name,value) in expected_label)
    return dict(tables=audit,source_changes=source_changes,bios_literal_rows=literal,json_each_sql=sql,json_each_result=expanded,new_facts=additions)


def stage():
    from devicehunt.catalog import Catalog
    from devicehunt.pipeline import import_source
    from devicehunt.rebuild import read_manifest
    checkpoint = acceptance('stage')
    preflight()
    _, manifest = read_manifest(DATA/'import-manifest.json')
    copy_new(LIVE,CAND)
    catalog = Catalog(CAND)
    try:
        for source,path in [('broadcom',BUNDLE),('curated',LABEL)]:
            print(source, import_source(catalog,source,path),flush=True)
    finally:
        catalog.db.close()
    audit = preservation(checkpoint)
    audit.update(database_sha256=sha(CAND),previous_sha256=BEFORE,checks=checks(CAND))
    assert sha(LIVE) == BEFORE
    save(HERE/'candidate-preservation.json',audit)
    copy_new(DATA/'import-manifest.json',DATA/('import-manifest-before-'+TAG+'.json'))
    jobs = [job for job in manifest['imports'] if job['source']=='curated' and job['path']=='label-evidence.json']
    assert len(jobs) == 1
    jobs[0]['path'] = LABEL.name
    manifest['files'][LABEL.name] = sha(LABEL)
    assert len(manifest['imports']) == 116 and len(manifest['files']) == 2172
    # No facts key: rebuild actual/expected compares a fixed set of counts.
    temporary = DATA/('.manifest-'+TAG+'.json')
    save(temporary,manifest)
    read_manifest(temporary)
    acceptance('stage')
    os.replace(temporary,DATA/'import-manifest.json')


def rebuild():
    acceptance('stage')
    assert sha(LIVE) == BEFORE and not REBUILD.exists()
    subprocess.run([sys.executable,'-m','devicehunt','--db',str(REBUILD),'rebuild','--manifest',str(DATA/'import-manifest.json')],cwd=ROOT,check=True)
    subprocess.run([sys.executable,'scripts/compare_rebuild.py',str(REBUILD),'--database',str(CAND),'--output-prefix',TAG+'-rebuild'],cwd=ROOT,check=True)
    with read_only(CAND) as original,read_only(REBUILD) as rebuilt:
        for row in original.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"):
            table = row[0]
            if table.startswith(('search_index_','document_search_')):
                continue  # FTS physical segment layout depends on write history.
            assert rows(original,table,('observed_at','fetched_at','cache_path')) == rows(rebuilt,table,('observed_at','fetched_at','cache_path')),table
    save(HERE/'rebuild-proof.json',dict(database_sha256=sha(CAND),rebuild_sha256=sha(REBUILD),checks=checks(REBUILD),duplicate_sensitive_equal=True))


def proof(database, label):
    from devicehunt.catalog import Catalog
    from devicehunt.__main__ import report
    before = sha(database)
    buying_path = HERE/(label+'-buying-reports.json')
    if buying_path.exists():
        assert json.loads(buying_path.read_text())['database_sha256'] == before
    else:
        buying_snapshot(database,buying_path)
    arguments = [['buying-report',query,'--format','json'] for query in ('7760-32X-A-AC-F','E4-2C','E5-2C','X10-2C','X7-2C')]
    documented_sql = (BATCH/'documented-bios-query.sql').read_text()
    arguments.append(['sql',documented_sql,'--format','json'])
    results = []
    for args in arguments:
        result = subprocess.run([sys.executable,'-m','devicehunt','--db',str(database),*args],cwd=ROOT,capture_output=True,text=True,check=True)
        payload = json.loads(result.stdout)
        if args[0] == 'sql':
            assert len(payload) == 206 and all(row['source_url'] for row in payload)
        else:
            if args[1] == '7760-32X-A-AC-F':
                assert any(row['statement']=='100-240~, 50 - 60Hz, 6A Max' for row in payload['categories']['power']['statements'])
            else:
                bios = [row for row in payload['categories']['firmware_boot_software']['statements'] if row['fact_name']=='compatibility_bios_certification_rows']
                if args[1] == 'X7-2C':
                    assert not bios
                else:
                    assert len(bios) == 1 and len(json.loads(bios[0]['statement'])) == 3
        results.append(dict(arguments=args,result=payload))
    html = HERE/(label+'-catalog.html')
    assert not html.exists()
    catalog = Catalog(database,read_only=True)
    try:
        report(catalog,html)
    finally:
        catalog.db.close()
    node = r'''const fs=require('fs'),assert=require('assert/strict');const {mount}=require('./tests/report.test.cjs');const html=fs.readFileSync(process.argv[1],'utf8');const template=fs.readFileSync('devicehunt/report.html','utf8');const script=x=>x.match(/<script>\s*([\s\S]*?)<\/script>/)[1];assert.equal(script(html),script(template));const data=JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);assert.equal(data.length,4083);const ui=mount(data);ui.set('scope','all');ui.set('query','7760-32X-A-AC-F');assert.ok(ui.titles().some(x=>x.includes('7760-32X-A-AC-F')));ui.set('vendor','ByteDance');ui.set('query','');assert.equal(ui.titles().length,0);'''
    result = subprocess.run(['node','-e',node,str(html)],cwd=ROOT,text=True,capture_output=True,check=True)
    assert sha(database) == before
    save(HERE/(label+'-cli-dom-proof.json'),dict(database_sha256=before,read_only=True,cli=results,html_sha256=sha(html),dom_stdout=result.stdout,dom='generated HTML payload and inline script; no screenshot'))


def finish():
    checkpoint = acceptance('promotion')
    assert sha(LIVE) == BEFORE
    audit = json.loads((HERE/'candidate-preservation.json').read_text())
    assert sha(CAND) == audit['database_sha256']
    comparison = json.loads((DATA/'exports'/(TAG+'-rebuild-comparison.json')).read_text())
    assert len(comparison) == 43 and all(r['original']==r['rebuilt'] and r['only_original']==r['only_rebuilt']==0 for r in comparison.values())
    for artifact in ('rebuild-proof.json','candidate-cli-dom-proof.json'):
        assert json.loads((HERE/artifact).read_text())['database_sha256'] == sha(CAND)
    checks(CAND)
    copy_new(LIVE,DATA/('devices-before-'+TAG+'.sqlite'))
    staging = DATA/('.devices-'+TAG+'-promote.sqlite')
    copy_new(CAND,staging)
    acceptance('promotion')
    os.replace(staging,LIVE)
    for script in ('scripts/refresh_exports.py','scripts/refresh_collection_reports.py'):
        with (HERE/(Path(script).stem+'.log')).open('x') as log:
            subprocess.run([sys.executable,script],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
    subprocess.run([sys.executable,'scripts/compare_rebuild.py',str(REBUILD),'--database',str(LIVE),'--output-prefix','rebuild'],cwd=ROOT,check=True)
    buying_snapshot(LIVE,DATA/'exports'/(TAG+'-buying-reports.json'))
    proof(LIVE,'final')
    checks(LIVE)
    from devicehunt.rebuild import read_manifest
    _,manifest = read_manifest(DATA/'import-manifest.json')
    latest = json.loads((DATA/'exports/latest-export-audit.json').read_text())
    assert latest['database_sha256'] == sha(LIVE) and latest['lookup_reports_refreshed'] == 98
    for name in ('collection-reports-audit','hyperscaler-inventory-audit'):
        assert json.loads((DATA/'exports'/(name+'.json')).read_text())['database_sha256'] == sha(LIVE)
    verified_pdfs = []
    with read_only(LIVE) as connection:
        counts = {table:connection.execute('SELECT count(*) FROM '+table).fetchone()[0] for table in ('devices','records','sources','facts','documents','document_pages')}
        for row in connection.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id'):
            path = Path(row['cache_path']).parent/row['pdf_file']
            assert sha(path) == row['pdf_sha256']
            verified_pdfs.append(dict(source=row['source_id'],path=str(path),sha256=sha(path)))
    assert len(verified_pdfs) == 55
    acceptance('promotion')
    save(HERE/'final-verification.json',dict(database_sha256=sha(LIVE),previous_sha256=BEFORE,rebuild_sha256=sha(REBUILD),schema=28,counts=counts,checks=checks(LIVE),manufacturer_inventory=latest['inventories']['manufacturer'],manufacturer_factory_unchanged_before_promotion=True,hyperscaler_records=22,manifest_jobs=len(manifest['imports']),hashed_inputs=len(manifest['files']),verified_pdfs=verified_pdfs,lookup_reports_refreshed=98,buying_reports=10,logical_tables=43,tests=checkpoint['test_logs_sha256']))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command',choices=('preflight','stage','rebuild','proof','finish'),nargs='?',default='preflight')
    args = parser.parse_args()
    if args.command == 'proof':
        acceptance('stage')
        proof(CAND,'candidate')
    else:
        globals()[args.command]()

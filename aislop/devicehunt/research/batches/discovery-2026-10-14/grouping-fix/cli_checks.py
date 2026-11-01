"""Run an explicit overlay CLI process against a private fixture only."""
import datetime
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import devicehunt
from devicehunt import specifications, __main__
from devicehunt.catalog import Catalog
OWN=Path(__file__).resolve().parent
assert Path.cwd()==OWN
assert Path(os.environ['PYTHONPATH']).resolve()==OWN/'overlay'
paths={m.__name__:str(Path(m.__file__).resolve()) for m in (devicehunt,specifications,__main__)}
assert all(Path(p).is_relative_to(OWN/'overlay') for p in paths.values())
env=os.environ.copy();env['PYTHONPATH']=str(OWN/'overlay');env['PYTHONDONTWRITEBYTECODE']='1'
child='''import json,sys;from pathlib import Path;import devicehunt;from devicehunt import specifications,__main__;root=Path(sys.argv[1]);paths={m.__name__:str(Path(m.__file__).resolve()) for m in (devicehunt,specifications,__main__)};assert all(Path(p).is_relative_to(root) for p in paths.values());print(json.dumps(paths))'''
resolved=json.loads(subprocess.check_output([sys.executable,'-B','-c',child,str(OWN/'overlay')],cwd=OWN,env=env,text=True))
with tempfile.TemporaryDirectory() as tmp:
    db=Path(tmp)/'fixture.sqlite'
    c=Catalog(db)
    for sid in ('a','b'):
        c.source(sid,'https://example.invalid/'+sid,'vendor','fixture','fixture')
        c.add(sid,'one','https://example.invalid/'+sid,'Celestica','CLI-1','Celestica CLI-1','switch',{},[('Celestica','documentation_vendor')],facts=[('power_description','2 PSUs')])
        c.finish(sid,1)
    c.db.close()
    before=hashlib.sha256(db.read_bytes()).hexdigest()
    completed=subprocess.run([sys.executable,'-B','-m','devicehunt','--db',str(db),'specs','CLI-1','--group-identical','--format','json'],cwd=OWN,env=env,text=True,capture_output=True,check=True)
    output=json.loads(completed.stdout)
    assert output[0]['evidence_count']==2
    after=hashlib.sha256(db.read_bytes()).hexdigest()
    assert before==after
    missing=Path(tmp)/'missing.sqlite'
    failed=subprocess.run([sys.executable,'-B','-m','devicehunt','--db',str(missing),'specs','CLI-1','--group-identical'],cwd=OWN,env=env,text=True,capture_output=True)
    assert failed.returncode!=0 and not missing.exists()
    help_out=subprocess.run([sys.executable,'-B','-m','devicehunt','specs','--help'],cwd=OWN,env=env,text=True,capture_output=True,check=True)
    assert '--group-identical' in help_out.stdout
    assert 'Limits count complete groups' in ' '.join(help_out.stdout.split())
    assert not (OWN/'overlay/data').exists()

    # Exercise the raw-value repair through actual child CLI JSON/CSV/table paths.
    c=Catalog(db)
    record=c.db.execute("SELECT id FROM records WHERE source_id='a'").fetchone()['id']
    raw_values=['Rack Mount Chassis (a/0123456789ab:1)', 'Rack Mount Chassis (b/0123456789ab:2)']
    c.db.executemany('INSERT INTO facts VALUES (?,?,?)',[(record,'dmi_chassis_type',v) for v in raw_values])
    c.db.commit();c.db.close()
    raw_before=hashlib.sha256(db.read_bytes()).hexdigest()
    rendered={}
    for fmt in ('json','csv','table'):
        done=subprocess.run([sys.executable,'-B','-m','devicehunt','--db',str(db),'specs','CLI-1',
                             '--group-identical','--include-source-facts','--format',fmt],
                            cwd=OWN,env=env,text=True,capture_output=True,check=True)
        rendered[fmt]=done.stdout
    raw_after=hashlib.sha256(db.read_bytes()).hexdigest()
    assert raw_before==raw_after
    raw_groups=json.loads(rendered['json'])
    csv_rows=list(csv.DictReader(io.StringIO(rendered['csv'])))
    assert [json.loads(row['evidence_json']) for row in csv_rows]==[g['evidence'] for g in raw_groups]
    dmi_headers=[json.loads(line) for line in rendered['table'].splitlines()
                 if not line.startswith('  evidence ') and json.loads(line)['source_fact_name']=='dmi_chassis_type']
    assert len(dmi_headers)==2 and dmi_headers[0]!=dmi_headers[1]
    assert {row['raw_fact']['value'] for row in dmi_headers}==set(raw_values)
    assert rendered['table'].count('2 PSUs')==1
    assert rendered['table'].count('  evidence ')==4
    for value in raw_values:assert rendered['table'].count(value)==1
    help_widths=[]
    for width in (40,80,120):
        done=subprocess.run([sys.executable,'-B','-m','devicehunt','specs','--help'],
                            cwd=OWN,env=env|{'COLUMNS':str(width)},text=True,capture_output=True,check=True)
        normalized=' '.join(done.stdout.split())
        assert '--group-identical' in normalized and 'Limits count complete groups' in normalized
        help_widths.append({'columns':width,'normalized_phrase_present':True})
proof={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'module_paths':paths,'child_module_paths':resolved,'cwd':str(OWN),'pythonpath':env['PYTHONPATH'],'fixture_sha256_before':before,'fixture_sha256_after':after,'grouped_output':output,'missing_db_returncode':failed.returncode,'missing_db_not_created':True,'overlay_default_data_absent':True,'help':help_out.stdout,'raw_fixture_sha256_before':raw_before,'raw_fixture_sha256_after':raw_after,'raw_table_groups_distinguishable':True,'raw_json_csv_evidence_equal':True,'raw_table':rendered['table'],'help_widths':help_widths}
(OWN/'cli-checks.json').write_text(json.dumps(proof,indent=2)+'\n')
print('CLI path/assertion, grouped evidence, read-only fixture, missing-db, help: PASS')

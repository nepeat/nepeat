"""Check actual offline CLI, source-scoped specs and illustrative listing matching."""
import json
import subprocess
import sys
from pathlib import Path
from devicehunt.rebuild import file_sha256
ROOT=Path(__file__).resolve().parents[2]
DB=ROOT/'data/devices.sqlite'; OUT=ROOT/'data/exports'
before=file_sha256(DB)
checks=[]
cases=[
(['vendor-inventory','Inspur','--scope','all','--limit','-1','--format','json'],133),
(['vendor-inventory','Inspur','--scope','manufacturer','--limit','-1','--format','json'],3),
(['vendor-inventory','Inventec','--scope','all','--limit','-1','--format','json'],81),
(['specs','OB5162M5','--vendor','Inspur','--exact-model','--format','json'],'Open19 power shelf'),
(['specs','SN3116J4','--vendor','Inspur','--exact-model','--format','json'],'12V'),
(['specs','AGX-2','--vendor','Inspur','--exact-model','--format','json'],'3000W'),
(['specs','1020E','--vendor','Inspur','--exact-model','--format','json'],'200-240V'),
(['documents','X680m','--format','json'],None),
(['hyperscaler-inventory','--vendor','Inspur','--format','json'],None),
(['match-listing','--title','Inspur OB5162M5 bare node untested','--vendor','Inspur','--with-specs','--format','json'],'Open19 power shelf'),
(['validate-inventory','--inventory','validation/owned-hardware.json','--format','json'],10),
(['validate-inventory','--inventory','validation/vendor-examples.json','--format','json'],0),
]
for args,expected in cases:
    run=subprocess.run([sys.executable,'-m','devicehunt',*args],cwd=ROOT,capture_output=True,text=True)
    gap=args[0]=='validate-inventory' and expected==0
    assert run.returncode==(2 if gap else 0),(args,run.stderr)
    result=json.loads(run.stdout)
    if args[0]=='vendor-inventory': assert result['unique_identities']==expected
    elif args[0]=='validate-inventory': assert result['summary']['hits']==expected
    elif args[0] in ('specs','match-listing'): assert expected in json.dumps(result)
    elif args[0]=='documents': assert result['matches']>0 and any(r['source_id']=='reference-pdf-inventec-x680m' for r in result['rows'])
    elif args[0]=='hyperscaler-inventory':
        assert result['assertions']==1 and result['unique_identities']==1
        assert 'documented_design_lineage' in json.dumps(result) and 'Baidu' in json.dumps(result)
    if args[0]=='match-listing':
        candidates=result['results'][0]['result']['candidates']
        assert len(candidates)==1 and candidates[0]['model']=='OB5162M5'
        assert 'untested' in result['results'][0]['result']['context_terms']
    checks.append(dict(arguments=args,exit_code=run.returncode,result=result))
assert file_sha256(DB)==before
(OUT/'inspur-2018-inventec-cli-checks-2026-10-04.json').write_text(json.dumps(dict(database_sha256=before,read_only=True,listing_example='Illustrative title, not a collected marketplace listing',checks=checks),ensure_ascii=False,indent=2)+'\n')
print('Verified inventories, exact specs, PDF search, Baidu design-only assertion, illustrative title matching and both validation sets.')

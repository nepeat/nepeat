"""Exercise the real CLI, including literal grouped-model lookup and raw SQL."""
import json
import subprocess
import sys
from pathlib import Path
from devicehunt.rebuild import file_sha256

ROOT=Path(__file__).resolve().parents[2]
before=file_sha256(ROOT/'data/devices.sqlite')
checks=[]
for args in (
    ['lookup','i24&NS5162M5','--format','json'],
    ['sql',"SELECT product_id,parent_id,name FROM inspur_support_entries WHERE parent_id='115' ORDER BY name",'--format','json'],
    ['search','Metagross','--vendor','Inventec','--format','json'],
    ['specs','Metagross','--format','json'],
    ['specs','E850G4','--format','json'],
    ['specs','NF5280M5','--format','json'],
    ['validate-inventory','--inventory','validation/vendor-examples.json','--format','json'],
):
    p=subprocess.run([sys.executable,'-m','devicehunt',*args],cwd=ROOT,capture_output=True,text=True)
    assert p.returncode==(2 if args[0]=='validate-inventory' else 0),(args,p.stderr)
    data=json.loads(p.stdout);assert data
    if args[0]=='lookup':
        raw=next(s for s in data['datasets'] if s['dataset']=='inspur_support')
        assert raw['stored_rows']==1064 and raw['matches']==1
        assert raw['rows'][0]['name']=='i24&NS5162M5'
    elif args[0]=='sql':assert len(data)==148
    elif args[0]=='search':assert len(data)==1 and data[0]['model']=='Metagross'
    elif args[0]=='validate-inventory':assert data['summary']==dict(total=2,hits=0,gaps=2)
    checks.append(dict(arguments=args,exit_code=p.returncode,result=data))
assert file_sha256(ROOT/'data/devices.sqlite')==before
(ROOT/'data/exports/storage-inspur-cli-checks-2026-10-04.json').write_text(
    json.dumps(dict(database_sha256=before,read_only=True,checks=checks),indent=2)+'\n')
print('Verified support lookup/SQL, model search, three specification queries and explicit ByteDance gaps.')

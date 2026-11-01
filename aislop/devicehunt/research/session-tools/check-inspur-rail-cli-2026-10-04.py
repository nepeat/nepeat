"""Exercise exact node/chassis specifications, paired-model SQL and source boundaries."""
import json
import subprocess
import sys
from pathlib import Path
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
before = file_sha256(ROOT/'data/devices.sqlite')
sql = "SELECT d.model,f.name,f.value FROM facts f JOIN records r ON r.id=f.record_id JOIN devices d ON d.id=r.device_id WHERE r.source_id='inspur-incloud-rail-hardware-2023' AND f.name='server_node_model' ORDER BY d.model"
checks = []
for args in (
    ['vendor-inventory','Inspur','--scope','all','--limit','-1','--format','json'],
    ['vendor-inventory','Inspur','--scope','manufacturer','--limit','-1','--format','json'],
    ['specs','NS5162M5','--vendor','Inspur','--exact-model','--format','json'],
    ['specs','i24','--vendor','Inspur','--exact-model','--format','json'],
    ['specs','NE5260M5','--vendor','Inspur','--exact-model','--format','json'],
    ['sql',sql,'--format','json'],
    ['lookup','i24&NS5162M5','--format','json'],
    ['documents','CN61108PC-V-H','--format','json'],
    ['validate-inventory','--inventory','validation/vendor-examples.json','--format','json'],
):
    p = subprocess.run([sys.executable,'-m','devicehunt',*args],cwd=ROOT,capture_output=True,text=True)
    assert p.returncode == (2 if args[0]=='validate-inventory' else 0), (args,p.stderr)
    data = json.loads(p.stdout)
    assert data
    if args[0]=='vendor-inventory':
        assert data['unique_identities'] == (47 if args[3]=='all' else 3), data
    elif args[0]=='specs':
        assert all(row['model']==args[1] for row in data), (args,data)
    elif args[0]=='sql':
        assert len(data)==9
        assert {r['value'] for r in data} == {'NF5280M7','NF8480M7','NF5280M6','NF5266M6','NF8480M6','NF5468M6','NE5260M5','NF8480M5','NF5280M5'}
    elif args[0]=='lookup':
        raw = next(s for s in data['datasets'] if s['dataset']=='inspur_support')
        assert raw['stored_rows']==1064 and raw['matches']==1
        assert raw['rows'][0]['name']=='i24&NS5162M5'
    elif args[0]=='validate-inventory':
        assert data['summary']==dict(total=2,hits=0,gaps=2)
    checks.append(dict(arguments=args,exit_code=p.returncode,result=data))
assert file_sha256(ROOT/'data/devices.sqlite')==before
(ROOT/'data/exports/inspur-rail-cli-checks-2026-10-04.json').write_text(
    json.dumps(dict(database_sha256=before,read_only=True,checks=checks),indent=2)+'\n')
print('Verified Inspur inventories, three exact specification queries, nine paired models, raw lookup, PDF search and ByteDance gaps.')

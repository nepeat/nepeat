import sys, json, shutil, hashlib, tempfile
from pathlib import Path
sys.dont_write_bytecode=True
P='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/proto'
sys.path.insert(0,P)
from devicehunt import rra, campaign_sources, rra_manufacturer_lists
R='/home/erin/nepeat/aislop/devicehunt/'
src=Path(P)/'data/raw/rra-celestica-official'
good=src/'bundle-composite-cells.json'
def attempt(label, fn):
    try: r=fn(); print('OK  ',label, r)
    except Exception as e: print('REFUSED',label,'->',type(e).__name__, str(e)[:110])
A=campaign_sources.RraSavedAdapter()
attempt('composite bundle (control)', lambda: A.records(good,'x')['stats'])
# 1 Oracle saved bundle unchanged by the code
o=Path(R+'data/raw/rra-oracle-official/bundle.json')
inv=json.load(open(R+'research/batches/campaign-c3b-2026-10-10/inventory/inventory.json'))
exp=[u for u in inv['units'] if u['unit_id']=='rra-official-query-73009f906e4e'][0]['expectations']
out=A.records(o,'data/raw/rra-oracle-official/bundle.json')
print('oracle compare:', A.compare(out,exp))
# 2 old celestica bundle (no wrapper) behaves exactly as before
old=A.records(src/'bundle.json','data/raw/rra-celestica-official/bundle.json')
exp2=[u for u in inv['units'] if u['unit_id']=='rra-official-query-41e4ca912b33'][0]['expectations']
print('celestica old bundle compare:', A.compare(old,exp2))
print('celestica composite compare:', A.compare(out if False else A.records(good,'data/raw/rra-celestica-official/bundle-composite-cells.json'),exp2))
# 3 wrong query maker: wrapper declared by an Oracle-maker bundle
tmp=Path(tempfile.mkdtemp(dir=P)); 
for f in ['rra-composite-manufacturer-cells-2026-10-11.json','celestica-historical-companies.json','celestica-subsidiaries-2025.json','celestica-sec-8.1-reviewed.json','celestica-archived-21.1-reviewed.json']: shutil.copy(src/f,tmp/f)
ob=json.load(open(o.parent/'bundle.json')); 
for f in o.parent.iterdir():
    if f.name!='bundle.json' and f.is_file(): shutil.copy(f,tmp/f.name)
ob['company_evidence']=json.load(open(good))['company_evidence']
(tmp/'bundle.json').write_text(json.dumps(ob,indent=2))
attempt('wrapper under the Oracle query', lambda: rra.validate(tmp/'bundle.json')[0]['query']['maker'])
# 4 tampered cell text in the wrapper
w=json.load(open(src/'rra-composite-manufacturer-cells-2026-10-11.json'))
t2=Path(tempfile.mkdtemp(dir=P)); shutil.copytree(src,t2,dirs_exist_ok=True)
w['entries'][0]['parts'][0]['text']='Celestica de Monterrey S.A.'
(t2/'rra-composite-manufacturer-cells-2026-10-11.json').write_text(json.dumps(w,indent=2,ensure_ascii=False))
b=json.load(open(t2/'bundle-composite-cells.json')); b['company_evidence']['sha256']=hashlib.sha256((t2/'rra-composite-manufacturer-cells-2026-10-11.json').read_bytes()).hexdigest()
(t2/'bundle-composite-cells.json').write_text(json.dumps(b,indent=2))
attempt('wrapper part text edited (no longer reproduces the cell)', lambda: rra.validate(t2/'bundle-composite-cells.json')[0]['expected_rows'])
# 5 detail page changed (cell changed upstream): different detail bytes with the same cert but another cell
w=json.load(open(src/'rra-composite-manufacturer-cells-2026-10-11.json'))
t3=Path(tempfile.mkdtemp(dir=P)); shutil.copytree(src,t3,dirs_exist_ok=True)
w['entries'][0]['certificate_number']='SUN-T6300'
(t3/'rra-composite-manufacturer-cells-2026-10-11.json').write_text(json.dumps(w,indent=2,ensure_ascii=False))
b=json.load(open(t3/'bundle-composite-cells.json')); b['company_evidence']['sha256']=hashlib.sha256((t3/'rra-composite-manufacturer-cells-2026-10-11.json').read_bytes()).hexdigest()
(t3/'bundle-composite-cells.json').write_text(json.dumps(b,indent=2))
attempt('entry keyed to a certificate whose cell differs (matches no row)', lambda: rra.validate(t3/'bundle-composite-cells.json')[0]['expected_rows'])
# 6 manufacturer-lists with old bundle unchanged
m=Path(R+'data/raw/rra-manufacturer-lists.json')
print('manufacturer-lists summary:', rra_manufacturer_lists.prepare(m)[2])
shutil.rmtree(tmp); shutil.rmtree(t2); shutil.rmtree(t3)

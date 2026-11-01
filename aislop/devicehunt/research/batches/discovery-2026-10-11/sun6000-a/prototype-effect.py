import sys, json, sqlite3, shutil
from pathlib import Path
sys.dont_write_bytecode=True
P='/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/proto'
sys.path.insert(0,P)
from devicehunt import vendor_inventory
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
LIVE='/home/erin/nepeat/aislop/devicehunt/data/devices.sqlite'
copy=Path(P)/'live-copy.sqlite'
if copy.exists(): copy.unlink()
src=sqlite3.connect('file:'+LIVE+'?mode=ro',uri=True); dst=sqlite3.connect(copy); src.backup(dst); dst.close(); src.close()
def metrics(path):
    db=sqlite3.connect(path); db.row_factory=sqlite3.Row
    out={}
    for scope in ('manufacturer','all'):
        r=vendor_inventory.run(db,None,scope,0)
        out[scope]=dict(pairs=r['identity_target_pairs'],ids=r['unique_identities'],by={c['target']:(c['identities'],c['source_records'],c['role_assertions']) for c in r['counts_by_target'] if c['target'] in ('Celestica','Oracle')})
    out['devices']=db.execute('select count(*) from devices').fetchone()[0]
    out['records']=db.execute('select count(*) from records').fetchone()[0]
    out['affiliations']=db.execute('select count(*) from affiliations').fetchone()[0]
    out['facts']=db.execute('select count(*) from facts').fetchone()[0]
    out['identifiers']=db.execute('select count(*) from identifiers').fetchone()[0]
    out['rra_equipment']=db.execute('select count(*) from rra_equipment').fetchone()[0]
    out['sources']=db.execute('select count(*) from sources').fetchone()[0]
    out['seen']=db.execute("select records_seen from sources where id='rra-official-query-41e4ca912b33'").fetchone()[0]
    return out
before=metrics(copy)
c=Catalog(copy)
print(import_source(c,'rra',Path(P)/'data/raw/rra-celestica-official/bundle-composite-cells.json'))
c.db.commit(); c.db.close()
after=metrics(copy)
print(json.dumps(before)); print(json.dumps(after))

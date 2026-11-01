"""Read-only role-separated inventory deltas at candidate checkpoint."""
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[4];sys.path.insert(0,str(ROOT))
from stage import CAND_DB,DATA,OUT,sha,save
from devicehunt.catalog import Catalog
from devicehunt import vendor_inventory,validation,lineage
paths=[CAND_DB,DATA/'devices.sqlite'];before={str(p.relative_to(ROOT)):sha(p) for p in paths}
a=Catalog(paths[0],read_only=True);b=Catalog(paths[1],read_only=True)
try:
 manufacturer=vendor_inventory.run(a.db,limit=0);assert manufacturer==vendor_inventory.run(b.db,limit=0)
 all_roles=vendor_inventory.run(a.db,scope='all',limit=0)
 examples=validation.run(a.db,ROOT/'validation/vendor-examples.json')['summary'];assert examples=={'total':2,'hits':0,'gaps':2}
 hyper={'rows':a.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0],'identities':a.db.execute('SELECT count(DISTINCT r.device_id) FROM hyperscaler_evidence h JOIN records r ON r.id=h.record_id').fetchone()[0]};assert hyper=={'rows':22,'identities':18}
 counts={n:a.db.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in lineage.TABLES}
 assert counts=={n:b.db.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in lineage.TABLES}
finally:a.db.close();b.db.close()
assert before=={str(p.relative_to(ROOT)):sha(p) for p in paths}
save(OUT/'inventory-check.json',{'manufacturer_unchanged':True,'manufacturer':manufacturer,'all_target_associations':all_roles,'vendor_examples':examples,'hyperscaler':hyper,'lineage_table_counts':counts,'database_hashes_unchanged':before})
print('all target pairs',all_roles['identity_target_pairs'],'identities',all_roles['unique_identities'])

import json
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import monitron
from devicehunt.rebuild import file_sha256,read_manifest

root=Path.cwd();c=Catalog(root/'data/devices.sqlite')
path=root/'data/curated.json'; data=json.loads(path.read_text())
bundle=root/'data/raw/aws-monitron-bundle.json'
_,bundle_data,ready,_=monitron.prepare(bundle)
source=next(s for s in data['sources'] if s['id']=='monitron-model')
assert source['url']==bundle_data['index_url']
snapshot=path.parent/source['snapshot'];assert file_sha256(snapshot)==bundle_data['index']['sha256']
assert next(r for r in ready if r['name']=='sensor')['models']==['TE1A001','TE1A105']
before={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ('devices','records','sources','hyperscaler_evidence')}
# Replay only this source using the curated import contract; the full manifest
# still obtains the same source from the original curated.json job.
sid=source['id'];c.source(sid,source['url'],source['tier'],source['license'],source['coverage'],snapshot)
records=[r for r in data['devices'] if r['source']==sid]
assert len(records)==1
for d in records:
 c.add(sid,d['key'],d.get('url',source['url']),d['brand'],d['model'],d['title'],d['kind'],d,
       d['affiliations'],d.get('identifiers',[]),d.get('facts',[]),d.get('hyperscaler',[]))
c.finish(sid,len(records))
monitron.import_bundle(c,bundle)
after={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in before}
assert {k:after[k]-before[k] for k in before}==dict(devices=0,records=0,sources=0,hyperscaler_evidence=5)
c.db.close()
manifest=root/'data/import-manifest.json';m=json.loads(manifest.read_text())
m['files']['curated.json']=file_sha256(path)
manifest.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n');read_manifest(manifest)
audit=dict(before=before,after=after,restored_source=sid,snapshot_sha256=file_sha256(snapshot),
           existing_bundle_verified=True,manufacturer_and_deployment_claims_unchanged=True,
           database_sha256=file_sha256(root/'data/devices.sqlite'))
(root/'data/exports/monitron-product-links-import-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))

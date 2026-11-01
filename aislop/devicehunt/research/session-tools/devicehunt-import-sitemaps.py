import json,hashlib
from pathlib import Path
from devicehunt import fcc_sitemaps as f
from devicehunt.catalog import Catalog
root=Path('data/raw/fcc-sitemaps');report=json.loads((root/'collection-report.json').read_text());children=[]
for e in report['entries']:
 if e.get('status')==200:
  rows=f.parse_urls((root/e['path']).read_bytes())
  children.append(dict(url=e['url'],status='captured',page={k:e[k] for k in ('path','sha256')},row_count=len(rows)))
 else:children.append(dict(url=e['url'],status='unavailable',issue=e['error']))
bundle=root/'bundle.json';bundle.write_text(json.dumps(dict(schema_version=1,source_url=f.URL,index={k:report['index'][k] for k in ('path','sha256')},children=children),indent=2)+'\n')
f.validate(bundle)
db=Path('data/devices.sqlite');assert hashlib.sha256(db.read_bytes()).hexdigest()=='8405cf108febcbeb86619166f9d2ca8293784e1d367dd8f4779b97f02de80a44'
c=Catalog(db);result=f.import_bundle(c,bundle);c.db.close();print(result)
m=Path('data/import-manifest.json');data=json.loads(m.read_text());assert not any(j['source']=='fcc-sitemaps' for j in data['imports']);data['imports'].append(dict(source='fcc-sitemaps',path=str(bundle.relative_to(m.parent))))
for p in sorted(root.iterdir()):
 if p.is_file():data['files'][str(p.relative_to(m.parent))]=hashlib.sha256(p.read_bytes()).hexdigest()
data['expected']['sources']['fcc-sitemaps']=result['url_rows'];data['expected']['fcc_sitemap_pages']=result['sitemaps'];data['expected']['fcc_sitemap_urls']=result['url_rows'];m.write_text(json.dumps(data,indent=2)+'\n')
print('jobs',len(data['imports']),'files',len(data['files']),'sha',hashlib.sha256(db.read_bytes()).hexdigest())

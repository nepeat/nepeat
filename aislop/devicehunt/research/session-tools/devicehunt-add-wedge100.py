import json, hashlib, sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.importers import curated
from devicehunt.meta_networking import Article
from devicehunt.rebuild import file_sha256, read_manifest

root=Path.cwd(); raw=root/'data/raw/meta-wedge100'
url='https://engineering.fb.com/2016/10/18/data-center-engineering/wedge-100-more-open-and-versatile-than-ever/'
sid='meta-wedge100-2016'; reviewed=root/'data/meta-wedge100-reviewed.json'
html=raw/'article.html'; sha=file_sha256(html)
fetch=json.loads((raw/'fetch-report.json').read_text())
assert fetch['status']==200 and fetch['url']==fetch['final_url']==url
assert fetch['sha256']==sha and fetch['bytes']==html.stat().st_size
a=Article(html.read_text())
assert a.metadata['og:url']==[url] and a.dates==['2016-10-18']
assert a.metadata['article:published_time']==['2016-10-18T16:00:00+00:00']
def evidence(index, *phrases):
    tag,text=a.blocks[index]
    assert tag=='p' and all(p in text for p in phrases)
    return dict(block_index=index,tag=tag,text_sha256=hashlib.sha256(text.encode()).hexdigest())
deployment=evidence(5,'we use Wedge 100 in our production environments','deploy it at scale across our data centers')
manufacturing=evidence(21,'Edgecore’s Wedge 100-32X','manufactured by Edgecore’s parent company, Accton Technology','also manufactures the Wedge 100 for our network deployment')
mounting=evidence(11,'19” rack','21” Open Rack adapter tray','12V bus bar','PSU pass through module')
software=evidence(21,'shipped with diagnostics, OpenBMC firmware','Open Network Install Environment')
scope=('Historical first-party article dated 2016-10-18. Explicit Accton manufacturer claims for the two printed model names. Facebook production deployment applies to Wedge 100; the commercial Wedge 100-32X has documented design lineage only. No individual-unit provenance, present stock, support, firmware availability or current ownership is established. Namespace and whitespace variants remain separate; no cross-brand or suffix aliases are inferred.')
common=[['published_date','2016-10-18'],['source_html_sha256',sha],['evidence_scope',scope]]
devices=[
 dict(source=sid,key='wedge100-facebook',url=url,brand='Accton',model='Wedge 100',title='Accton-manufactured Facebook Wedge 100',kind='network switch',
      affiliations=[['Accton Technology','manufacturer'],['Facebook','platform_designer']],
      facts=common+[['catalog_namespace','Accton is the named manufacturer, not an asserted retail brand.'],
                    ['manufacturer_statement','Facebook names Accton Technology as manufacturer of Wedge 100 for its network.'],
                    ['physical_description','2016 design: 19-inch chassis; Facebook describes a separate 21-inch Open Rack adapter.'],
                    ['power_description','Facebook describes an optional pass-through module connecting the Open Rack 12V bus to the switch board; installed auction hardware must be checked.']],
      hyperscaler=[['Facebook / Meta','documented_platform_deployment','Facebook reported production use of Wedge 100 in its data centers on 2016-10-18; no surplus-unit provenance.']],
      evidence_blocks=[manufacturing,deployment,mounting]),
 dict(source=sid,key='wedge100-32x-edgecore',url=url,brand='Edge-Core',model='Wedge 100-32X',title='Edgecore Wedge 100-32X manufactured by Accton',kind='network switch',
      affiliations=[['Edgecore Networks','brand'],['Accton Technology','manufacturer']],
      facts=common+[['manufacturer_statement','Facebook identifies Accton Technology as the manufacturer of Edgecore Wedge 100-32X.'],
                    ['software_installation_description','2016 shipping description: diagnostics, OpenBMC and ONIE. This is not a current firmware-download or installed-software guarantee.']],
      hyperscaler=[['Facebook / Meta','documented_design_lineage','The commercial model complies with the Wedge 100 OCP specification. This article does not explicitly establish commercial Wedge 100-32X deployment at Facebook.']],
      evidence_blocks=[manufacturing,software])]
data=dict(reviewed_on='2026-09-27',sources=[dict(id=sid,url=url,tier='vendor',license='Factual statements paraphrased; original article rights retained',snapshot='raw/meta-wedge100/article.html',coverage=scope)],devices=devices)
assert not reviewed.exists()
reviewed.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
(raw/'review-notes.json').write_text(json.dumps(dict(source_url=url,html_sha256=sha,published_date=a.dates[0],reviewed_blocks={'manufacturing':manufacturing,'deployment':deployment,'rack_and_power':mounting,'shipping_software':software},scope=scope),indent=2)+'\n')
c=Catalog(root/'data/devices.sqlite')
assert c.db.execute('SELECT count(*) FROM sources WHERE id=?',(sid,)).fetchone()[0]==0
before={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ('devices','records','sources','affiliations','facts','hyperscaler_evidence')}
assert curated(c,reviewed)==2
after={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in before}
assert {k:after[k]-before[k] for k in before}==dict(devices=2,records=2,sources=1,affiliations=4,facts=12,hyperscaler_evidence=2)
c.db.close()
manifest_path=root/'data/import-manifest.json'; m=json.loads(manifest_path.read_text())
m['imports'].append(dict(source='curated',path=reviewed.name))
for p in (reviewed,html,raw/'fetch-report.json',raw/'review-notes.json'):
    m['files'][str(p.relative_to(root/'data'))]=file_sha256(p)
m['expected']['devices']=after['devices']; m['expected']['records']=after['records'];m['expected']['sources'][sid]=2
manifest_path.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n')
read_manifest(manifest_path)
audit=dict(before=before,after=after,database_sha256=file_sha256(root/'data/devices.sqlite'),manifest_jobs=len(m['imports']),manifest_files=len(m['files']),source_html_sha256=sha)
(root/'data/exports/meta-wedge100-import-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))

from pathlib import Path
import json,re,html,hashlib
from devicehunt.importers import Page
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans');raw=root/'data/raw/odm-primary-2026-10-04';body=(raw/'wiwynn-products.html').read_text();models={};cards=0
for match in re.finditer(r'<div class="back">\s*<div class="contents">(.*?)</div>',body,re.S):
 fragment=match.group(1);heading=re.search(r'<h4\b[^>]*>(.*?)</h4>',fragment,re.S)
 links=[(title,url) for title,url in Page(fragment).links if url.startswith('/products/')]
 if not heading or not links:continue
 back=html.unescape(re.sub('<[^>]+>','',heading.group(1))).strip()
 fronts=re.findall(r'<h4\b[^>]*>(.*?)</h4>',body[:match.start()],re.S);assert fronts
 model=html.unescape(re.sub('<[^>]+>','',fronts[-1])).strip();assert re.fullmatch(r'[A-Z]{2}\d{3,4}(?:G\d|[A-Z])?',model),model
 assert len(links)==1,links
 text=' '.join(html.unescape(re.sub('<[^>]+>',' ',fragment)).split()).removesuffix('See more').strip();cards+=1
 url='https://www.wiwynn.com'+links[0][1];old=models.get(model);row=(url,text,back)
 if old:assert old==row,(old,row)
 models[model]=row
assert len(models)==12 and cards==12,(cards,models)
source=dict(id='wiwynn-product-index',url='https://www.wiwynn.com/products',tier='vendor',license='Wiwynn public product facts; upstream presentation rights retained',snapshot='raw/odm-primary-2026-10-04/wiwynn-products.html',coverage='Original public product-index HTML captured 2026-10-04. Twelve explicit front-facing product-card headings, preserved as printed. EP100G2 has a conflicting reverse heading EP200G2; ST7000G4 links to an SV7000G4 slug. These discrepancies are flagged and no equivalence or corrected model is inferred. Exact printed models and linked pages only; linked details were not fetched. Brand/platform-vendor association, not legal manufacturer, factory, operator deployment, commercial ordering-code equivalence or complete historical product coverage.')
rows=[]
for model,(url,text,back) in sorted(models.items()):
 rows.append(dict(source=source['id'],key=model,brand='Wiwynn',model=model,title='Wiwynn '+model,kind='hardware platform',url=url,affiliations=[['Wiwynn','brand'],['Wiwynn','platform_vendor']],facts=[['index_url',source['url']],['product_card_description',text],['front_card_model',model],['reverse_card_heading',back],['card_heading_status','consistent' if back==model else 'Front/reverse card headings disagree; reverse text is not an alias or second model assertion.'],['source_scope','Original index card; linked detail not captured. No manufacturer or hyperscaler deployment inferred.'],['link_label_status','consistent' if url.rstrip('/').rsplit('/',1)[1]==model.lower() else 'Printed model and linked URL slug disagree; no alias or correction inferred.']]))
hyve_url='https://ir.tdsynnex.com/news/news-details/2023/Hyve-Solutions-Announces-Ultra-High-Density-Mass-Storage-Platform/default.aspx'
hyve=(raw/'hyve-hercules.html').read_text()
for term in ('Hyve Hercules 9120','Hyve Solutions Corporation','TD SYNNEX Corporation','October 17, 2023','96 hard drives per 5U system'):
 assert term in hyve,term
sources=[source,dict(id='hyve-hercules-2023',url=hyve_url,tier='vendor',license='TD SYNNEX public product facts; upstream presentation rights retained',snapshot='raw/odm-primary-2026-10-04/hyve-hercules.html',coverage='Original TD SYNNEX announcement dated 2023-10-17, captured 2026-10-04. Hyve product/platform association with a separately dated TD SYNNEX parent/subsidiary-portfolio role. No physical manufacturer, factory, named hyperscaler deployment, full ordering SKU, current availability, installed drive count or present parent ownership inferred. Announced drive/CPU support is a historical platform claim, not a specific auction-unit configuration.')]
rows.append(dict(source='hyve-hercules-2023',key='Hercules 9120',brand='Hyve',model='Hercules 9120',title='Hyve Hercules 9120',kind='storage platform',affiliations=[['Hyve Solutions Corporation','platform_vendor'],['Hyve','brand'],['TD SYNNEX Corporation','historical_subsidiary_portfolio']],facts=[['published_date','2023-10-17'],['chassis_height','5U'],['storage_bays','96 hard drives per 5U system (announced design capacity, not installed drives)'],['rack_configuration','Eight 5U chassis, 768 hard drives per rack as announced'],['host_interface','PCIe 5.0 support as announced'],['corporate_relationship','Release names Hyve Solutions Corporation as wholly owned subsidiary of TD SYNNEX Corporation on 2023-10-17; this is a dated parent association, not parent manufacturing.']]))
(root/'data/odm-products-2026-10-04.json').write_text(json.dumps(dict(sources=sources,devices=rows),indent=2)+'\n')
print('Prepared',len(rows),'source-qualified products:',', '.join(r['model'] for r in rows))

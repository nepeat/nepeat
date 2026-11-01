"""Preserve original PDFs and reviewed model/role facts; examples stay separate."""
import hashlib
import json
from pathlib import Path
from pypdf import PdfReader
ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT/'data'; RAW=DATA/'raw'; captures=RAW/'bytedance-inspur-2026-10-04'
def save(path,data):
    with path.open('x') as stream: stream.write(json.dumps(data,indent=2,ensure_ascii=False)+'\n')
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
bundle=json.loads((RAW/'reference-documents-with-odm-models-2026-10-04.json').read_text())
items=[('accton-b4010-compliance','https://www.accton.com.cn/wp-content/uploads/B4010-2302C014-CER_FCCISED.pdf',
        'BTL B4010 compliance statements; Accton applicant, brand N/A',2,'certification_body'),
       ('inspur-i24-whitepaper','https://www.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/08/2020081316525840458.pdf',
        'Inspur i24 Product Technical White Paper, revision 3.0, 2020-01',34,'vendor')]
for name,url,title,count,tier in items:
    pdf=RAW/(name+'-2026-10-04.pdf')
    with pdf.open('xb') as stream:stream.write((captures/(name+'.pdf')).read_bytes())
    pages=[dict(page=n,text=p.extract_text()) for n,p in enumerate(PdfReader(pdf).pages,1)]
    assert len(pages)==count
    textpath=pdf.with_suffix('.pages.json')
    save(textpath,dict(pdf_sha256=sha(pdf),extraction_method='pypdf embedded text; no OCR',pages=pages))
    bundle['documents'].append(dict(id=name,pdf=dict(path=pdf.name,sha256=sha(pdf)),
        text=dict(path=textpath.name,sha256=sha(textpath)),url=url,title=title,tier=tier,
        category='compliance statement' if name.startswith('accton') else 'vendor technical white paper',
        page_count=count,scope='Original public vendor-hosted PDF captured 2026-10-04. '
        'Named company roles and model strings require separate reviewed assertions. '
        'No FCC ID, ByteDance model equivalence, factory or hyperscaler deployment inferred.'))
save(RAW/'reference-documents-with-bytedance-inspur-2026-10-04.json',bundle)
sources=[dict(id='accton-b4010-compliance',url=items[0][1],tier='certification_body',
    license='BTL factual compliance statements; upstream rights retained',
    snapshot='raw/bytedance-inspur-2026-10-04/accton-b4010-compliance.pdf',
    coverage='Two original BTL statements hosted by Accton. Both name B4010, brand N/A, '
    'and Accton Technology Corporation as applicant. No manufacturer, FCC ID, ByteDance brand '
    'or deployment assertion. Report identifier is not an equipment authorization ID.'),
    dict(id='inspur-i24-whitepaper-2020',url=items[1][1],tier='vendor',
    license='Inspur factual product names; upstream rights retained',
    snapshot='raw/bytedance-inspur-2026-10-04/inspur-i24-whitepaper.pdf',
    coverage='Original public 34-page i24 technical white paper. Cover gives revision 3.0 and '
    '2020-01 release date; product headers and company copyright footer support the '
    'Inspur / Inspur Electronic Information Industry Co., Ltd. platform-vendor association. '
    'No physical manufacturer, installed configuration, alias expansion or operator deployment inferred.')]
devices=[dict(source=sources[0]['id'],key='B4010',brand='N/A',model='B4010',
    title='B4010 Ethernet Switch — BTL compliance statements, brand N/A',kind='switch',
    affiliations=[['Accton Technology Corporation','applicant'],['BTL INC.','certification_body']],
    facts=[['source_pages','1-2'],['brand_as_printed','N/A'],['report_number','BTL-FICE-1-2302C014'],
           ['fcc_standard','47 CFR Part 15 Subpart B; test statement, not FCC equipment authorization'],
           ['ised_standard','ICES-003 Issue 7:October 2020'],
           ['identity_limit','No ByteDance association in this document; cannot independently validate the ByteDance B4010 example.']]),
    dict(source=sources[1]['id'],key='i24',brand='Inspur',model='i24',title='Inspur Yingxin Server i24',
    kind='server platform',affiliations=[['Inspur','brand'],['Inspur Electronic Information Industry Co.,Ltd','platform_vendor']],
    facts=[['document_revision','3.0'],['document_release_date','2020-01'],
           ['identity_scope','i24 platform as printed on cover and page headers; no automatic M5/node-model alias.']])]
save(DATA/'bytedance-inspur-facts-2026-10-04.json',dict(sources=sources,devices=devices))
print('Prepared two reviewed model records and two additive PDF references.')

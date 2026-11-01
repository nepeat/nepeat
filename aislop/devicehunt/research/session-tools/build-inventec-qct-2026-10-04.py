"""Prepare reviewed product/portfolio facts and a complete additive PDF bundle."""
import json
from pathlib import Path
from pypdf import PdfReader
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
RAW = DATA / 'raw'
CAP = RAW / 'inventec-qct-2026-10-04'
def save(path, value):
    with path.open('x') as stream:
        stream.write(json.dumps(value, indent=2, ensure_ascii=False) + '\n')

urls = {row['path']: row['url'] for row in json.loads((CAP / 'fetch-report.json').read_text()) if row.get('path')}
bundle = json.loads((RAW / 'reference-documents-with-ingrasys-2026-10-04.json').read_text())
texts = {}
for name, title, count in (
    ('quanta-affiliates-2025', 'Quanta Computer summaries of affiliated companies 2025', 18),
    ('qct-wssd', 'QCT Windows Server Software Defined solutions (2018 footer)', 4),
    ('qct-qxvdi-2020', 'QCT QxStack QxVDI OA HC reference architecture version 2.0', 40),
):
    original = CAP / (name + '.pdf')
    pdf = RAW / (name + '-2026-10-04.pdf')
    with pdf.open('xb') as stream:
        stream.write(original.read_bytes())
    pages = [{'page': n, 'text': p.extract_text()} for n, p in enumerate(PdfReader(pdf).pages, 1)]
    assert len(pages) == count
    textpath = pdf.with_suffix('.pages.json')
    save(textpath, dict(pdf_sha256=file_sha256(pdf), extraction_method='pypdf embedded text; no OCR', pages=pages))
    texts[name] = pages
    bundle['documents'].append(dict(id=name,
        pdf=dict(path=pdf.name, sha256=file_sha256(pdf)), text=dict(path=textpath.name, sha256=file_sha256(textpath)),
        url=urls[original.name], title=title,
        category='corporate relationship report' if name == 'quanta-affiliates-2025' else 'vendor reference document',
        tier='vendor', page_count=count,
        scope='Original vendor-hosted PDF captured 2026-10-04. Embedded text only; no OCR. '
              'Quanta ownership chart page 1, WSSD tables pages 2-3, and QxVDI parent statement '
              'page 40 visually reviewed for selected facts. Corporate and product assertions '
              'are separate; no automatic manufacturer, FCC ID, hyperscaler deployment or '
              'auction configuration. Copyright, file metadata and URL dates are not release dates.'))
save(RAW / 'reference-documents-with-inventec-qct-2026-10-04.json', bundle)
assert 'The parent of QCT is Quanta Computer Inc.' in texts['qct-qxvdi-2020'][-1]['text']
assert 'QCT (Quanta Cloud Technology)' in texts['qct-qxvdi-2020'][-1]['text']

sources, records = [], []
def source(sid, filename, coverage):
    sources.append(dict(id=sid, url=urls[filename], tier='vendor',
        license='Public vendor product/corporate facts; original document rights retained',
        snapshot=str((CAP / filename).relative_to(DATA)), coverage=coverage))

for model, filename, physical, cpu, power_raw, power, management in (
    ('Horsea', 'inventec-horsea.html', '2U2P; W x H x D: 447.6 x 87 x 780 mm',
     'Two AMD EPYC 7003/7002 sockets; up to 280 W TDP per CPU',
     '1+1 冗余，800W/1300W/1600W/2000W (100-220VAC)白金',
     '1+1 redundant Platinum PSUs; alternatives 800/1300/1600/2000 W; printed input 100-220 VAC.',
     'IPMI 2.0 + KVM with dedicated LAN'),
    ('Seadra', 'inventec-seadra.html', '2U1N2P rackmount; W x H x D: 438 x 87.9 x 780 mm',
     'Two third-generation Intel Xeon Scalable sockets; up to 270 W TDP',
     '1+1 CRPS 电源 800W/1300W/1600W/ 2000W (110-220VAC), 白金',
     '1+1 CRPS Platinum PSUs; alternatives 800/1300/1600/2000 W; printed input 110-220 VAC.',
     'Dedicated 100M/1GbE Base-T management; ASPEED AST2500'),
):
    sid = 'inventec-' + model.lower() + '-page'
    source(sid, filename, 'Original Chinese product page captured 2026-10-04. Exact named platform '
        'and selected specification-table facts; English statements are translations, raw power '
        'text retained. Alternatives and SKU A/B/C are not commercial order-code aliases. '
        'No manufacturer, operator, installed auction-unit configuration or firmware-access claim.')
    body = (CAP / filename).read_text()
    for token in (model, '6056', '2000W'):
        assert token in body
    records.append(dict(source=sid, key=model, brand='Inventec', model=model,
        title='Inventec ' + model, kind='server platform',
        affiliations=[['Inventec', 'brand'], ['Inventec', 'platform_vendor']], facts=[
            ['source_observed_date', '2026-10-04'], ['source_language', 'Chinese; selected facts translated to English'],
            ['processor', cpu], ['physical_description', physical],
            ['power_description', power], ['power_statement_as_printed', power_raw],
            ['cooling_description', 'Six 6056 hot-swappable fans; N+1 redundancy.'],
            ['management', management], ['memory', '32 DDR4 DIMM slots; two DIMMs per channel'],
            ['configuration_limit', 'PSU capacities and chassis configurations are alternatives, not all installed together. No exact SKU A/B/C ordering-code mapping supplied.']]))

source('qct-wssd-models', 'qct-wssd.pdf',
    'Nine exact model names from original QCT-hosted WSSD datasheet, table headers on pages 2-3. '
    'QuantaGrid/QuantaPlex prefixes retained in the model field, QCT is the source brand. '
    'Footer copyright 2018, PDF metadata 2018-11-21, URL path 2020/11 and filename 2019 '
    'are recorded without choosing a product release date. Selected CPU/form-factor facts '
    'from visually reviewed columns describe WSSD configurations, not maximum platform '
    'capabilities or auction-unit contents. No manufacturer or Microsoft deployment inferred.')
models = [
    ('QuantaPlex T21P-4U', 2, '4U 1-node rackmount server', 'Intel Xeon E5-2600 v4 family'),
    ('QuantaGrid D51B-2U', 3, '2U 1-node rackmount server', 'Intel Xeon E5-2600 v4 family'),
    ('QuantaGrid D52BQ-2U', 3, '2U 1-node rackmount server', 'Intel Xeon Scalable; Silver, Gold and Platinum options'),
    ('QuantaPlex T41S-2U', 3, '2U 4-node rackmount server', 'Intel Xeon E5-2600 v4 family'),
    ('QuantaPlex T42S-2U', 3, '2U 4-node rackmount server', 'Intel Xeon Scalable; Silver, Gold and Platinum options'),
    ('QuantaGrid D52BV-2U', 3, '2U 1-node rackmount server', 'Intel Xeon Scalable; Silver, Gold and Platinum options'),
    ('QuantaGrid D51PH-1ULH', 3, '1U 1-node storage server', 'Intel Xeon E5-2600 v4 family'),
    ('QuantaGrid D52T-1ULH', 3, '1U 1-node storage server', 'Intel Xeon Scalable; Silver, Gold and Platinum options'),
    ('QuantaGrid S31A-1U', 3, '1U 1-node rackmount server', 'Intel Xeon E3-1200 v6 family'),
]
for model, page, physical, cpu in models:
    compact = ''.join(texts['qct-wssd'][page-1]['text'].split())
    assert ''.join(model.split()) in compact, model
    records.append(dict(source='qct-wssd-models', key=model, brand='QCT', model=model,
        title='QCT ' + model, kind='server platform',
        affiliations=[['QCT', 'brand'], ['QCT', 'platform_vendor']], facts=[
            ['source_pages', str(page)], ['physical_description', physical], ['processor', cpu],
            ['solution_context', 'Vendor Windows Server Software Defined configuration; compatibility/solution participation, not Microsoft-operated hardware.'],
            ['source_date_limit', 'Printed copyright 2018; PDF metadata 2018-11-21; URL path 2020/11; filename includes 2019. Product release date unassigned.'],
            ['identity_limit', 'Full printed QuantaGrid/QuantaPlex prefix retained; not automatically merged with a bare code under another brand.'],
            ['configuration_limit', 'WSSD solution configurations only. No full ordering code, PSU specification, firmware access or specific surplus-unit contents verified.']]))

source('qct-quanta-portfolio-2020', 'qct-qxvdi-2020.pdf',
    'Page 40 of the original QCT QxVDI reference architecture explicitly expands QCT as '
    'Quanta Cloud Technology and names Quanta Computer Inc. as parent; footer copyright '
    '2020-2021. Nine separately evidenced WSSD product models are joined as historical '
    'subsidiary portfolio associations only. This is an explicit cross-source inference, '
    'not a manufacturing assertion, current ownership guarantee, QCT global legal-name '
    'alias or named hyperscaler deployment. Both source URLs/hashes retained per record.')
for model, _, _, _ in models:
    records.append(dict(source='qct-quanta-portfolio-2020', key=model, brand='QCT', model=model,
        title='QCT ' + model, kind='server platform',
        affiliations=[['Quanta Computer Inc.', 'historical_subsidiary_portfolio']], facts=[
            ['association_method', 'Explicit cross-source inference: QCT product document plus separately dated QCT parent-company statement.'],
            ['parent_name_as_printed', 'Quanta Computer Inc.'], ['subsidiary_name_as_printed', 'QCT (Quanta Cloud Technology)'],
            ['relationship_source_pages', '40'], ['relationship_date_context', 'Footer copyright 2020-2021; not a verified ownership-start date'],
            ['corporate_source_url', urls['qct-qxvdi-2020.pdf']], ['corporate_source_sha256', file_sha256(CAP / 'qct-qxvdi-2020.pdf')],
            ['model_source_url', urls['qct-wssd.pdf']], ['model_source_sha256', file_sha256(CAP / 'qct-wssd.pdf')],
            ['association_limit', 'Historical portfolio association; no manufacturer or operator inferred.']]))
assert len(records) == 20 and len(sources) == 4
save(DATA / 'inventec-qct-facts-2026-10-04.json', dict(sources=sources, devices=records))
save(CAP / 'research-outcomes.json', dict(
    qct_press_page=dict(url='https://www.qct.io/zh-CN/news/index/PR/Server/QCT-to-open-new-European-Head-Office-in-Dusseldorf-moving-closer-to-the-European-customers-and-improving-IT-support/1/0',
        status=403, method='web open', note='Do not retry/bypass; original reference architecture independently supplies parent statement.'),
    bytedance=dict(models=['B4020', 'B4010'], result='No new primary vendor/model affiliation evidence found in this bounded search.',
        queries=['site.accton.com.cn "B4020"', '"B4020" "Accton"', '"B4010" "ByteDance"'],
        exclusions=['Trade-data previews: original shipment/brand provenance unverified; existing Eximpedia 402 not retried.',
                    'IMPERX B4020/ICL-B4020 are camera-model collisions, not ByteDance model evidence.']),
    inventec=dict(public_index_models=28, public_names_with_login_restricted_details=4,
        restricted_details_requested=False),
    quanta=dict(ownership_chart_reviewed_pages=[1], wssd_tables_reviewed_pages=[2, 3],
        parent_statement_reviewed_pages=[40], global_qct_alias_added=False)))
print('Prepared 20 model/portfolio assertions and three additive original PDFs (62 pages).')

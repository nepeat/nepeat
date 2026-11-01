"""One-time reviewed Ingrasys facts; original captures remain immutable."""
import json
from pathlib import Path
from pypdf import PdfReader
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
RAW = DATA / 'raw'
CAP = RAW / 'ingrasys-servers-2026-10-04'
def save(path, value):
    with path.open('x') as stream:
        stream.write(json.dumps(value, indent=2, ensure_ascii=False) + '\n')

# The storage download has an injected UTF-8 BOM. Archive all original bytes;
# the explicitly labelled PDF derivative removes only those three prefix bytes.
original = CAP / 'es2000-es2100.html'
body = original.read_bytes()
assert body.startswith(b'\xef\xbb\xbf%PDF-1.6')
pdf = RAW / 'ingrasys-es2000-es2100-bom-removed-2026-10-04.pdf'
with pdf.open('xb') as stream:
    stream.write(body[3:])
reader = PdfReader(pdf, strict=True)
assert len(reader.pages) == 2
pages = [{'page': n, 'text': page.extract_text()} for n, page in enumerate(reader.pages, 1)]
assert all(not p['text'].strip() for p in pages)
textpath = pdf.with_suffix('.pages.json')
save(textpath, dict(pdf_sha256=file_sha256(pdf), extraction_method='pypdf embedded text; no OCR', pages=pages))
storage_url = 'https://www.ingrasys.com/docs-detail/es2000-es2100/'
bundle = json.loads((RAW / 'reference-documents-with-bytedance-inspur-2026-10-04.json').read_text())
bundle['documents'].append(dict(id='ingrasys-es2000-es2100',
    pdf=dict(path=pdf.name, sha256=file_sha256(pdf)),
    text=dict(path=textpath.name, sha256=file_sha256(textpath)),
    url=storage_url, title='Ingrasys ES2000 / ES2100 storage datasheet (2024 footer)',
    category='vendor datasheet', tier='vendor', page_count=2,
    scope='Image-only PDF. Archived derivative removes exactly three leading UTF-8 BOM bytes from '
          'original HTTP body raw/ingrasys-servers-2026-10-04/es2000-es2100.html, SHA-256 '
          + file_sha256(original) + '. Original unchanged; no other repair. Both pages visually '
          'reviewed, model-specific facts stored separately. No embedded text, no OCR. '
          'DOCS index date Jun/2024 and printed copyright 2024 do not prove release date.'))
save(RAW / 'reference-documents-with-ingrasys-2026-10-04.json', bundle)

index = RAW / 'odm-models-2026-10-04/ingrasys-docs.html'
assert 'SV1120/SV1121/SV2121/SV2221' in index.read_text()
about = (CAP / 'about.html').read_text()
assert "Foxconn Technology Group's subsidiary" in about
assert 'SV2221A' in (CAP / 'amd-epyc.html').read_text()
assert 'SV2221I' in (CAP / 'intel-xeon.html').read_text()
assert 'ES2100' in (CAP / 'nvme-of-ebof.html').read_text()

sources = []
devices = []
def source(sid, url, snapshot, coverage):
    sources.append(dict(id=sid, url=url, tier='vendor',
        license='Ingrasys factual product/corporate information; upstream rights retained',
        snapshot=str(snapshot.relative_to(DATA)), coverage=coverage))
def product(sid, model, kind, facts):
    devices.append(dict(source=sid, key=model, brand='Ingrasys', model=model,
        title='Ingrasys ' + model, kind=kind,
        affiliations=[['Ingrasys', 'brand'], ['Ingrasys', 'platform_vendor']],
        facts=facts))

source('ingrasys-server-docs-index', 'https://www.ingrasys.com/docs/', index,
    'Four exact unsuffixed server names split from the DOCS heading SV1120/SV1121/SV2121/SV2221, '
    'dated Oct/2023 in the index captured 2026-10-04. Index evidence only. Original linked '
    'catalog HTTP 200 body is malformed (BOM, repeated PDF header, truncated tail); no specs '
    'or document pages imported from it. No equivalence to SV2221A or SV2221I inferred.')
for model in ('SV1120', 'SV1121', 'SV2121', 'SV2221'):
    product('ingrasys-server-docs-index', model, 'server platform', [
        ['source_scope', 'DOCS index model heading only; malformed linked catalog remains an unresolved document gap.'],
        ['index_date', 'Oct/2023; not established as product release date'],
        ['group_heading', 'SV1120/SV1121/SV2121/SV2221'],
        ['document_url', 'https://www.ingrasys.com/docs-detail/sv1120-sv1121-sv2121-sv2221/'],
        ['identity_limit', 'Unsuffixed name as printed; no automatic alias to suffixed variants.']])

for sid, filename, url, model, processor, memory, cooling in (
    ('ingrasys-sv2221a-page', 'amd-epyc.html',
     'https://www.ingrasys.com/solutions/amd/amd-epyc-processor-solution/',
     'SV2221A', 'Two fifth-generation AMD EPYC server processors',
     '24 DDR5 RDIMM slots; 12 channels; up to 6000 MT/s at 1DPC', 'Liquid cooling'),
    ('ingrasys-sv2221i-page', 'intel-xeon.html',
     'https://www.ingrasys.com/solutions/intel/intel-xeon-processor-solution/',
     'SV2221I', 'Two Intel Xeon 6 processors',
     '32 DDR5 RDIMM slots; 8 channels; up to 6400 MT/s at 1DPC and 5200 MT/s at 2DPC',
     'Air or liquid cooling'),
):
    source(sid, url, CAP / filename,
        'Original current Ingrasys product page captured 2026-10-04. Exact suffixed model '
        'and vendor-reported platform specifications only; not a release date, installed '
        'auction configuration, legal manufacturer, FCC ID or named operator deployment.')
    product(sid, model, 'server platform', [
        ['source_observed_date', '2026-10-04'], ['processor', processor], ['memory', memory],
        ['physical_description', '2 OU'], ['cooling_description', cooling],
        ['power_description', 'Centralized 48 V busbar with power distribution board (PDB).'],
        ['expansion', 'Two PCIe 5.0 x16 FHHL slots; eight PCIe 5.0 x16 LP/HHHL slots with riser cards.'],
        ['identity_limit', 'Suffix retained; no equivalence to unsuffixed SV2221 asserted.']])

source('ingrasys-es2000-es2100-datasheet', storage_url, original,
    'Original two-page image-only PDF HTTP body captured 2026-10-04; extension .html reflects '
    'download URL assumption, actual body is PDF with leading UTF-8 BOM. Both rendered pages '
    'visually reviewed after removing only BOM in a separately hashed derivative. Model facts '
    'manually transcribed from page 2, not OCR/embedded text. Footer copyright 2024. '
    'Shared specs kept separate from explicitly model-specific switch/port rows. '
    'Platform brand only; no FCC ID, manufacturer, operator or auction-unit claim.')
for model, switch, ports in (
    ('ES2000', 'Marvell 98EX5630', '12 x 200GbE QSFP56'),
    ('ES2100', 'NVIDIA Spectrum-2', '24 x 200GbE QSFP56'),
):
    product('ingrasys-es2000-es2100-datasheet', model, 'NVMe-oF storage system', [
        ['source_pages', '1-2; detailed model-specific specifications on page 2'],
        ['extraction_method', 'Manual visual transcription; image-only PDF; no embedded text or OCR'],
        ['document_date', 'Copyright 2024; DOCS index Jun/2024, not proven release date'],
        ['switch_asic', switch], ['ports', ports],
        ['storage', '24 hot-swap U.2/E3.S NVMe SSD bays; Western Digital RapidFlex A2000 NVMe-oF controller per drive carrier.'],
        ['boot_storage', 'One M.2 2280 boot device (120 GB) on each switch module.'],
        ['management', 'Intel Atom C3538; 8 GB DDR4-2133 RDIMM'],
        ['software_installation_description', 'Printed NOS row: ONIE/SONiC; no firmware availability or auction-unit access verified.'],
        ['physical_description', '2U rackmount; printed H x W x D: 87.1 x 447.2 x 597.0 mm; weight 29 kg with SSDs.'],
        ['power_description', 'AC input 100-240 V, 50-60 Hz; 1+1 redundant 1600 W Platinum PSUs. Output derating not specified.'],
        ['cooling_description', 'Six 40 x 56 mm fans per switch module; N+1 cooling redundancy.'],
        ['regulatory_description', 'Printed certification labels UL/CB/CE/BSMI; no certificate numbers or FCC ID provided.']])

source('ingrasys-es2100-page', 'https://www.ingrasys.com/solutions/15/nvme-of_ebof_solution/',
    CAP / 'nvme-of-ebof.html',
    'Original product page captured 2026-10-04, independently preserved alongside the 2024 '
    'shared ES2000/ES2100 datasheet. ES2100 only. Platform specs do not assert deployment, '
    'manufacturer or configurations of specific surplus units.')
product('ingrasys-es2100-page', 'ES2100', 'NVMe-oF storage system', [
    ['source_observed_date', '2026-10-04'], ['switch_asic', 'NVIDIA Spectrum-2'],
    ['storage', '24 U.2/E3.S NVMe SSD bays'], ['ports', '24 x 200GbE QSFP56'],
    ['power_description', '1+1 redundant 1600 W Platinum power supplies.'],
    ['physical_description', '2 RU'], ['cooling_description', 'Air cooling'],
    ['serviceability', 'Two hot-swappable redundant switch modules.']])

# Explicit cross-source portfolio join, never a global Ingrasys/Foxconn alias.
sid = 'ingrasys-foxconn-portfolio-2026-10-04'
about_url = 'https://www.ingrasys.com/company/about/'
source(sid, about_url, CAP / 'about.html',
    'Reviewed cross-source association as observed 2026-10-04: current Ingrasys company page '
    'calls Ingrasys a subsidiary of Foxconn Technology Group; separate vendor model sources '
    'identify each listed Ingrasys platform. Foxconn is the canonical portfolio search label, '
    'with raw parent name retained in facts. The company page does not name these models. '
    'This is a parent-company portfolio inference only, not a manufacturer, factory, '
    'operator, historical ownership-start date or product-release claim. Ingrasys is not '
    'added to Foxconn global company aliases. Product and corporate hashes retained per record.')
for model in dict.fromkeys(row['model'] for row in devices):
    row = next(row for row in devices if row['model'] == model)
    model_source = next(s for s in sources if s['id'] == row['source'])
    devices.append(dict(source=sid, key=model, brand='Ingrasys', model=model,
        title=row['title'], kind=row['kind'], affiliations=[['Foxconn', 'parent_company_portfolio']],
        facts=[['association_method', 'Cross-source inference: Ingrasys model evidence plus separately documented parent relationship.'],
            ['relationship_observed_date', '2026-10-04'],
            ['parent_name_as_printed', 'Foxconn Technology Group'],
            ['subsidiary_name_as_printed', 'Ingrasys'],
            ['corporate_source_url', about_url], ['corporate_source_sha256', file_sha256(CAP / 'about.html')],
            ['model_source_id', row['source']], ['model_source_url', model_source['url']],
            ['model_source_sha256', file_sha256(DATA / model_source['snapshot'])],
            ['association_limit', 'Portfolio association only; not manufacturer or hyperscaler deployment.']]))
assert len(devices) == 17 and len({r['model'] for r in devices}) == 8
save(DATA / 'ingrasys-facts-2026-10-04.json', dict(sources=sources, devices=devices))
save(CAP / 'format-review.json', dict(
    storage=dict(original_sha256=file_sha256(original), derivative_sha256=file_sha256(pdf),
        transform='Remove only three prefix bytes EF BB BF; all remaining bytes identical',
        pages=2, pages_with_embedded_text=0, visual_review='Both pages reviewed',
        facts_method='Manual visual transcription from rendered page 2'),
    server=dict(path='sv1120-sv1121-sv2121-sv2221.html',
        sha256=file_sha256(CAP / 'sv1120-sv1121-sv2121-sv2221.html'),
        result='Original HTTP 200 response is malformed; pypdf cannot read /Pages',
        diagnostics='BOM prefix, PDF headers at byte 3 and 4128654, truncated %%EO tail',
        archive_imported=False, specifications_imported=False),
    foxconn_press_refetch=dict(url='https://www.foxconn.com/en-us/press-center/press-releases/latest-news/1172',
        status=403, method='web open', note='Earlier original snapshot retained; new web check denied. Do not retry.')))
print('Prepared 8 identities / 17 assertions and one explicitly labelled two-page PDF derivative.')

"""Build reviewed facts and a complete additive reference-PDF bundle once."""
import hashlib
import json
from pathlib import Path
from pypdf import PdfReader

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
RAW = DATA / 'raw'
CAPTURES = RAW / 'odm-models-2026-10-04'
def save(path, value):
    with path.open('x') as stream:
        stream.write(json.dumps(value, indent=2, ensure_ascii=False) + '\n')
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

wiwynn_url = 'https://www.wiwynn.com/hubfs/Datasheet%20-%20SV7100G4_210917_internal.pdf'
jade_url = 'https://cdn.prod.website-files.com/697d7e6ffbb0348d6224a85d/69f8ec237474bffbedb6ffcd_Hyve%20-%20Jade_RGB_Rev1_050126.pdf'
assert jade_url in (CAPTURES / 'hyve-datasheets.html').read_text()
assert 'Datasheet%20-%20SV7100G4_210917_internal.pdf' in (CAPTURES / 'wiwynn-sv7100g4.html').read_text()
bundle = json.loads((RAW / 'reference-documents-bundle.json').read_text())
texts = {}
for name, url, title, count in (
    ('wiwynn-sv7100g4', wiwynn_url, 'Wiwynn SV7100G4 and SV7100G4-C datasheet v1.0', 2),
    ('hyve-jade-2026', jade_url, 'Hyve Jade datasheet May 2026 Rev0.050126', 1),
):
    origin = CAPTURES / (name + '.pdf')
    pdf = RAW / ('odm-' + name + '-2026-10-04.pdf')
    with pdf.open('xb') as stream:
        stream.write(origin.read_bytes())
    pages = [{'page': n, 'text': page.extract_text()} for n, page in enumerate(PdfReader(pdf).pages, 1)]
    assert len(pages) == count
    text_path = pdf.with_suffix('.pages.json')
    save(text_path, dict(pdf_sha256=sha(pdf), extraction_method='pypdf embedded text; no OCR', pages=pages))
    texts[name] = '\n'.join(page['text'] for page in pages)
    bundle['documents'].append(dict(id='odm-' + name, pdf=dict(path=pdf.name, sha256=sha(pdf)),
        text=dict(path=text_path.name, sha256=sha(text_path)), url=url, title=title,
        category='vendor datasheet', tier='vendor', page_count=count,
        scope='Original vendor-linked PDF captured 2026-10-04; printed model names and specifications only. '
              'No FCC identifier, legal manufacturer, operator deployment or installed auction-unit configuration assigned. '
              'For Wiwynn, embedded glyph artifacts are retained and blade/sled/chassis labels require layout review.'))
save(RAW / 'reference-documents-with-odm-models-2026-10-04.json', bundle)
for term in ('SV7100G4-C', '12.5V DC bus bar', 'AST2520', '100GbE'):
    assert term in texts['wiwynn-sv7100g4'], term
for term in ('Hyve Jade', 'BCM78900', '3200W', 'Front-to-Rear', 'TD SYNNEX Corporation'):
    assert term in texts['hyve-jade-2026'], term

sources = [dict(id='wiwynn-sv7100g4-datasheet', url=wiwynn_url, tier='vendor',
    license='Wiwynn factual product specifications; upstream document rights retained',
    snapshot='raw/odm-models-2026-10-04/wiwynn-sv7100g4.pdf',
    coverage='Original two-page v1.0 PDF, captured 2026-10-04. Page 1 names platform SV7100G4; '
             'page 2 labels its blade SV7100G4-C. Separate identities, not aliases. '
             'Blade/sled/chassis wording and units retained; no independent correction of specifications. '
             'The product page lists a 2021/09/17 datasheet date, while the PDF footer says copyright 2015; '
             'neither establishes platform release date. No legal manufacturer or named operator deployment.'),
    dict(id='hyve-jade-datasheet-2026', url=jade_url, tier='vendor',
    license='Hyve factual product specifications; upstream document rights retained',
    snapshot='raw/odm-models-2026-10-04/hyve-jade-2026.pdf',
    coverage='Original one-page May 2026 Rev0.050126 datasheet linked from the Hyve datasheets page, '
             'captured 2026-10-04. Named product Jade, no numeric commercial order code provided. '
             'TD SYNNEX is a dated parent-portfolio association from the footer, not manufacturer. '
             'No specific hyperscaler, FCC ID, auction-unit configuration or current market availability inferred.')]
records = [dict(source='wiwynn-sv7100g4-datasheet', key='SV7100G4', brand='Wiwynn', model='SV7100G4',
    title='Wiwynn SV7100G4 multi-node server platform', kind='server platform',
    affiliations=[['Wiwynn', 'brand'], ['Wiwynn', 'platform_vendor']], facts=[
        ['source_pages', '1-2'], ['architecture', 'OCP Yosemite V3 / Open Rack V2'],
        ['named_blade', 'SV7100G4-C; separate blade identity, not platform alias'],
        ['physical_description', 'Page 1: 4OU cubby; three sleds, up to four blades per sled.'],
        ['power_description', 'Page 2 chassis specification: centralized 12.5 V DC busbar.'],
        ['cooling_description', 'Page 2 chassis specification: four 8030 fan modules per sled, tolerating one fan failure.'],
        ['specification_limit', 'Page 1 says four 1U blades; page 2 says four server blades in a 1OU sled. Units retained without reconciliation.']]),
    dict(source='wiwynn-sv7100g4-datasheet', key='SV7100G4-C', brand='Wiwynn', model='SV7100G4-C',
    title='Wiwynn SV7100G4-C server blade', kind='server blade',
    affiliations=[['Wiwynn', 'brand'], ['Wiwynn', 'platform_vendor']], facts=[
        ['source_pages', '2'], ['platform_name', 'SV7100G4; platform relation, not alias'],
        ['physical_description', 'Blade table: 1OU, one-third width of OCP vCubby chassis.'],
        ['processor', 'One third-generation Intel Xeon Scalable socket per blade'],
        ['storage', 'One M.2 SSD, 2280 or 22110'],
        ['networking', '100GbE multi-host OCP 3.0 NIC shared by four nodes'],
        ['management', 'ASPEED AST2520; OpenBMC-based SSH'],
        ['power_description', 'Blade-table power-consumption row: 130-165 W (Max). Chassis supply listed separately as centralized 12.5 V DC busbar.'],
        ['specification_limit', 'Printed table dimensions/weight are not normalized into blade shipping dimensions; mechanical scope needs confirmation.']]),
    dict(source='hyve-jade-datasheet-2026', key='Jade', brand='Hyve', model='Jade',
    title='Hyve Jade custom AI network switch', kind='network switch',
    affiliations=[['Hyve', 'brand'], ['Hyve Solutions Corporation', 'platform_vendor'],
                  ['TD SYNNEX Corporation', 'historical_subsidiary_portfolio']], facts=[
        ['source_pages', '1'], ['datasheet_date', 'May 2026'], ['document_revision', 'Rev0.050126'],
        ['ports', '64 x 800GbE OSFP'], ['switch_asic', 'Broadcom BCM78900 (Tomahawk 5)'],
        ['switching_capacity', '51.2 Tbps'], ['physical_description', '2RU; 438 x 650 x 87.3 mm; 23 kg.'],
        ['power_description', 'Two 3200 W hot-swappable PSUs, 1+1 redundancy; C20 connector. Printed input range 200-240 V; output table separately lists 2436 W at 180-229 Vac and 3168 W at 230-264 Vac. Ranges preserved, not reconciled.'],
        ['cooling_description', 'Air cooling, front-to-rear; four Sunon fans, 3+1 redundancy, hot-swappable.'],
        ['software_installation_description', 'ONIE installer; vendor states SONiC compatibility.'],
        ['corporate_relationship', 'May 2026 footer names Hyve Solutions Corporation as wholly owned subsidiary of TD SYNNEX Corporation.'],
        ['identification_limit', 'Jade is the printed product name; no numeric ordering code or FCC ID provided.']])]
save(DATA / 'odm-model-facts-2026-10-04.json', dict(sources=sources, devices=records))
print('Prepared three product records and two additive PDF references; original snapshots retained.')

"""Build reviewed Inspur hardware facts and the complete reference-PDF collection."""
import json
from pathlib import Path
from pypdf import PdfReader
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
RAW = DATA / 'raw'
CAP = RAW / 'inspur-rail-2026-10-04'

def save(path, data):
    with path.open('x') as stream:
        stream.write(json.dumps(data, indent=2, ensure_ascii=False) + '\n')

fetches = json.loads((CAP/'fetch-report.json').read_text())
assert len(fetches) == 4 and all(r['curl_exit'] == 0 for r in fetches)
urls = {r['path']: r['url'] for r in fetches}
original = CAP/'inspur-incloud-rail-compatibility.pdf'
pdf = RAW/'inspur-incloud-rail-compatibility-2026-10-04.pdf'
with pdf.open('xb') as stream:
    stream.write(original.read_bytes())
pages = [dict(page=n, text=p.extract_text() or '') for n,p in enumerate(PdfReader(pdf).pages, 1)]
assert len(pages) == 64
texts = pdf.with_suffix('.pages.json')
save(texts, dict(pdf_sha256=file_sha256(pdf), extraction_method='pypdf embedded text; no OCR', pages=pages))
bundle = json.loads((RAW/'reference-documents-with-storage-inspur-2026-10-04.json').read_text())
bundle['documents'].append(dict(id='inspur-incloud-rail-compatibility',
    pdf=dict(path=pdf.name, sha256=file_sha256(pdf)), text=dict(path=texts.name, sha256=file_sha256(texts)),
    url=urls[original.name], title='Inspur InCloud Rail compatibility list, printed December 2023',
    category='vendor reference document', tier='vendor', page_count=64,
    scope='Original public Inspur compatibility list; cover dated December 2023, PDF metadata modified March 2024. '
          'Full text includes third-party parts and software, which are not automatically Inspur hardware. '
          'Reviewed hardware tables on PDF pages 9, 12, 17, 20 and 51 retain source-qualified brand/platform facts. '
          'Appliance and server-node model columns are distinct, not aliases. No manufacturer or operator claim.'))
save(RAW/'reference-documents-with-inspur-rail-2026-10-04.json', bundle)

sources, devices = [], []
def source(sid, filename, coverage, url=None):
    path = CAP/filename if url is None else DATA/filename
    sources.append(dict(id=sid, url=url or urls[filename], tier='vendor',
        license='Public vendor product facts; original document rights retained',
        snapshot=str(path.relative_to(DATA)), coverage=coverage))

def device(sid, model, kind, facts):
    devices.append(dict(source=sid, key=model, brand='Inspur', model=model,
        title='Inspur '+model, kind=kind,
        affiliations=[['Inspur','brand'],['Inspur','platform_vendor']],
        facts=[['source_observed_date','2026-10-04'], *facts]))

rail = 'inspur-incloud-rail-hardware-2023'
source(rail, original.name,
    'Selected original compatibility-table rows: nine appliance/node pairs, four SSDs, five RAID cards, '
    'five switches and two FPGA cards. Distinct names remain separate identities. Brand/platform '
    'association only; the vendor column is not a physical-manufacture assertion. Third-party '
    'components and software are excluded from curated devices. Historical compatibility and firmware '
    'rows are source claims, not current support or a recommendation to flash an auction unit.')

def rail_device(model, kind, page, facts):
    assert model in pages[page-1]['text'], model
    device(rail, model, kind, [
        ['document_date_as_printed','2023-12'],
        ['document_date_context','Cover December 2023; PDF modification metadata March 2024; not a product release date'],
        ['source_pages',str(page)+' (PDF page number)'],
        ['compatibility_context','Listed in the archived InCloud Rail compatibility list; historical source claim, not current support or hyperscaler deployment'],
        *facts])

pairs = [
    ('IR5280M7','NF5280M7','M7'), ('IR8480M7','NF8480M7','M7'),
    ('IR5280M6','NF5280M6','M6'), ('IR5266M6','NF5266M6','M6'),
    ('IR8480M6','NF8480M6','M6'), ('IR5468M6','NF5468M6','M6'),
    ('600S-M5','NE5260M5','M5'), ('4010S-M5','NF8480M5','M5'), ('1010S-M5','NF5280M5','M5'),
]
for appliance, node, generation in pairs:
    rail_device(appliance, 'hyperconverged appliance', 9, [
        ['model_generation_as_printed',generation], ['server_node_model',node],
        ['identity_scope','Hyperconverged model column; the paired server-node name is not an alias']])
    rail_device(node, 'server', 9, [
        ['model_generation_as_printed',generation], ['hyperconverged_appliance_model',appliance],
        ['identity_scope','Server-node model column; the paired appliance name is not an alias']])

for model, firmware, note in (
    ('NS8600G1','HUT600D7',''), ('NS8500G2','MM5MA027','Intel+HG versions as printed'),
    ('NS8500G1','HUT600D7',''), ('NS6510','',''),
):
    facts = [['interface_as_printed','U.2'],['vendor_as_printed','Inspur']]
    if firmware: facts.append(['recommended_firmware_as_printed',firmware])
    if note: facts.append(['compatibility_note',note])
    rail_device(model, 'SSD', 12, facts)

for model, firmware, note in (
    ('3108_2GB','4.680.00-8573',''), ('3108_4GB','4.680.00-8527',''),
    ('3008IT','16.00.18.00','Intel and Hygon ICR versions'),
    ('3008IMR','4.680.01-8582','Intel and Hygon ICR versions'),
    ('PM8204_2GB','5.92','Intel and Hygon ICR versions'),
):
    facts = [['vendor_section_as_printed','7.1 Inspur'],['recommended_firmware_as_printed',firmware]]
    if note: facts.append(['compatibility_note',note])
    rail_device(model, 'storage controller', 17, facts)

for model, speed, note in (
    ('SC5630EL','25G','Deep management integration as printed'),
    ('CN9008-48YC-S','25G',''), ('S6550V2-48TQ-AC/D','1G','Grouped AC/D spelling retained; no suffix expansion'),
    ('S6820-24XQ-E','10G','Deep management integration as printed'),
    ('CN61108PC-V-H','10G','Deep management integration as printed'),
):
    facts = [['vendor_as_printed','Inspur'],['switch_type_as_printed',speed]]
    if note: facts.append(['compatibility_note',note])
    rail_device(model, 'switch', 20, facts)
for model in ('F10A','F37X'):
    rail_device(model, 'FPGA accelerator', 51, [['vendor_as_printed','Inspur']])

sid = 'inspur-ne5260m5-launch-2019'
source(sid, 'inspur-ne5260m5-launch.html',
    'Original Inspur 2019-02-27 launch article describing the 2019-02-25 event and NE5260M5. '
    'Brand/platform specifications only. OTII industry collaboration is not deployment evidence. '
    'Later URL migration and site footer dates are not article dates.')
device(sid, 'NE5260M5', 'edge server', [
    ['article_date','2019-02-27'], ['launch_event_date','2019-02-25'],
    ['physical_description','2U, 19-inch width, 430 mm chassis depth; wall-mount design described'],
    ['processor','Two forthcoming Intel scalable processors, as described in the 2019 launch article; exact SKU unasserted'],
    ['memory','16 DIMM slots'], ['expansion','Six PCI-E slots'],
    ['drive_bays','Six 2.5-inch drives and two M.2 SSDs'],
    ['design_context','OTII edge-server design; named telecom collaboration is not operator deployment'],
    ['configuration_limit','Historical platform options, not actual auction-unit contents or current firmware access']])

sid = 'inspur-i24-node-relationship-2020'
source(sid, 'raw/bytedance-inspur-2026-10-04/inspur-i24-whitepaper.pdf',
    'Additional reviewed pages of the existing original i24 revision 3.0 white paper. '
    'PDF page 5 explicitly separates the i24 chassis and NS5162M5 nodes; page 8 describes '
    'node architecture. Existing i24 statements are retained independently. No node/chassis alias, '
    'manufacturer, electrical interchangeability or operator inference.',
    'https://www.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/08/2020081316525840458.pdf')
common = [['document_revision','3.0'], ['document_release_date','2020-01'], ['source_pages','5, 8 (PDF page numbers)']]
device(sid, 'i24', 'server platform', [*common,
    ['physical_description','2U chassis supports four NS5162M5 compute nodes'],
    ['compute_node_model','NS5162M5'],
    ['management','CMC and BMC management'],
    ['power_description','Nodes share chassis power supplies and cooling through the side-card connection'],
    ['identity_scope','Chassis/platform identity; NS5162M5 is a separate compute-node model']])
device(sid, 'NS5162M5', 'compute node', [*common,
    ['chassis_model','i24'], ['processor','Two Intel Xeon Scalable processors per node'],
    ['memory','Up to 16 DDR4 DIMMs per node'],
    ['expansion','Page 8 describes three PCIe x16 slots and OCP/PHY connectivity; configuration-dependent'],
    ['power_description','Uses the i24 chassis shared power and cooling via a side card'],
    ['identity_scope','Compute-node identity; not an alias for the i24 chassis or an independent rack server']])

assert len(devices) == 37 and len(sources) == 3
assert len({d['model'] for d in devices}) == 36
save(DATA/'inspur-rail-facts-2026-10-04.json', dict(sources=sources, devices=devices))
save(CAP/'research-outcomes.json', dict(observed_at='2026-10-04', successful_original_fetches=4,
    added_pdf_pages=64, curated_records=37, paired_appliance_node_rows=[dict(appliance=a,node=n,generation=g) for a,n,g in pairs],
    exclusions=['Third-party Intel/LSI/Ruijie components', 'Guest OS and application names',
                'Network card tables pending merged-cell/name review', 'No automatic Inspur support-feed conversion'],
    inventec_review='Pages 2 and 3 each contain ten public download rows. Models and document titles can differ; '
                    'NX-H1313 row points at an NX-H1331 title. K905G6_G6P stays grouped. '
                    'Artemis II Rack and X680m accessories are follow-up leads; no new Inventec assertions in this batch.',
    access_scope='Public GETs only; no gated NX product details, serial/configuration, warranty, activation or contact requests.',
    bytedance_examples='B4020/B4010 still lack qualifying primary ByteDance affiliation evidence.'))
print('Prepared 37 source records and one original 64-page PDF; no alias or manufacturer assertions.')

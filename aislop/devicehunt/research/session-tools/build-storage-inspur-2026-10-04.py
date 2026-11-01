"""Prepare original PDF archive, raw support feed and three reviewed platform records."""
import json
from pathlib import Path
from pypdf import PdfReader
from devicehunt.rebuild import file_sha256
from devicehunt.inspur_support import API_URL, INDEX_URL

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
RAW = DATA / 'raw'
CAP = RAW / 'storage-inspur-2026-10-04'

def save(path, value):
    with path.open('x') as stream:
        stream.write(json.dumps(value, indent=2, ensure_ascii=False) + '\n')

fetches = json.loads((CAP / 'fetch-report.json').read_text())
urls = {r['path']: r['url'] for r in fetches if r.get('path')}
assert all(r['curl_exit'] == 0 for r in fetches)
rows = json.loads((CAP / 'inspur-product-names.json').read_text())
assert len(rows) == 1064 and len({r['typeTreeCname'] for r in rows}) == 1055
save(CAP / 'inspur-support-bundle.json', dict(schema_version=1, source_url=API_URL,
    index_url=INDEX_URL, expected_rows=len(rows),
    index=dict(path='inspur-download-index.html', sha256=file_sha256(CAP/'inspur-download-index.html')),
    products=dict(path='inspur-product-names.json', sha256=file_sha256(CAP/'inspur-product-names.json'))))

bundle = json.loads((RAW / 'reference-documents-with-inventec-qct-2026-10-04.json').read_text())
for name, title, count in (
    ('inspur-nf5280m5-whitepaper', 'Inspur NF5280M5 product technical white paper revision 5.0, 2019-12-22', 47),
    ('inventec-e850g4-brief', 'Inventec E850G4 MEC Server product brief, copyright 2021', 2),
):
    original = CAP / (name + '.pdf')
    pdf = RAW / (name + '-2026-10-04.pdf')
    with pdf.open('xb') as stream: stream.write(original.read_bytes())
    pages = [dict(page=n, text=p.extract_text() or '') for n, p in enumerate(PdfReader(pdf).pages, 1)]
    assert len(pages) == count
    text = pdf.with_suffix('.pages.json')
    save(text, dict(pdf_sha256=file_sha256(pdf), extraction_method='pypdf embedded text; no OCR', pages=pages))
    bundle['documents'].append(dict(id=name, pdf=dict(path=pdf.name, sha256=file_sha256(pdf)),
        text=dict(path=text.name, sha256=file_sha256(text)), url=urls[original.name], title=title,
        category='vendor reference document', tier='vendor', page_count=count,
        scope='Original publicly hosted vendor PDF captured 2026-10-04. Full embedded text retained. '
              'Selected platform facts only; no manufacturer, deployment or installed-unit inference. '
              'Printed revision, copyright, metadata and upload dates remain distinct. '
              'NF5280M5 power text contains apparent errors in some titanium/DC lines; no correction '
              'or equivalent PSU model inferred. Selected power table visually reviewed.'))
save(RAW / 'reference-documents-with-storage-inspur-2026-10-04.json', bundle)

sources, devices = [], []
def add(sid, filename, brand, model, kind, facts, coverage):
    sources.append(dict(id=sid, url=urls[filename], tier='vendor',
        license='Public vendor product facts; original document rights retained',
        snapshot=str((CAP/filename).relative_to(DATA)), coverage=coverage))
    devices.append(dict(source=sid, key=model, brand=brand, model=model,
        title=brand+' '+model, kind=kind, affiliations=[[brand,'brand'],[brand,'platform_vendor']],
        facts=[['source_observed_date','2026-10-04'], *facts]))

add('inventec-metagross-page', 'inventec-metagross.html', 'Inventec', 'Metagross', 'storage enclosure', [
    ['physical_description', '2U JBOD; D 925 x W 447 x H 86.8 mm'],
    ['drive_bays', '42 hot-swappable 3.5-inch SATA drives'],
    ['power_description', 'Two 800 W power supplies; 1+1 redundancy. Input voltage not printed.'],
    ['cooling_description', 'Four 8056 hot-swappable dual-rotor fans; N+1 redundancy'],
    ['management', 'RJ45 management NIC, separate RJ45 UART, mini USB I2C; BMC health monitoring'],
    ['storage_connectivity', 'Two Mini SAS HD x4 connectors'],
    ['configuration_limit', 'Platform capabilities; auction-unit drive/PSU contents and firmware access unverified.'],
], 'Original named Inventec product page and specification table. Exact Metagross identity; '
   'brand/platform evidence only. No manufacturer, installed drives or operator claim.')

add('inventec-e850g4-brief-2021', 'inventec-e850g4-brief.pdf', 'Inventec', 'E850G4', 'edge server', [
    ['source_pages', '1-2'], ['source_date_context', 'Copyright 2021; PDF metadata 2021-03-16; no product release date inferred'],
    ['physical_description', '2U1N rackmount; W 447 x H 87 x D 450 mm'],
    ['processor', 'Dual-socket second-generation Intel Xeon Scalable'],
    ['memory', '12 DDR4 DIMM slots; up to 1.5 TB'],
    ['drive_bays', '6 (+2) 2.5-inch SAS/SATA/NVMe hot-plug bays'],
    ['expansion', 'Three PCIe Gen3 x8 and four PCIe Gen3 x16 low-profile slots'],
    ['network', 'Two onboard 10G SFP+ ports'],
    ['power_description', '800 W or 1300 W CRPS options; 1+1 redundancy; no input-voltage claim'],
    ['cooling_description', '8056 dual-rotor fans; 3+1 redundancy'],
    ['management', 'IPMI 2.0 and KVM with dedicated LAN'],
    ['design_context', 'OTII hardware specification design; not a named operator deployment'],
], 'Original two-page Inventec brief. Selected page 2 table visually reviewed. Exact E850G4 '
   'platform facts supplement the separate 2020 manufacturer statement without transferring its '
   'role to this document. No installed auction configuration or operator assertion.')

add('inspur-nf5280m5-whitepaper-2019', 'inspur-nf5280m5-whitepaper.pdf', 'Inspur', 'NF5280M5', 'server', [
    ['document_revision', '5.0'], ['document_release_date', '2019-12-22'],
    ['source_pages', '1, 5, 21-22, 35-36 (PDF page numbers)'],
    ['physical_description', '2U; without handles W 435 x H 87 x D 779.8 mm; with handles W 478.8 x H 87 x D 811.5 mm'],
    ['processor', 'One or two Intel Xeon Scalable processors'],
    ['memory', 'Up to 24 DDR4 DIMMs; 2400/2666/2933 MT/s options'],
    ['power_description', 'Up to two hot-swappable CRPS PSUs. Platinum 550/800/1300/1600/2000 W options; the 1300/1600/2000 W options are limited to 1000 W at rated 110 VAC.'],
    ['power_limit', 'Certain GPU configurations do not support 1+1 redundancy. PSU alternatives depend on the actual module.'],
    ['source_power_errors', 'Titanium rows repeat 230VAC and list 550 W DC values under higher ratings. Page 36 prints 130W for a 1300 W row and AC -48V. These lines remain uncorrected in archived text; no electrical equivalence inferred.'],
    ['management', 'ASPEED AST2500; IPMI, SOL, KVM over IP and virtual media'],
    ['configuration_limit', 'Historical platform options, not actual auction-unit contents; no manufacturer or operator attribution.'],
], 'Original Inspur technical white paper revision 5.0. Brand/platform facts and selected '
   'specifications, with conditional PSU limits and apparent source errors retained. Not a '
   'manufacturer assertion, component-equivalence guarantee or hyperscaler deployment claim.')
save(DATA / 'storage-inspur-facts-2026-10-04.json', dict(sources=sources, devices=devices))

save(CAP / 'research-outcomes.json', dict(
    observed_at='2026-10-04', original_fetches=len(fetches), support_rows=1064, distinct_support_names=1055,
    support_scope='Publicly referenced feed includes mixed systems, third-party components, software and category names. All rows remain raw; no blanket Inspur branding or manufacture.',
    unreviewed_sources=['Inventec download-center first page retained for later pagination/model review'],
    bytedance_examples='B4020/B4010 remain without qualifying primary association evidence; trade-data previews unverified. Known Eximpedia 402 not retried.',
    new_catalog_models=['Metagross'], enriched_existing_models=['E850G4','NF5280M5'],
    no_network_in_importers=True, no_private_serial_queries=True, no_authentication_or_license_actions=True))
print('Prepared 1064 raw support rows, three platform records and two original PDFs (49 pages).')

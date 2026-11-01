"""Historical first-pass builder. Its nested-path PDF bundle was rejected before writes.
The byte-identical sibling-file v2 bundle is the manifest input; see the research note.
Do not rerun: curated facts and captured originals are already preserved.
"""
import json
import re
from pathlib import Path
from pypdf import PdfReader
from devicehunt.rebuild import file_sha256

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT/'data'
RAW = DATA/'raw'
CAP = RAW/'inspur-2018-inventec-2026-10-04'
TAG = 'inspur-2018-inventec-2026-10-04'
BUNDLE = RAW/('reference-documents-with-'+TAG+'.json')
FACTS = DATA/('inspur-2018-inventec-facts-2026-10-04.json')
assert not BUNDLE.exists() and not FACTS.exists()

def save(path, data):
    with path.open('x') as stream:
        stream.write(json.dumps(data, ensure_ascii=False, indent=2)+'\n')

fetches = json.loads((CAP/'fetch-report.json').read_text())
assert len(fetches) == 3
by_file = {r['path']: r for r in fetches}
texts = {}
for name, row in by_file.items():
    assert row['curl_exit'] == 0 and row['result'].startswith('200 ')
    assert file_sha256(CAP/name) == row['sha256']
    texts[name] = [dict(page=n, text=p.extract_text() or '')
                  for n,p in enumerate(PdfReader(CAP/name).pages,1)]

sources, devices = [], []
common_scope = ('Historical vendor product facts. Brand/platform association only; no physical-manufacturer, '
                'factory, FCC-ID or auction-unit provenance assertion. Specifications are catalog options, '
                'not the configuration or firmware/license availability of a particular surplus unit. '
                'English descriptions translate or paraphrase the cited original pages. Conflicting facts remain explicit.')

def source(sid, filename, coverage):
    sources.append(dict(id=sid, url=by_file[filename]['url'], tier='vendor',
        license='Public vendor product facts; original document rights retained',
        snapshot=str((CAP/filename).relative_to(DATA)), coverage=coverage+' '+common_scope))

sid = 'inspur-products-2018-reviewed'
source(sid, 'inspur-products-2018.pdf',
       'Selected named hardware in the 2018 Chinese product catalog. Software, unnamed nodes, third-party '
       'components and inferred suffix expansions are excluded. Model tables and product headings are '
       'reviewed manually. One source-scoped AGX-2 alias is explicitly paired with NF5288M5. '
       'SR-AI joint development with Baidu is design lineage only.')

def ins(model, kind, pages, facts=(), aliases=(), title=None):
    if isinstance(pages, int): pages = [pages]
    text = '\n'.join(texts['inspur-products-2018.pdf'][n-1]['text'] for n in pages)
    compact = lambda s: re.sub(r'\s+', '', s)
    assert compact(model) in compact(text), (model, pages)
    for alias in aliases: assert compact(alias) in compact(text), alias
    d = dict(source=sid, key=model, brand='Inspur', model=model,
        title=title or 'Inspur '+model, kind=kind,
        affiliations=[['Inspur','brand'],['Inspur','platform_vendor']],
        facts=[['source_observed_date','2026-10-04'],
               ['document_date_context','Copyright 2018; PDF metadata 2018-08-07; not a product release date'],
               ['source_pages',', '.join(map(str,pages))+' (PDF page numbers; each spread may contain two printed pages)'],
               ['configuration_limit','Historical catalog options; actual unit contents, firmware access and transferable software licenses unverified'],
               *facts])
    if aliases: d['identifiers'] = [['alias',a] for a in aliases]
    devices.append(d)
    return d

# Exact server model tables; values are options, not installed-unit configurations.
for model,page,dimensions,power,processor,memory in [
    ('NF5180M5',5,'435 x 43 x 750 mm (W x H x D)','Up to 1300W PSUs, 1+1 redundancy','Two Intel Xeon Scalable processors','24 DIMMs, up to 2666MT/s'),
    ('NF5280M5',6,'435 x 87 x 780 mm (W x H x D)','Up to 1600W PSUs, 1+1 redundancy','Two Intel Xeon Scalable processors','24 DIMMs, up to 2666MT/s'),
    ('NF5270M5',6,'447 x 87 x 720 mm (W x H x D)','500W/800W/1300W platinum PSUs, 1+1 redundancy','Two Intel Xeon Scalable processors, up to 150W','16 DIMMs, up to 2666MT/s'),
    ('NF8480M5',7,'448 x 175.5 x 812 mm (W x H x D)','Up to 1600W PSUs; 1+1/2+1/2+2/3+1 redundancy options','Two or four Intel Xeon Scalable processors','48 DIMMs, up to 2666MT/s'),
    ('NF8260M5',7,'446 x 87 x 779.8 mm (W x H x D)','Up to 1600W PSUs, 1+1 redundancy','Two or four Intel Xeon Scalable processors','48 DIMMs, up to 2666MT/s'),
    ('NF5486M5',9,'4U; 447 x 175 x 995 mm (W x H x D)','2000W PSUs, 1+1 redundancy','Two Intel Xeon Scalable processors, TDP 165W','16 DIMMs, up to 2666MT/s'),
    ('NF5466M5',10,'4U; 448 x 175.5 x 815 mm (W x H x D)','Up to 1600W PSUs, 1+1 redundancy','Table lists two or four Intel Xeon Scalable processors; highlights describe two 165W CPUs','24 DIMMs, up to 2666MT/s'),
    ('NP5570M5',14,'Tower; 220 x 435 x 610 mm (W x H x D)','Two 500W PSUs with 1+1 redundancy, or one 500W/800W PSU','Two Intel Xeon Scalable processors','16 DIMMs, up to 2666MT/s'),
]:
    ins(model,'server',page,[['physical_description',dimensions],['power_description',power],['processor',processor],['memory',memory]])
ins('天梭 M13','enterprise server',3,[['physical_description','483 x 528 x 919 mm (W x H x D)'],
    ['power_description','Six 3000W PSUs; N+N redundancy (N: 2 or 3)'],
    ['memory_as_printed','Page 3 introduction: up to 48TB; specification table: maximum 24T DDR4-2400'],
    ['specification_conflict','Memory claims conflict on the same page; no single capacity selected']])
ins('天梭 TS860G3','enterprise server',4,[['physical_description','444 x 352 x 830 mm (W x H x D)'],
    ['power_description','Up to eight 800W PSUs; N+M/N+N redundancy'],
    ['processor','Up to eight Intel Xeon E7-4800/8800 v3/v4 processors']])
ins('天梭 TS860M5','enterprise server',4,[['physical_description','4U; 448 x 175.5 x 800 mm (W x H x D)'],
    ['power_description','Up to four 800/1300/1600 CRPS PSUs as printed; 2+2/3+1 redundancy'],
    ['processor','Eight Intel Xeon 6100/8100 series scalable processors']])

ins('i48','server chassis',8,[['physical_description','4U chassis; 448 x 175.5 x 898.2 mm (W x H x D)'],
    ['power_description','Four 800W/1300W/1600W/2000W 80Plus platinum PSUs; 2+2 redundancy'],
    ['node_models','NS5488M5; NS5484M5; NS5482M5; NS5482JD'],
    ['identity_scope','Chassis and its separately named compute/storage/JBOD nodes are not aliases']])
for model,form,storage in [
    ('NS5488M5','4U8-node configuration','Four front 2.5-inch SATA/SAS/SSD/NVMe drives'),
    ('NS5484M5','4U4-node configuration','Two front 2.5-inch drives plus twelve internal 3.5/2.5-inch drives'),
    ('NS5482M5','4U2-node configuration','Two front 2.5-inch drives plus thirty-six internal 3.5/2.5-inch drives'),
]:
    ins(model,'server node',8,[['chassis_model','i48'],['physical_description',form+' in the i48 chassis'],
        ['processor','Two Intel Xeon Scalable processors per node'],['memory','16 DDR4 DIMMs per node'],['storage',storage],
        ['identity_scope','Node for the separately named i48 chassis; not an independent AC-powered server assertion']])
ins('NS5482JD','JBOD node',8,[['chassis_model','i48'],['storage','36 internal 3.5-inch hot-swap drives'],
    ['required_components','Catalog explicitly requires pairing with another compute node; describes a 4U1-node 72-drive configuration'],
    ['physical_description','JBOD node listed separately from the i48 chassis and compute nodes']])
ins('i24','server platform',9,[['physical_description','2U chassis for four NS5162M5 nodes; 446 x 87.5 x 805 mm (2.5-inch chassis) or 845 mm depth (3.5-inch chassis)'],
    ['power_description','Two 2000W 80Plus platinum PSUs; 1+1 redundancy'],['cooling_description','Air- or water-cooling options described'],
    ['compute_node_model','NS5162M5'],['storage_constraint','Chassis drives are divided equally among nodes; no cross-node use']])
ins('NS5162M5','compute node',9,[['chassis_model','i24'],['physical_description','Four nodes in the separately identified i24 2U chassis'],
    ['processor','Two Intel Xeon Scalable processors per node'],['memory','16 DDR4 DIMMs per node'],
    ['storage','Up to six 2.5-inch or three 3.5-inch drives per node']])
ins('I9000','blade chassis',[10,11],[['physical_description','12U; up to sixteen half-width or eight full-width blades'],
    ['power_description','Page 11 chassis table: 3000W 100-240V AC hot-swap modules, N+N or N+M redundancy'],
    ['cooling_description','Up to ten chassis fan modules'],['node_models','NX8480M4; NX8880M4; NX5460M5; NX8280M4']])
for model in ('NX8480M4','NX8880M4','NX5460M5','NX8280M4'):
    ins(model,'blade server node',11,[['chassis_model','I9000'],
        ['identity_scope','Named node in I9000 section; chassis, I/O boxes and unnamed expansion nodes remain separate']])
ins('SR4200','integrated rack',12,[['physical_description','600 mm wide x 1200 mm deep; 2100 mm rolling height; usable 42U; inner width 538 mm, each U 46.5 mm as printed'],
    ['power_description','Centralized N+N supplies; 220V AC/240V DC; individual PSU rating 2000W-2500W; power modules at U21-U23'],
    ['cooling_description','Front intake, rear exhaust; rear centralized fan wall'],
    ['node_models','SN5160M4; SN5150M4; SN6116M5; SN5161M5; SN5171M5; SN3116M4S; SN3116J4']])
for model,page,form in [('SN5160M4',12,'1U1N'),('SN5150M4',12,'1U2N'),('SN6116M5',12,'1U1N'),
                        ('SN5161M5',12,'1U1N'),('SN5171M5',13,'1U1N'),('SN3116M4S',13,'1U1N')]:
    ins(model,'server node',page,[['rack_section','SR integrated rack'],['physical_description',form+' as printed'],
        ['identity_scope','Node in the SR rack section; no standalone mains-input assumption']])
ins('SN3116J4','JBOD node',13,[['physical_description','1U1 node JBOD'],
    ['power_description','12V DC through a power clip'],['storage','20 hot-swap 3.5/2.5-inch SATA/SAS drives or SSDs'],
    ['interfaces','Four MiniSAS HD; 12G SAS / 6G SATA; 100M management port'],
    ['identity_scope','Separate from the adjacent unnamed SAS switch']])
ins('InCloudRack','integrated rack',13,[['physical_description','42U standard rack; up to 48 nodes, configuration-dependent'],
    ['power_description','18 x 3000W platinum PSUs with 9+9 redundancy; table states 240V DC and two 380V/32A industrial connectors'],
    ['cooling_description','24 fan modules, four per tray with 3+1 redundancy'],
    ['node_models','NX5460M4; NX8280M4; NX8480M4; NX8880M4'],
    ['identity_scope','NX5460M4 in this table is not silently substituted with NX5460M5 from the I9000 table']])
ins('NX5460M4','server node',13,[['rack_model','InCloudRack'],['physical_description','Two-socket half-width node; up to 48 in the stated rack configuration']])
ins('ON5263M5','compute node',14,[['physical_description','174.5 x 91.1 x 731.4 mm (W x H x D)'],
    ['power_description','Centralized rack bus-bar power'],['processor','Two Intel Xeon Scalable processors'],['memory','16 DIMMs']])
for model,form in [('OB5161M4','1U full-width storage server node'),('OB5162M5','1U half-width compute server node; 212 x 41.5 x 907 mm (W x H x D)')]:
    ins(model,'Open19 server node',15,[['physical_description',form],['power_description','Open19 power shelf'],
        ['required_components','Open19 switch and power components form the complete server system; standardized network and power cabling described'],
        ['identity_scope','Node is distinct from the Open19 cage, switch and power shelf']])
ins('NF5288M5','GPU server',[16,33],[['model_as_printed','NF5288M5 (AGX-2)'],
    ['physical_description','2U server with eight GPUs; dual Intel Xeon Skylake Gold/Platinum processors'],
    ['power_description','PDF p16: two 3000W platinum modules. PDF p33: high-efficiency titanium redundant dual supplies'],
    ['specification_conflict','The two pages differ on PSU efficiency description; retained independently without resolving a unit configuration'],
    ['cooling_description','Water cooling supported as a catalog option']], aliases=['AGX-2'])
ins('NF5468M5','GPU server',17,[['physical_description','4U; eight V100/P100/P40 or sixteen P4 GPUs as catalog options'],
    ['power_description','Four 1600W/2000W/2200W 80Plus platinum PSU modules; 2+2 redundancy']])
ins('GX4','GPU expansion enclosure',18,[['physical_description','2U GPU box; four GPU cards'],
    ['power_description','1600W supplies, 1+1 redundancy'],
    ['required_components','Used with a dual-CPU head node; four GX4 boxes can form a sixteen-GPU system'],
    ['identity_scope','GPU expansion enclosure; not a standalone CPU server']])
ins('NF5280M5-V','GPU server',18,[['physical_description','2U dual-socket server; eight P4 GPUs'],
    ['power_description','High-efficiency titanium redundant dual supplies'],['identity_scope','Full -V suffix retained; not an alias of NF5280M5']])
d = ins('SR-AI整机柜','integrated GPU rack',19,[['model_as_printed','Heading: SR-AI整机柜; model table: SR AI 整机柜'],
    ['head_node_as_printed','SN 5160M4'],['gpu_box_model','SN3410G4'],
    ['design_statement','Inspur and Baidu jointly developed the SR-AI rack; historical product design, not deployment or auction-unit provenance'],
    ['physical_description','Rack architecture separates CPU head node and GPU box; the table names a 1U head node and 4U GPU box']], title='Inspur SR-AI整机柜 (SR-AI rack)')
d['affiliations'].append(['Baidu','platform_designer'])
d['hyperscaler'] = [['Baidu','documented_design_lineage','The 2018 Inspur catalog, PDF p19, explicitly describes joint development of the SR-AI rack with Baidu. No operator deployment or individual-unit provenance asserted.']]
ins('SN3410G4','GPU expansion enclosure',19,[['rack_model','SR-AI整机柜'],
    ['physical_description','4U GPU box; sixteen double-width PCIe 3.0 expansion slots'],
    ['identity_scope','Separately named GPU box; no independent Baidu deployment assertion']])
ins('F10A','FPGA accelerator',19,[['physical_description','Half-height, half-length PCIe Gen3 x8 board'],
    ['power_description','12V from PCIe 3.0 slot; no external power; 45W peak / 35W average as printed'],
    ['interfaces','Two SFP+ GE/10GE ports'],['memory','Two DDR4 SODIMMs, 4-16GB each, 2133Mbps, 72-bit with ECC'],
    ['controller','Intel Arria 10 GX1150']])

# Storage tables: fully spelled models are individual identities; grouped names stay grouped.
for model,page,kind,physical in [
    ('AS1300G2',23,'storage system','2U12 bays'),('AS1500G2',23,'storage system','4U24 bays'),('AS1600G2',23,'storage system','4U36 bays'),
    ('AS2200G2',23,'storage system','2U12 or 2U25 controller enclosure'),('AS2600G2',23,'storage system','2U12, 2U25 or 3U48 controller enclosure'),
    ('AS5300G2',24,'storage system','2U12, 2U25 or 3U48 controller enclosure'),('AS5500G2',24,'storage system','2U12, 2U25 or 3U48 controller enclosure'),
    ('AS5600',25,'storage system','482 x 805 x 133 mm, dimensions printed as length x width x height'),
    ('AS5600G2',25,'storage system','4U controller enclosure'),('AS5800G2',25,'storage system','4U controller enclosure'),
    ('AS18000',26,'storage system','Multi-controller storage with separately described controller, switch and expansion enclosures'),
    ('AS13000-M1/M3/M5/Rack',26,'storage system family','Grouped hardware family: 2U12, 2U25, 4U36 and 42U (1U12) configurations'),
    ('HF5000-H',27,'storage system','3U16'),('HF5000-F',27,'storage system','4U48'),
    ('AS2600G2-F',27,'flash storage system','2U25 integrated controller/drives'),
    ('AS5300G2-F',27,'flash storage system','2U25 integrated controller/drives'),
    ('AS5500G2-F',27,'flash storage system','2U25 integrated controller/drives'),
    ('AS5600G2-F',27,'flash storage system','4U; separate controller/drives'),('AS5800G2-F',27,'flash storage system','4U; separate controller/drives'),
]:
    d=ins(model,kind,page,[['physical_description',physical]])
    if '/' in model: d['facts'].append(['identity_scope','Printed grouped name retained; no inferred AS13000 suffix expansion or SKU equivalence'])
for model,drives,slots in [('TL1000','Two half-height or one full-height drive','24'),('TL2000','Four half-height or two full-height drives','48')]:
    ins(model,'tape library',28,[['drive_support',drives+'; LTO5/6/7 as catalog options'],['tape_slots',slots],['interfaces','FC or SAS; source prose lists 8Gb FC and 6Gb SAS']])
ins('DP1000-M1','backup appliance',28,[['storage','12 internal disks, expandable to 144; NL-SAS/SAS'],
    ['memory','16GB standard, up to 384GB'],['identity_scope','Named physical backup appliance; DPS backup software remains separate']])
ins('FS5900','Fibre Channel switch',30,[['physical_description','1U; 43 x 429 x 307 mm (H x W x D); 4.2kg without SFP/SFP+'],
    ['power_description','AC 85-264V, 1A-0.5A; 48W typical, up to 57W with 24 x 8Gb ports'],
    ['interfaces','8 to 24 Fibre Channel ports in increments of eight; 2/4/8Gbps'],
    ['software_installation_description','Catalog describes incremental port expansion and trial licenses for selected plug-in functions; actual unit entitlements unverified']])
ins('FS6500/6600','Fibre Channel switch family',31,[['physical_description','1U; 43 x 438 x 443 mm (H x W x D); 9.16kg with dual supplies, without optics'],
    ['power_description','AC 85-264V, 5A-2.5A; 6500: 60W typical/80W maximum as described; 6600: 72W typical/110W maximum'],
    ['interfaces','2/4/8/16Gbps; 6500: 12 to 24 ports; 6600: 24 to 48 ports, in increments of twelve'],
    ['identity_scope','Printed FS6500/6600 grouping retained; full separate order codes not inferred']])
ins('FS6800','Fibre Channel switch',31,[['physical_description','2U'],
    ['interfaces','48 to 96 ports, 4/8/16Gbps; 24-port expansion increments'],
    ['software_installation_description','Catalog lists licensed Fabric Vision, ISL Trunking, Extended Fabric and routing options; availability on a surplus unit unverified']])
for model,ports,blades in [('FS9510','256','four horizontal'),('FS9520','512','eight vertical')]:
    ins(model,'Fibre Channel director',32,[['interfaces','Up to '+ports+' x 16Gbps FC ports'],
        ['required_components','Up to '+blades+' port blades; 32/48/64-port blade options as printed']])
ins('I8020','blade chassis',33,[['physical_description','8U chassis for twenty compute blades'],
    ['power_description','Up to eight hot-swap 2200W PSU modules; N+M redundancy'],
    ['compute_node_model','NX5440M5'],['interfaces','100Gb/s EDR IB or Omni-Path options; Ethernet modules also described']])
ins('NX5440M5','blade server node',33,[['chassis_model','I8020'],['processor','Two Intel Xeon Skylake Gold/Platinum processors'],
    ['memory','16 DDR4 ECC DIMM slots'],['storage','Three 2.5-inch SATA drives (two may be NVMe) plus one M.2 SATA/NVMe SSD'],
    ['identity_scope','Blade node distinct from its I8020 chassis']])
for model,form in [('G100','Desktop appliance'),('G1000','Central compute appliance')]:
    ins(model,'genomics appliance',36,[['product_family','GeneEngine M2'],['physical_description',form],
        ['identity_scope','Two named hardware configurations under the GeneEngine M2 family; family name is not an alias']])
for model,form,power in [('SSA1900','1U','Single PSU'),('SSA2000','1U','Single PSU'),('SSA3000','1U','Redundant PSUs'),
                        ('SSA4000','1U','Redundant PSUs'),('SSA5000','2U','Redundant PSUs'),('SSA6000','2U','Redundant PSUs'),('SSA7000','2U','Redundant PSUs')]:
    ins(model,'application delivery appliance',41,[['physical_description',form],['power_description',power],
        ['interface_requirement','SSA1900 has six onboard gigabit copper ports; other SSA models require at least one optional network-card module' if model != 'SSA1900' else 'Six onboard gigabit copper ports']])
for model in ('SSC 1900','SSC 2900','SSC 3900'):
    ins(model,'operations security appliance',42,[['physical_description','1U'],['power_description','Redundant PSUs'],
        ['identity_scope','Space in printed model preserved; separate from SSA models']])
for model,power in [('SDP1000','80Plus gold PSU'),('SDP2000','80Plus gold redundant PSUs'),('SDP3000','80Plus gold redundant PSUs')]:
    ins(model,'database security appliance',43,[['physical_description','1U'],['power_description',power]])
for model in ('1010S','1010E','1020S','1020E'):
    small=model.startswith('1010')
    ins(model,'hyperconverged appliance',47,[['product_family','InCloud Rail 1000 Series'],
        ['physical_description','2U1 node; 447 x 87 x 720 mm (W x H x D)' if small else '2U4 nodes; 430 x 88 x 730 mm (W x H x D)'],
        ['power_description','550W or above, 1+1 redundancy; table input: AC 110-240V / DC -40V to -60V, 180-400V' if small else '2000W or above, 1+1 redundancy; table input: 200-240V, 50-60Hz'],
        ['power_configuration_limit','Alternative catalog configurations; this does not identify the PSU fitted to any particular unit'],
        ['identity_scope','Full 2018 model as printed; no inferred equivalence to later -M5 codes or server-node models']])

# Inventec product brochures do not turn general company boilerplate into per-model manufacture proof.
for source_id,filename,model,kind,facts in [
    ('inventec-artemis-ii-rack-2025','inventec-artemis-ii-rack.pdf','Artemis II Rack','integrated GPU rack',[
        ['document_date_context','Copyright 2025; PDF metadata 2025-08-15; not an independently established release date'],
        ['physical_description','48RU MGX rack; 600 x 2236 x 1068 mm (W x H x D); rolling height 2230 mm'],
        ['compute_configuration','NVIDIA GB300 NVL72; 36 Grace CPUs and 72 Blackwell GPUs; 18 x 1RU compute trays, 9 x 1RU NVLink switch trays'],
        ['power_description','Page 1 states maximum 136kW for GB300 NVL72 building blocks. Page 2 diagram labels two groups of 3 x 1RU 33kW power shelves'],
        ['power_configuration_limit','No aggregate power capacity, redundancy mode or input voltage inferred from the diagram'],
        ['cooling_description','Liquid cooling: row-based L2L or sidecar L2A; blind-mate manifold; rack-mounted CDU and leakage detection described'],
        ['identity_scope','Inventec rack platform; NVIDIA components are not new Inventec device models']]),
    ('inventec-x680m-2024','inventec-x680m.pdf','X680m','CXL memory accelerator',[
        ['document_date_context','Copyright 2024; PDF metadata 2024-05-15; not an independently established release date'],
        ['model_as_printed','Page 2 Model Name: Inventec X680m; page 1 prose also uses X680m'],
        ['physical_description','Single-width, full-height, three-quarter-length PCIe Gen5 x16 CEM card'],
        ['controller','Altera Agilex 7 I-series FPGA; quad-core ARM-A72'],
        ['memory_page_1_as_printed','2 ch DDR4 64GB'],['memory_page_2_as_printed','2 channel 8GB(64 + 8bits)Width'],
        ['specification_conflict','Memory statements differ between the two pages; no reconciled capacity asserted'],
        ['arm_hps_dram','Separate page 2 ARM HPS row: DDR4-3200, 1 channel 8GB (64 + 8bits) width'],
        ['storage','Two M.2 22110 slots, up to 4TB as printed; per-drive versus aggregate capacity unspecified'],
        ['interfaces','One 100Gbps QSFP-DD; RJ45 HPS debug; Micro USB TTY'],
        ['boot_media','FPGA qSPI; ARM eMMC 32GB']]),
]:
    source(source_id,filename,'Two-page original product brochure; exact product name and selected specifications reviewed.')
    assert model in '\n'.join(p['text'] for p in texts[filename])
    devices.append(dict(source=source_id,key=model,brand='Inventec',model=model,title='Inventec '+model,kind=kind,
        affiliations=[['Inventec','brand'],['Inventec','platform_vendor']],
        facts=[['source_observed_date','2026-10-04'],['source_pages','1, 2 (PDF page numbers)'],
               ['configuration_limit','Historical platform options, not an actual unit configuration, manufacturer or operator claim'],*facts]))

assert len({(d['source'],d['key']) for d in devices}) == len(devices)
assert sum(bool(d.get('hyperscaler')) for d in devices) == 1
assert sum(bool(d.get('identifiers')) for d in devices) == 1
# Validate every output before creating derived files.
bundle=json.loads((RAW/'reference-documents-with-inspur-rail-2026-10-04.json').read_text())
for name,title,count in [('inspur-products-2018.pdf','Inspur Chinese product catalog, copyright 2018',50),
                          ('inventec-artemis-ii-rack.pdf','Inventec Artemis II Rack brochure, copyright 2025',2),
                          ('inventec-x680m.pdf','Inventec X680m CXL Memory Accelerator brochure, copyright 2024',2)]:
    assert len(texts[name]) == count
    textpath=(CAP/name).with_suffix('.pages.json')
    save(textpath,dict(pdf_sha256=file_sha256(CAP/name),extraction_method='pypdf embedded text; no OCR',pages=texts[name]))
    bundle['documents'].append(dict(id=Path(name).stem,
        pdf=dict(path=str((CAP/name).relative_to(RAW)),sha256=file_sha256(CAP/name)),
        text=dict(path=str(textpath.relative_to(RAW)),sha256=file_sha256(textpath)),
        url=by_file[name]['url'],title=title,category='vendor reference document',tier='vendor',page_count=count,
        scope=common_scope+' Full embedded text includes uncurated software/component mentions; document hits do not establish hardware identities.'))
save(BUNDLE,bundle)
save(FACTS,dict(reviewed_on='2026-10-04',sources=sources,devices=devices))
save(CAP/'research-outcomes.json',dict(curated_records=len(devices),sources=len(sources),added_original_pdfs=3,added_pdf_pages=54,
    models_by_source={s['id']:[d['model'] for d in devices if d['source']==s['id']] for s in sources},
    visual_review='19 page renders reviewed across the preceding capture pass and continuation; table layout, names and selected facts checked; not every page visually reviewed.',
    excluded=['Software-only products including DPS-M1, BCP, ClusterEngine, InCloud OS and KDB',
              'Unnamed expansion modules and standalone third-party component identities',
              'Inferred full codes from FS6500/6600 and AS13000-M1/M3/M5/Rack',
              'Automatic conversion of Inspur support-menu rows into device claims'],
    unresolved=['X680m memory conflict','M13 memory conflict','NF5288M5 PSU-description conflict',
                'Unreviewed detailed tables remain available through full-text search',
                'ByteDance B4020/B4010 primary affiliation evidence remains absent']))
print(json.dumps(dict(records=len(devices),sources=len(sources),original_pdfs=3,pages=54),indent=2))

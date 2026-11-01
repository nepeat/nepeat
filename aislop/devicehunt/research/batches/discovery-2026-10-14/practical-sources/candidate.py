"""Offline curated proposal explicitly requested by coordinator after discovery."""
from pathlib import Path
import hashlib
import json
from devicehunt.documents import read_capture
from devicehunt.importers import prepare_curated

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
def sha(b): return hashlib.sha256(b).hexdigest()
def norm(s): return ' '.join(s.split())
LIMIT = ('Historical source-local documentation statements only; documentation_vendor role only. '
         'No manufacturer, factory, OEM, deployment, current support/stock/entitlement, individual-unit configuration or universal component fitment inference. '
         'Exact printed names and order codes remain separate identities; no prefix stripping, alias, suffix expansion or merge. '
         'Internal 3.3V standby/12V distribution rails are not external AC input ratings. Figure legend numbers are labels, not installed quantities. '
         'Component references stay on the named parent; no standalone Intel-generic-name or memory/PSU/fan identities.')

def main():
    discovery = json.loads((HERE/'findings.json').read_text())
    receipts = [json.loads(x) for x in (HERE/'requests.jsonl').read_text().splitlines()]
    pdf = HERE/'sd4100-known_captured_target_checked.pdf'
    pages, metadata = read_capture((HERE/'sd4100-pages.json').read_bytes(), sha(pdf.read_bytes()))
    sources = []
    models = {'SD4100': ('Celestica','SD4100','storage enclosure','Celestica Inc.'),
              'zs3-2': ('Oracle','Oracle ZFS Storage ZS3-2','storage controller','Oracle Corporation'),
              'zs3-4': ('Oracle','Oracle ZFS Storage ZS3-4','storage controller','Oracle Corporation')}
    source_ids = {'SD4100':'celestica-sd4100-datasheet-2026-10-14', 'zs3-2':'oracle-zfs-storage-zs3-2-internal-components-2026-10-14', 'zs3-4':'oracle-zfs-storage-zs3-4-internal-components-2026-10-14'}
    for key in models:
        capture = next(r for r in receipts if r['status']==200 and not r['is_policy'] and r['group']==('sd4100' if key=='SD4100' else key))
        s = dict(id=source_ids[key],url=capture['url'],tier='vendor',license='Public factual vendor documentation; original upstream rights retained; local research only.',coverage=LIMIT+' '+' '.join(discovery['absences_and_limits'][key.upper()]),snapshot=capture['body_path'],original_sha256=capture['body_sha256'],captured_at_utc=capture['finished_at_utc'])
        if key=='SD4100': s.update(text_sidecar='sd4100-pages.json',extraction_metadata=metadata)
        sources.append(s)
    def row(key, model=None):
        brand, printed, kind, company = models[key]
        if model: printed,kind=model,'storage enclosure order configuration'
        return dict(source=source_ids[key],key=printed,brand=brand,model=printed,title=printed if printed.startswith(brand+' ') else brand+' '+printed,kind=kind,affiliations=[[company,'documentation_vendor']],identifiers=[['part_number',printed]] if model else [],hyperscaler=[],facts=[],evidence=[],record_scope='Own exact printed order block and explicit SD4100 heading context only; no base-specification inheritance.' if model else 'Named model source-local statements only; no identities merged.',scope_limitations=LIMIT,source_absences=discovery['absences_and_limits'][key.upper()])
    devices = [row(k) for k in models]
    by_key = dict(zip(models, devices))
    sd_names = ['physical_description','service_constraint_as_printed','storage_drive_capacity_as_printed','system_uplink_as_printed','drive_interfaces_as_printed','power_description','module_configuration_as_printed','module_ports_as_printed','module_console_as_printed','power_description','power_compliance_as_printed','power_description','cooling_description','physical_description','physical_description','physical_description','environmental_limits_as_printed','environmental_limits_as_printed','environmental_limits_as_printed','environmental_limits_as_printed','acoustic_statement_as_printed','ordering_row_as_printed','ordering_bundle_as_printed','specification_change_note_as_printed','copyright_statement_as_printed','document_footer_token_as_printed']
    oracle_names = {'zs3-2':{68:'service_constraint_as_printed',72:'pcie_riser_configuration_as_printed',78:'power_description',90:'power_description',101:'drive_backplane_description_as_printed',198:'memory_configuration_as_printed',201:'memory_population_constraint_as_printed',348:'service_constraint_as_printed',437:'service_constraint_as_printed'}, 'zs3-4':{154:'service_constraint_as_printed',159:'power_description',171:'power_description',183:'power_distribution_interconnect_as_printed',193:'power_description',201:'cooling_description',210:'drive_backplane_description_as_printed',219:'storage_configuration_as_printed',221:'read_cache_configuration_constraint_as_printed',252:'hba_connectivity_as_printed',257:'memory_configuration_as_printed',260:'memory_population_constraint_as_printed',273:'fan_fault_indicator_as_printed',277:'fan_fault_indicator_as_printed',282:'fan_fault_indicator_as_printed',288:'fan_fault_indicator_as_printed',292:'fan_overtemperature_note_as_printed'}}
    def add(d,name,quote,locator,boundary,original_id=None,literal_quote=None):
        key = next(k for k,sid in source_ids.items() if sid==d['source'])
        source = next(s for s in sources if s['id']==d['source'])
        label = f'PDF p. {locator["pdf_page"]}' if 'pdf_page' in locator else f'HTML lines {locator["html_start_line"]}-{locator["html_end_line"]}'
        d['facts'].append([name, quote+' ['+label+'; '+boundary+']'])
        e = dict(fact_index=len(d['facts'])-1,fact_name=name,source_url=source['url'],capture_sha256=source['original_sha256'],literal_quote=literal_quote if literal_quote is not None else quote,locator=locator,selected_scope=d['record_scope'],value_method='Literal source-local statement with explicit scope note' if literal_quote is None else 'Literal table cells tagged by their original column headers, with explicit scope note')
        if original_id:e['original_observation_id']=original_id
        if key=='SD4100':
            visual = HERE/f'sd4100-render-page{locator["pdf_page"]}.png'
            e.update(complete_sidecar_sha256=sha((HERE/'sd4100-pages.json').read_bytes()),visual_original=str(visual.relative_to(ROOT)),visual_sha256=sha(visual.read_bytes()))
        d['evidence'].append(e)
    def pdf_locator(page,quote):
        lines=pages[page-1]['text'].splitlines()
        windows=[(j-i,i,j) for i in range(len(lines)) for j in range(i+1,len(lines)+1) if norm(quote) in norm(' '.join(lines[i:j]))]
        _,i,j=min(windows)
        return dict(pdf_page=page,lines_1based=list(range(i+1,j+1)),raw_lines=lines[i:j])
    sd_index=0
    for f in discovery['observations']:
        key=f['source'];d=by_key[key]
        if key=='SD4100':
            name=sd_names[sd_index];sd_index+=1
            loc=pdf_locator(f['page'],f['quote'])
        else:
            name=oracle_names[key][f['html_start_line']]
            loc={k:f[k] for k in ('html_start_line','html_end_line')}
        add(d,name,f['quote'],loc,f['boundary'],f['id'])
    order=row('SD4100','R4086-F9001-01')
    for quote,name,boundary in [
        ('R4086-F9001-01:','ordering_row_as_printed','Own literal order-code heading; no alias merge with SD4100.'),
        ('SD4100 2U 12 HDD SAS-4 Storage Expansion System','base_model_heading_as_printed','Explicit document heading context only; base power/cooling/dimensions are not copied to this order identity.'),
        ('JBOD system (chassis, PSUs, SAS-4 I/O modules, fan modules) 12x drive carriers 6x dummy drive carriers Rail kit set','ordering_bundle_as_printed','One visible R4086-F9001-01 order block; retain carrier/dummy carrier rows literally, no inferred drives or fan/PSU rating.')]:
        add(order,name,quote,pdf_locator(2,quote),boundary)
    devices.append(order)
    article=json.loads((HERE/'zs3-2-article.json').read_text())
    for start,name in [(208,'memory_population_table_row_as_printed'),(261,'replaceable_component_row_as_printed')]:
        t=next(t for t in article['tables'] if t['start_line']==start)
        headers=[c['text'] for c in t['rows'][0]['cells']]
        for r in t['rows'][1:]:
            cells=[c['text'] for c in r['cells']]
            quote=' | '.join(f'{h}: {v}' for h,v in zip(headers,cells))
            loc=dict(html_start_line=r['start_line'],html_end_line=r['end_line'],table_start_line=start,table_title=t['attrs']['title'],headers=headers,literal_cells=cells)
            boundary='Exact source-local parent table row; no component identity, parent alias, manufacturer role or actual installed inventory. Preserve literal Capacity dot; table title is CPU Descriptions despite DIMM-slot content.' if start==208 else 'Exact CPU/memory service-reference row on Oracle ZFS Storage ZS3-2 parent; FRU/CRU stays literal, no standalone component identity or manufacturer inference.'
            add(by_key['zs3-2'],name,quote,loc,boundary,literal_quote=' '.join(cells))
    data=dict(sources=sources,devices=devices,status='Local curated candidate; coordinator-requested after discovery; independent review required; no import',scope_limitations=LIMIT,complete_document_scope='SD4100 complete 2-page PDF and complete text sidecar; both saved Oracle article texts and all seven literal HTML tables retained.',routing_decisions=['Existing exact Celestica SD4100 and Oracle Oracle ZFS Storage ZS3-2 identities receive new source records.', 'New Celestica R4086-F9001-01 is the one explicitly printed order code; own bundle facts only, no inherited base specifications.', 'Full printed Oracle ZFS Storage ZS3-4 remains a distinct exact name from registry ZS3-4; no equivalence claimed.', 'Oracle CPU/memory codes7019701/7041603/7055964 remain parent source-scoped table references, without standalone identities.'])
    path=HERE/'candidate.json'
    path.write_text(json.dumps(data,indent=2)+'\n')
    prepared=prepare_curated(path)
    assert len(prepared)==3 and sum(len(x[3]) for x in prepared)==4
    print(json.dumps({'sources':3,'records':4,'facts':sum(len(d['facts']) for d in devices),'candidate_sha256':sha(path.read_bytes()),'prepare_curated':'passed; no catalog/database created or opened'}))

if __name__=='__main__':main()

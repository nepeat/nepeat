"""Build finite source-local discovery observations from immutable captures."""
from pathlib import Path
import hashlib
import json

HERE = Path(__file__).resolve().parent
def sha(b): return hashlib.sha256(b).hexdigest()
def norm(s): return ' '.join(s.split())

def main():
    sidecar = json.loads((HERE/'sd4100-pages.json').read_text())
    findings = []
    def pdf(page, quote, boundary):
        text = sidecar['pages'][page-1]['text']
        assert norm(quote) in norm(text), quote
        findings.append(dict(id=f'sd4100-{sum(f["source"] == "SD4100" for f in findings)+1:02}', source='SD4100', printed_scope='SD4100', source_file='sd4100-known_captured_target_checked.pdf', source_sha256=sidecar['pdf_sha256'], page=page, quote=quote, boundary=boundary, visual_review=f'sd4100-render-page{page}.png'))
    for quote, boundary in [
        ('2U enclosure for fitment in standard 19” wide racks', 'Enclosure/rack fitment; no exact unit condition claim.'),
        ('Cable management assembly (CMA) not required', 'Source installation statement only.'),
        ('Up to 12 3.5” form factor drives', 'Capacity limit, not installed drive count.'),
        ('SAS-4 system uplink', 'System uplink distinct from drive interfaces.'),
        ('SAS-3 or SATA-3 connectivity to drives', 'Drive interface statement; no drive SKU fitment inference.'),
        ('13W max. power per drive bay @ 35°C', 'Per-drive-bay maximum at stated temperature; not chassis draw.'),
        ('Two hot-swappable expansion SAS modules (ESM)', 'ESMs only; do not apply hot-swap qualifier to other components.'),
        ('Four 24G SAS-4 mini-SAS HD ports per ESM', 'Per ESM; no expanded total required.'),
        ('One UART via USB-C', 'Nested ESM feature in visible layout; preserve module scope.'),
        ('Two power efficient 800W CRPS PSUs', 'Printed PSU number/rating; no actual appliance draw or PSU part number. Adjacent visible Lot-9 claim recorded separately because two-column extraction interleaves left prose.'),
        ('(Lot-9 Compliant)', 'Visually follows the two 800W CRPS PSUs in the same row; source compliance claim only.'),
        ('100-127,200-240V AC input', 'Keep both disjoint printed input ranges; frequency/current absent.'),
        ('Three 40mm dual-rotor fans per ESM', 'Per ESM; do not interpret as three fans per chassis.'),
        ('Height: 87 mm (2U)', 'Printed height.'),
        ('Width: 446 mm', 'Printed width.'),
        ('Depth: 545 mm', 'Printed depth.'),
        ('Operating Temperature: 5°C to 35°C', 'Operating range distinct from non-operating range.'),
        ('Non-operating Temperature: -40°C to 60°C', 'Non-operating range only.'),
        ('Humidity: 8% to 80% RH', 'No non-condensing qualifier printed in this row.'),
        ('Altitude: 0 to 950 m (De-rate temperature 1°C per 300 m above 950 m)', 'Preserve entire derating clause; no inferred upper altitude.'),
        ('Sound Power: (23+/-2°C) <7.6Bel', 'Sound power with printed condition and unit; not sound pressure.'),
    ]: pdf(1, quote, boundary)
    pdf(2, 'R4086-F9001-01:', 'Literal order code heading for the following visible JBOD bundle under SD4100; no identity merge or inferred alias.')
    pdf(2, 'JBOD system (chassis, PSUs, SAS-4 I/O modules, fan modules) 12x drive carriers 6x dummy drive carriers Rail kit set', 'One literal order-information bundle under R4086-F9001-01; no PSU/FAN FRU code, drive count reinterpretation or inferred drive inclusion. The 12x carrier and 6x dummy carrier rows are retained independently as printed.')
    pdf(2, 'All specifications and figures are subject to change without prior notice.', 'Applies to the datasheet specifications/order information.')
    pdf(2, 'Copyright Celestica Inc. 2024.', 'Copyright year only; not manufacturing or publication date.')
    pdf(2, '121224', 'Printed footer token; no inferred revision/date decoding. PDF title separately contains this token.')
    selectors = {
        'zs3-2': {
            68: 'FRU service restriction; not permission or recommendation for customer replacement.',
            72: 'Explicit three risers per system and two CRU PCIe cards per riser; legend labels below are not counts.',
            78: 'Motherboard/SP context: 3.3V standby rail whenever AC supplied, even while off; not external input rating.',
            90: 'Main 12V distribution, direct PSU-to-PDB connection and top-cover interlock; not AC input or PSU output rating.',
            101: 'Explicit eight-disk backplane; component LEDs remain source-local.',
            198: 'Sixteen DDR3 LV DIMM slots in two groups; no installed capacity inference.',
            201: 'All sockets must be occupied by identical DDR3 DIMMs; retain alongside the literal component table.',
            348: 'Storage/power/cooling figure legend only; FRU service restriction explicit.',
            437: 'Internal cables are FRUs and not customer-serviceable; no individual cable part numbers printed.',
        },
        'zs3-4': {
            154: 'Internal board FRUs not customer-serviceable; service-platform name is not merged with registry ZS3-4.',
            159: 'Eight DIMM-riser slots and SP 3.3V standby rail; not external input rating.',
            171: 'Main 12V distribution, Vertical PDB interconnect, bus bar/ribbon and top-cover interlock; PSUs connect via PSU backplane (unlike ZS3-2 text).',
            183: 'Source-local vertical PDB/Paddle Card description; not a new standalone part identity.',
            193: 'Power supplies 0 and 1 are explicit labels, not a wattage/redundancy specification.',
            201: 'Explicit two fan power boards, FRU service scope, module LEDs/I2C; no fan-module count inferred.',
            210: 'Explicit six-drive backplane; not installed drive count.',
            219: 'Two mirrored boot drives in slots 0/1; read-cache choices remain conditional and constrained by following note.',
            221: 'Read cache must be in either controller or DE2-24 shelf, never both simultaneously; linked software/slot rules not collected.',
            252: 'Source-local 4x4 SAS-2 HBA connectivity and logical port numbering 3-0; not a new HBA part code.',
            257: 'Source statement supports 16GB DDR3 on all eight risers, accommodating 1TB; no unit configuration claim.',
            260: 'D0/D2/D4/D6 required on every riser; optional D1/D3/D5/D7 as a group on all installed risers; identical DIMMs.',
            273: 'Fan-fault introduction; the following three LED blocks belong to this condition.',
            277: 'Fan-fault indication: front and rear Service Action Required LEDs.',
            282: 'Fan-fault indication: front Fan Module Service Action Required TOP LED.',
            288: 'Fan-fault indication: Fan Fault LED on or adjacent to faulty module.',
            292: 'Overtemp LED might light if fan fault raises system operating temperature; conditional, not guaranteed.',
        },
    }
    table_observations = []
    for model, selected in selectors.items():
        article = json.loads((HERE/(model+'-article.json')).read_text())
        for start, boundary in selected.items():
            b = next(b for b in article['blocks'] if b['start_line'] == start)
            findings.append(dict(id=f'{model}-html-{start}', source=model, printed_scope='Oracle ZFS Storage '+model.upper(), source_file=article['source_path'], source_sha256=article['source_sha256'], html_start_line=b['start_line'], html_end_line=b['end_line'], quote=b['text'], boundary=boundary))
        for t in article['tables']:
            table_observations.append(dict(source=model, printed_scope='Oracle ZFS Storage '+model.upper(), source_file=article['source_path'], source_sha256=article['source_sha256'], html_start_line=t['start_line'], html_end_line=t['end_line'], title=t['attrs'].get('title'), summary=t['attrs'].get('summary'), rows=[[c['text'] for c in r['cells']] for r in t['rows']], boundary='Preserve full rows. Unnamed component/drive LED tables are figure legends: leading numerals are labels, not installed quantities. ZS3-2 CPU Descriptions capacity dot remains literal; no repaired value. Part-number table is source-scoped compatibility/service context, not controller alias or manufacturer proof.'))
    result = dict(status='discovery observations only; no curated candidate or import', observations=findings, literal_html_tables=table_observations, absences_and_limits={
        'SD4100': ['No chassis weight, AC frequency/current, exact PSU/fan FRU number or measured appliance consumption printed in this two-page datasheet.', 'No assumption that R4086-F9001-01 is equivalent to every SD4100 configuration or any marketplace unit.', 'No publication/manufacturing date inferred from footer 121224 or PDF metadata.'],
        'ZS3-2': ['No explicit PSU wattage/model/input frequency/current, chassis dimensions/weight, fan-module count or airflow specification in captured article.', 'No claim that legend 12 Power supplies means twelve supplies or legend 9 Fan modules means nine fans.', 'CPU Descriptions table title contrasts with DIMM-placement content; its second Capacity cell is literal dot, not an inferred capacity.'],
        'ZS3-4': ['Full Oracle ZFS Storage ZS3-4 scope remains distinct from registry ZS3-4 pending routing review.', 'No explicit PSU wattage/model/input frequency/current, chassis dimensions/weight, fan-module count or airflow specification in captured article.', 'No remote figures or linked slot/software requirements were fetched. Fan power board count two does not establish fan-module count.'],
        'cross_source': ['3.3V standby and 12V distribution describe internal rails; not AC input voltage.', 'ZS3-2 direct PDB PSU attachment and ZS3-4 PSU-backplane attachment are separate model descriptions, not a conflict requiring normalization.', 'No company manufacture, factory, deployment, current availability, support entitlement or auction-unit configuration inferred.']})
    (HERE/'findings.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({'observations':len(findings),'literal_tables':len(table_observations),'SD4100_observations':sum(f['source']=='SD4100' for f in findings),'ZS3-2_observations':sum(f['source']=='zs3-2' for f in findings),'ZS3-4_observations':sum(f['source']=='zs3-4' for f in findings)}))

if __name__ == '__main__': main()

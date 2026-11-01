"""IP Infusion's public HCL: scoped platform compatibility, not unit entitlement."""
import datetime as dt
import json
import re
from pathlib import Path

from .catalog import target_for
from .documents import dependency
from .fcc_index import Tree

SOURCE = 'ipinfusion-ocnos-hcl'
URL = 'https://www.ipinfusion.com/resources/ocnos-hardware-compatibility-list/'
API = 'https://www.ipinfusion.com/wp-json/ipi/v1/platforms'
HEADERS = ['Platform', 'OcNOS', 'Vendor', 'ASIC', 'Capacity', 'OcNOS version',
           'Form', 'Ports', 'Capabilities']
PORTS = {'g800': '800G', 'g400': '400G', 'g200': '200G', 'g100': '100G',
         'g100qdd': '100G QSFP-DD', 'g25': '25G', 'g10': '10G',
         'g1s': '1G SFP', 'g1b': '1G BASE-T'}


def parse(html, platforms):
    root = Tree(html).root
    canonical = [n.attrs.get('href') for n in root.walk('link') if n.attrs.get('rel') == 'canonical']
    sections = [n.parent for n in root.walk('h2')
                if re.fullmatch(r'OcNOS validated hardware platforms \(\d+\)', n.text())]
    if canonical != [URL] or len(sections) != 1:
        raise ValueError('Unexpected OcNOS HCL page or missing platform section')
    section = sections[0]
    heading = list(section.walk('h2'))[0].text()
    count = int(re.search(r'\((\d+)\)', heading)[1])
    tables = list(section.walk('table'))
    if len(tables) != 1 or [n.text() for n in tables[0].walk('th')] != HEADERS:
        raise ValueError('OcNOS table columns changed')
    rows = [[n.text() for n in tr.walk('td')] for tr in tables[0].walk('tr') if list(tr.walk('td'))]
    if (not isinstance(platforms, list) or not platforms or len(platforms) != count
            or len(rows) != count or any(len(r) != len(HEADERS) for r in rows)):
        raise ValueError('Incomplete OcNOS API or table')
    by_heading = {r[0]: r for r in rows}
    if len(by_heading) != count:
        raise ValueError('Duplicate OcNOS table model')
    seen = set()
    ready = []
    for p in platforms:
        strings = ['id', 'vendor', 'nos', 'asicFull', 'ocnosVer', 'form', 'commercialName',
                   'dimMm', 'dimIn', 'weight', 'ram', 'opTemp', 'power', 'special', 'status', 'datasheet']
        if not isinstance(p, dict) or any(not isinstance(p.get(k), str) for k in strings):
            raise ValueError('Malformed OcNOS platform fields')
        if (not re.fullmatch(r'[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*', p['id'])
                or p['id'] in seen or p['nos'] not in ('SP', 'DC')
                or not p['vendor'] or not p['ocnosVer'] or not p['asicFull']
                or (p['commercialName'] and not re.fullmatch(r'[A-Z]+\d+', p['commercialName']))):
            raise ValueError('Invalid or duplicate OcNOS identity')
        seen.add(p['id'])
        title = p['id'] + (' (' + p['commercialName'] + ')' if p['commercialName'] else '')
        row = by_heading.pop(title, None)
        if row is None or [row[i] for i in (1, 2, 3, 5, 6)] != [
                'OcNOS-' + p['nos'], p['vendor'], p['asicFull'], p['ocnosVer'], p['form']]:
            raise ValueError('OcNOS API/table identity or specification mismatch')
        cap = re.fullmatch(r'(\d+(?:\.\d+)?) (Gbps|Tbps)', row[4])
        if (not cap or type(p.get('cap')) not in (int, float) or p['cap'] <= 0
                or float(cap[1]) * (1000 if cap[2] == 'Tbps' else 1) != p['cap']):
            raise ValueError('OcNOS capacity mismatch')
        ports = p.get('ports')
        if (not isinstance(ports, dict) or set(ports) != set(PORTS)
                or any(type(v) is not int or v < 0 for v in ports.values())):
            raise ValueError('Invalid OcNOS port profile')
        port_text = ', '.join(str(ports[k]) + 'x' + label for k, label in PORTS.items() if ports[k])
        if row[7] != port_text:
            raise ValueError('OcNOS port profile mismatch')
        for key in ('skus', 'firmware', 'roles'):
            if (not isinstance(p.get(key), list)
                    or any(not isinstance(v, str) or not v for v in p[key])):
                raise ValueError('Malformed OcNOS ' + key)
        if not p['skus'] or not isinstance(p.get('flags'), dict) or any(type(v) is not bool for v in p['flags'].values()):
            raise ValueError('Missing OcNOS SKUs or malformed capability flags')
        lifecycle = p.get('lifecycle')
        if lifecycle != []:
            if not isinstance(lifecycle, dict) or set(lifecycle) - {'announce', 'eos', 'eol'}:
                raise ValueError('Unrecognized OcNOS lifecycle fields')
            for date in lifecycle.values():
                if not isinstance(date, str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}', date):
                    raise ValueError('Malformed OcNOS lifecycle date')
                dt.date.fromisoformat(date)
        ready.append((p, dict(zip(HEADERS, row))))
    return ready


def import_bundle(c, path):
    path = Path(path).resolve()
    b = json.loads(path.read_text())
    if b.get('schema_version') != 1 or b.get('url') != URL or b.get('api_url') != API:
        raise ValueError('Unexpected OcNOS bundle')
    html = dependency(path.parent, b['html']).decode('utf-8')
    platforms = json.loads(dependency(path.parent, b['api']))
    ready = parse(html, platforms)  # Validate both captures before replacing any source.
    c.source(SOURCE, URL, 'vendor', 'Factual HCL extracts; IP Infusion retains source rights',
             'All API platforms reconciled with the complete static HTML table; exact target vendors imported. '
             'Software-vendor compatibility assertions and reported specifications only, not physical OEM, deployment, '
             'unit configuration, available software downloads or license transfer. Version, firmware and SKU lists '
             'are snapshot context, not a per-SKU version matrix or minimum firmware requirements. '
             'Approximate power/weight preserved, without treating power as measured consumption or PSU rating. '
             'Lifecycle dates belong to this software-vendor HCL; absent dates do not prove ongoing support. '
             'The API status/maxSpd fields may conflict with dates/port counts and remain raw data only.', path)
    imported = 0
    for p, row in ready:
        target = target_for(p['vendor'])
        if target is None:
            continue
        facts = [('compatibility_software', 'OcNOS-' + p['nos']),
                 ('compatibility_releases', p['ocnosVer']),
                 ('compatibility_asic', p['asicFull']),
                 ('compatibility_ports', row['Ports']),
                 ('compatibility_capacity', row['Capacity']),
                 ('compatibility_capabilities', row['Capabilities']),
                 ('source_platform_heading', row['Platform']),
                 ('software_installation_description', 'IP Infusion HCL lists ' + p['ocnosVer'] +
                  ' for OcNOS-' + p['nos'] + '. Listed SKUs: ' + ', '.join(p['skus']) +
                  '. Snapshot compatibility only; software access and license transfer are not established.')]
        for key, name in [('dimMm', 'compatibility_dimensions_mm'), ('dimIn', 'compatibility_dimensions_in'),
                          ('weight', 'compatibility_weight'), ('ram', 'compatibility_ram'),
                          ('opTemp', 'compatibility_operating_temperature'), ('datasheet', 'vendor_datasheet_url')]:
            if p[key]: facts.append((name, p[key]))
        facts += [('compatibility_sku', v) for v in p['skus']]
        facts += [('compatibility_firmware_context', v) for v in p['firmware']]
        facts.append(('physical_description', 'IP Infusion HCL reports ' + p['form'] + '; ' + p['dimMm'] + '; ' + p['weight']))
        if p['power']:
            facts.append(('power_description', 'IP Infusion HCL reports power: ' + p['power'] +
                          '; measurement conditions and PSU rating are unspecified.'))
        if p['flags'].get('fanless') is True:
            facts.append(('cooling_description', 'IP Infusion HCL marks this platform fanless.'))
        for key, date in (p['lifecycle'] or {}).items():
            facts.append(('ocnos_hardware_' + {'announce': 'announcement_date', 'eos': 'end_of_sale', 'eol': 'end_of_life'}[key], date))
        ids = [('alias', p['commercialName'])] if p['commercialName'] else []
        c.add(SOURCE, p['id'], URL, target, p['id'], target + ' ' + p['id'],
              'network platform', {'api': p, 'html_table': row},
              [(p['vendor'], 'compatibility_partner'), ('IP Infusion', 'software_vendor')], ids, facts)
        imported += 1
    c.finish(SOURCE, len(ready))
    return {'platforms': len(ready), 'imported': imported}

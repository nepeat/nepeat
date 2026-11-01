import os, sys, json, glob, re
sys.path.insert(0, os.path.dirname(__file__))
import lj

RAW = '/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-sonic-pddf'
COMMIT = 'c431b65bf2905ce88d14578a03c68338413b353b'
OUT = os.path.join(RAW, 'sonic-pddf-facts-2026-10-11.json')
EXTR = '/home/erin/nepeat/aislop/devicehunt/research/batches/discovery-2026-10-11/sonic-pddf/extraction.json'
OLD = '/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json'

COVER = ("Software PDDF (Platform Driver Development Framework) declarations at sonic-net/sonic-buildimage commit "
         + COMMIT + " (device/<vendor>/<platform>/pddf/*.json). PDDF declarations are what SONiC's driver framework is told to "
         "expect; they are not a hardware datasheet, installed configuration, vendor support statement or proof of a model's "
         "existence or variant. Counts and names are literal values from the JSON; register addresses/offsets are not recorded; "
         "a fact is omitted when the file lacks it. bmc/nonbmc files are alternative declarations and are listed separately. "
         "Directory/vendor names imply no company role.")


def loc(fn, ln):
    if isinstance(ln, tuple):
        a, b = min(ln), max(ln)
        return '(pddf/%s:%d)' % (fn, a) if a == b else '(pddf/%s:%d-%d)' % (fn, a, b)
    return '(pddf/%s:%d)' % (fn, ln)


def dev_entries(root):
    for k, kl, v in root[2]:
        if v[0] == 'obj' and lj.get(v, 'dev_info') is not None:
            yield k, kl, v


def scalar(node):
    return node[2]


def extract_device(path, fn):
    root = lj.parse(path)
    r = {'file': fn}
    plat = lj.get(root, 'PLATFORM')
    r['counts'] = {}
    for k, kl, v in plat[2]:
        if k.startswith('num_') or k == 'bmc_present':
            r['counts'][k] = (scalar(v), kl)
    dt = lj.get(plat, 'pddf_dev_types')
    d = lj.get(dt, 'description') if dt else None
    if d is not None:
        r['description'] = (d[2], d[1])
    psus, fans, temps, leds, comps, psuattrs = [], [], [], [], [], {}
    for k, kl, v in root[2]:
        if k.startswith('COMPONENT') and v[0] == 'obj':
            ca = lj.get(v, 'comp_attr')
            if ca is not None:
                nm = lj.get(ca, 'name')
                comps.append((k, nm[2], nm[1]))
            continue
        if v[0] != 'obj':
            continue
        di = lj.get(v, 'dev_info')
        if di is None:
            continue
        t = lj.get(di, 'device_type')
        t = t[2] if t else ''
        name = (lj.get(di, 'device_name') or ('str', kl, k))[2]
        if t == 'PSU':
            da = lj.get(v, 'dev_attr')
            dnode = lj.get(di, 'device_name')
            ix = lj.get(da, 'dev_idx') if da else None
            nfp = lj.get(da, 'num_psu_fans') if da else None
            lines = [x[1] for x in (dnode, ix, nfp) if x]
            if dnode is None:
                name = name + ' [object key; no device_name printed]'
                lines.append(kl)
            psus.append((name, tuple(lines) if lines else kl, scalar(ix) if ix else None, scalar(nfp) if nfp else None))
        elif t in ('PSU-PMBUS', 'PSU-EEPROM'):
            i2c = lj.get(v, 'i2c')
            al = lj.get(i2c, 'attr_list') if i2c else None
            for a in (al[2] if al else []):
                an = lj.get(a, 'attr_name')
                if an and an[2] not in psuattrs:
                    psuattrs[an[2]] = (an[1], t)
        elif t == 'FAN':
            i2c = lj.get(v, 'i2c')
            da = lj.get(i2c, 'dev_attr') if i2c else None
            nf = lj.get(da, 'num_fantrays') if da else None
            dnode = lj.get(di, 'device_name')
            fans.append((name, dnode[1] if dnode else kl, scalar(nf) if nf else None, nf[1] if nf else None))
        elif t == 'TEMP_SENSOR':
            da = lj.get(v, 'dev_attr')
            dn = lj.get(da, 'display_name') if da else None
            temps.append((name, dn[2] if dn else None, dn[1] if dn else kl))
        elif t == 'LED':
            dnode = lj.get(di, 'device_name'); leds.append((name, dnode[1] if dnode else kl))
    r.update(psus=psus, fans=fans, temps=temps, leds=leds, comps=comps, psuattrs=psuattrs)
    return r


def leaves(node, path=()):
    if node[0] == 'obj':
        for k, kl, v in node[2]:
            yield from leaves(v, path + (k,))
    elif node[0] in ('str', 'num', 'lit'):
        yield path, node[2], node[1]


def extract_plugin(path, fn):
    root = lj.parse(path)
    r = {'file': fn, 'names': {}, 'limits': [], 'dirmaps': [], 'modelkeys': []}
    psu = lj.get(root, 'PSU')
    fan = lj.get(root, 'FAN')
    for sect, key, label in ((psu, 'name', 'psu_names'), (psu, 'fan_name', 'psu_fan_names'),
                             (psu, 'thermal_name', 'psu_thermal_names'),
                             (fan, 'drawer_name', 'fan_drawer_names'), (fan, 'name', 'fan_names')):
        n = lj.get(sect, key) if sect else None
        if n is not None:
            r['names'][label] = [('.'.join(p), v, ln) for p, v, ln in leaves(n)]
    for sect, nm in ((psu, 'PSU'), (fan, 'FAN')):
        if not sect:
            continue
        for p, v, ln in leaves(sect, (nm,)):
            last = p[-1]
            if re.search(r'MAX.*(SPEED|RPM)|(SPEED|RPM).*MAX', '/'.join(p)) and isinstance(v, (int, str)):
                r['limits'].append(('.'.join(p), v, ln))
    d = lj.get(psu, 'psu_fan_dir') if psu else None
    if d is not None:
        for p, v, ln in leaves(d, ('PSU', 'psu_fan_dir')):
            tok = p[-1]
            if tok in ('0', '1', 'F2B', 'B2F'):
                r['dirmaps'].append(('.'.join(p[:-1]), tok, v, ln))
            else:
                r['modelkeys'].append((tok, v, ln, '.'.join(p[:-1])))
    d = lj.get(fan, 'direction') if fan else None
    if d is not None:
        for p, v, ln in leaves(d, ('FAN', 'direction')):
            r['dirmaps'].append(('.'.join(p[:-1]), p[-1], v, ln))
    return r


def main():
    old = {x['model']: x for x in json.load(open(OLD))['devices']}
    plats = {}
    for f in sorted(glob.glob(RAW + '/device/*/*/pddf/*.json')):
        parts = f.split('/')
        plat, vendor, fn = parts[-3], parts[-4], parts[-1]
        p = plats.setdefault(plat, {'vendor': vendor, 'dev': [], 'plug': None})
        if fn.startswith('pddf-device'):
            p['dev'].append(extract_device(f, fn))
        elif fn == 'pd-plugin.json':
            p['plug'] = extract_plugin(f, fn)
    sources, devices, extraction = [], [], {}
    conflicts = []
    for plat, p in sorted(plats.items()):
        facts = []
        base = 'device/%s/%s/pddf/' % (p['vendor'], plat)
        facts.append(('pddf_commit', COMMIT))
        facts.append(('pddf_vendor_dir_as_printed', p['vendor']))
        files = [d['file'] for d in p['dev']] + (['pd-plugin.json'] if p['plug'] else [])
        facts.append(('pddf_files', json.dumps(files)))
        for key in ('num_psus', 'num_fantrays', 'num_fans_pertray', 'num_ports', 'num_temps', 'num_components', 'bmc_present'):
            vals = [(d['file'], d['counts'][key]) for d in p['dev'] if key in d['counts']]
            if vals:
                facts.append(('pddf_' + key, '; '.join('%s %s' % (v[0], loc(fn, v[1])) for fn, v in vals)))
        descs = []
        for d in p['dev']:
            if 'description' in d and d['description'][0] not in [x[0] for x in descs]:
                descs.append((d['description'][0], d['file'], d['description'][1]))
        if descs:
            facts.append(('pddf_dev_types_description', '; '.join('%r %s' % (t.strip(), loc(fn, ln)) for t, fn, ln in descs)))
        def per_file(name, fmt_key, items_fn, cap=40):
            out = []
            for d in p['dev']:
                items = items_fn(d)
                if items:
                    out.append('%s: %s' % (d['file'], ', '.join(items[:cap]) + (' ... (+%d more)' % (len(items) - cap) if len(items) > cap else '')))
            if out:
                facts.append((name, ' | '.join(out)))
        per_file('pddf_psu_devices', None, lambda d: ['%s (dev_idx=%s, num_psu_fans=%s) %s' % (n, i, nf, loc(d['file'], ln)) for n, ln, i, nf in d['psus']])
        per_file('pddf_fan_ctrl_devices', None, lambda d: ['%s%s %s' % (n, '' if nf is None else ' (num_fantrays=%s)' % nf, loc(d['file'], (ln, nl) if nl else ln)) for n, ln, nf, nl in d['fans']])
        per_file('pddf_temp_sensor_display_names', None, lambda d: ['%s [%s] %s' % (dn, n, loc(d['file'], ln)) for n, dn, ln in d['temps'] if dn])
        per_file('pddf_temp_sensor_declared_count', None, lambda d: [str(len(d['temps']))])
        per_file('pddf_led_device_names', None, lambda d: ['%s %s' % (n, loc(d['file'], ln)) for n, ln in d['leds']])
        per_file('pddf_component_names', None, lambda d: ['%s=%s %s' % (k, nm, loc(d['file'], ln)) for k, nm, ln in d['comps']])
        # psu attrs names (names only)
        pa = {}
        for d in p['dev']:
            for a, (ln, t) in d['psuattrs'].items():
                pa.setdefault(a, (d['file'], ln))
        if pa:
            facts.append(('pddf_psu_declared_attr_names',
                          'attr names only, register map not recorded: ' + ', '.join('%s %s' % (a, loc(fn, ln)) for a, (fn, ln) in pa.items())))
        pl = p['plug']
        if pl:
            for lab, items in pl['names'].items():
                facts.append(('pddf_plugin_' + lab, ', '.join('%s=%r %s' % (k, v, loc('pd-plugin.json', ln)) for k, v, ln in items)))
            if pl['limits']:
                facts.append(('pddf_plugin_max_speed_limits', ', '.join('%s=%s %s' % (k, v, loc('pd-plugin.json', ln)) for k, v, ln in pl['limits'])))
            if pl['modelkeys']:
                facts.append(('pddf_plugin_psu_model_keys_in_fan_dir_map',
                              ', '.join('%r->%r %s' % (k, v, loc('pd-plugin.json', ln)) for k, v, ln, _ in pl['modelkeys'])
                              + " (keys of the plugin's PSU.psu_fan_dir.i2c.valmap lookup table, as printed: PSU model strings the plugin can map to an airflow label; a lookup table in the plugin, NOT a statement about an installed or shipped PSU; not interpreted)"))
            if pl['dirmaps']:
                facts.append(('pddf_plugin_airflow_valmaps',
                              ', '.join('%s[%s]=%r %s' % (k, t, v, loc('pd-plugin.json', ln)) for k, t, v, ln in pl['dirmaps'])))
        # conflicts vs existing sonic candidate (platform.json) and NOS
        o = old.get(plat)
        oldf = dict(o['facts']) if o else {}
        c = {}
        for d in p['dev']:
            cn = d['counts']
            chk = []
            if 'sonic_psu_count' in oldf and 'num_psus' in cn and str(cn['num_psus'][0]) != oldf['sonic_psu_count']:
                chk.append('num_psus %s vs sonic_psu_count %s' % (cn['num_psus'][0], oldf['sonic_psu_count']))
            if 'sonic_fan_drawer_count' in oldf and 'num_fantrays' in cn and str(cn['num_fantrays'][0]) != oldf['sonic_fan_drawer_count']:
                chk.append('num_fantrays %s vs sonic_fan_drawer_count %s' % (cn['num_fantrays'][0], oldf['sonic_fan_drawer_count']))
            if 'sonic_drawer_fan_count' in oldf and 'num_fantrays' in cn and 'num_fans_pertray' in cn and cn['num_fantrays'][0] * cn['num_fans_pertray'][0] != int(oldf['sonic_drawer_fan_count']):
                chk.append('fantrays*fans_pertray %d vs sonic_drawer_fan_count %s' % (cn['num_fantrays'][0] * cn['num_fans_pertray'][0], oldf['sonic_drawer_fan_count']))
            if 'sonic_thermal_count' in oldf and 'num_temps' in cn and str(cn['num_temps'][0]) != oldf['sonic_thermal_count']:
                chk.append('num_temps %s vs sonic_thermal_count %s' % (cn['num_temps'][0], oldf['sonic_thermal_count']))
            if d['temps'] and 'num_temps' in cn and len(d['temps']) != cn['num_temps'][0]:
                chk.append('num_temps %s vs %d TEMP_SENSOR entries in same file' % (cn['num_temps'][0], len(d['temps'])))
            if len(d['psus']) != cn.get('num_psus', (len(d['psus']),))[0]:
                chk.append('num_psus %s vs %d PSU entries same file' % (cn['num_psus'][0], len(d['psus'])))
            if chk:
                conflicts.append((plat, d['file'], chk))
        devices.append({'source': 'sonic-pddf-' + plat, 'key': plat, 'brand': 'SONiC platform', 'model': plat,
                        'title': 'SONiC platform ' + plat, 'kind': 'network switch software platform definition',
                        'affiliations': [], 'identifiers': [['sonic_platform', plat]], 'facts': [list(f) for f in facts]})
        prim = (['pddf-device.json'] if any(d['file'] == 'pddf-device.json' for d in p['dev']) else ['pddf-device-nonbmc.json'])[0]
        sources.append({'id': 'sonic-pddf-' + plat,
                        'url': 'https://github.com/sonic-net/sonic-buildimage/tree/%s/device/%s/%s/pddf' % (COMMIT, p['vendor'], plat),
                        'tier': 'project', 'license': 'Apache-2.0 / upstream rights; factual extraction',
                        'coverage': COVER, 'snapshot': base + prim})
        extraction[plat] = {'vendor': p['vendor'], 'facts': facts, 'in_old_candidate': plat in old}
    json.dump({'sources': sources, 'devices': devices}, open(OUT, 'w'), indent=1)
    json.dump({'conflicts': conflicts, 'platforms': extraction}, open(EXTR, 'w'), indent=1)
    print(len(sources), len(devices), sum(len(d['facts']) for d in devices))
    for c in conflicts:
        print(c)


main()

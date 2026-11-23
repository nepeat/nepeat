#!/usr/bin/env python3
"""Offline stand-in for the switch side of sonic-test-lib.sh: redis-cli (one JSON file, all databases), the SONiC
managers' STATE_DB entries and the kernel state they make (ip, bridge, sysctl), swssconfig and ping. Called by the
name of the tool it replaces (symlinks made by run.sh). State: $FAKE_STATE (JSON).

Manager model (from sonic-swss cfgmgr/intfmgr.cpp, vrfmgr.cpp, vlanmgr.cpp), applied after every write until stable:
  APPL_DB INTF_TABLE / VRF_TABLE / VLAN_TABLE / VLAN_MEMBER_TABLE mirror the applied entries (orchagent popped them).
  INTERFACE|p: applied (STATE INTERFACE_TABLE|p, vrf=<vrf_name>) once its VRF is applied; a changed vrf_name on an
    applied entry is refused ("can not change ... directly", logged); a delete waits until p has no address.
  INTERFACE|p|a: applied once INTERFACE|p is.   VRF|v: applied at once; a delete waits while an interface uses it.
  VLAN|v: applied at once.   VLAN_MEMBER|v|p: applied once VLAN|v is; p is then in the kernel bridge.
Kernel = the applied entries, plus 'stuck' leftovers (state['stuck']) that only ip link del / ip addr del remove.
"""
import json, os, sys, fnmatch, time

F = os.environ['FAKE_STATE']


def load():
    with open(F) as f:
        return json.load(f)


def save(s):
    tmp = F + '.tmp'
    with open(tmp, 'w') as f:
        json.dump(s, f, indent=1, sort_keys=True)
    os.replace(tmp, F)


def log(s, msg):
    with open(F + '.managers.log', 'a') as f:
        f.write(msg + '\n')


def managers(s):
    c, st = s['db']['4'], s['db']['6']
    for _ in range(10):
        before = json.dumps(st, sort_keys=True)
        for k in list(c):
            t, _, rest = k.partition('|')
            if t == 'VRF':
                st['VRF_TABLE|' + rest] = {'t': 'hash', 'v': {'state': 'ok'}}
            elif t == 'VLAN':
                st['VLAN_TABLE|' + rest] = {'t': 'hash', 'v': {'state': 'ok'}}
            elif t == 'VLAN_MEMBER':
                v = rest.split('|')[0]
                if 'VLAN_TABLE|' + v in st:
                    st['VLAN_MEMBER_TABLE|' + rest] = {'t': 'hash', 'v': {'state': 'ok'}}
            elif t == 'INTERFACE' and '|' not in rest:
                vrf = c[k]['v'].get('vrf_name', '')
                sk = 'INTERFACE_TABLE|' + rest
                if sk in st and st[sk]['v'].get('vrf', '') != vrf:
                    log(s, '%s can not change to %s directly, skipping' % (rest, vrf))
                elif not vrf or 'VRF_TABLE|' + vrf in st:
                    st[sk] = {'t': 'hash', 'v': {'vrf': vrf}}
            elif t == 'INTERFACE':
                p = rest.split('|')[0]
                if 'INTERFACE_TABLE|' + p in st:
                    st['INTERFACE_TABLE|' + rest] = {'t': 'hash', 'v': {'state': 'ok'}}
        for sk in list(st):
            t, _, rest = sk.partition('|')
            if t == 'VRF_TABLE' and 'VRF|' + rest not in c:
                if not any(v['v'].get('vrf') == rest for k2, v in st.items() if k2.startswith('INTERFACE_TABLE|') and k2.count('|') == 1):
                    del st[sk]
            elif t == 'VLAN_TABLE' and 'VLAN|' + rest not in c:
                if not any(k2.startswith('VLAN_MEMBER_TABLE|%s|' % rest) for k2 in st):
                    del st[sk]
            elif t == 'VLAN_MEMBER_TABLE' and 'VLAN_MEMBER|' + rest not in c:
                del st[sk]
            elif t == 'INTERFACE_TABLE' and 'INTERFACE|' + rest not in c:
                if '|' in rest or not any(k2.startswith(sk + '|') for k2 in st):
                    del st[sk]
        if json.dumps(st, sort_keys=True) == before:
            break
    a = s['db']['0']   # APPL_DB: what orchagent popped (instantly here)
    m = {'INTERFACE_TABLE': 'INTF_TABLE', 'VRF_TABLE': 'VRF_TABLE', 'VLAN_TABLE': 'VLAN_TABLE', 'VLAN_MEMBER_TABLE': 'VLAN_MEMBER_TABLE'}
    for k in [k for k in a if k.split(':')[0] in m.values()]:
        del a[k]
    for sk in st:
        t, _, rest = sk.partition('|')
        if t in m:
            a[m[t] + ':' + rest.replace('|', ':')] = {'t': 'hash', 'v': {'popped': '1'}}
    vlans = kernel(s)['vlan']   # a netdev's sysctls go with it
    for k in [k for k in s.get('sysctl', {}) if k.startswith('net.ipv6.conf.Vlan') and k.split('.')[3] not in vlans]:
        del s['sysctl'][k]


def kernel(s):
    st = s['db']['6']
    k = {'master': {}, 'inet': {}, 'vrf': set(), 'vlan': set()}
    for sk, v in st.items():
        t, _, rest = sk.partition('|')
        if t == 'VRF_TABLE':
            k['vrf'].add(rest)
        elif t == 'VLAN_TABLE':
            k['vlan'].add(rest)
        elif t == 'VLAN_MEMBER_TABLE':
            k['master'][rest.split('|')[1]] = 'Bridge'
        elif t == 'INTERFACE_TABLE' and '|' not in rest and v['v'].get('vrf'):
            k['master'][rest] = v['v']['vrf']
        elif t == 'INTERFACE_TABLE' and '|' in rest:
            p, a = rest.split('|', 1)
            k['inet'].setdefault(p, set()).add(a)
    for x in s.get('stuck', []):
        kind, *a = x.split()
        if kind == 'vrf': k['vrf'].add(a[0])
        if kind == 'vlan': k['vlan'].add(a[0])
        if kind == 'addr': k['inet'].setdefault(a[0], set()).add(a[1])
        if kind == 'master': k['master'][a[0]] = a[1]
    return k


def redis(argv):
    s = load()
    db, raw, args, i = '0', False, [], 0
    while i < len(argv):
        a = argv[i]
        if a == '-s': i += 2; continue
        if a == '-n': db = argv[i + 1]; i += 2; continue
        if a == '--raw': raw = True; i += 1; continue
        args = argv[i:]; break
    d = s['db'].setdefault(db, {})
    cmd, rest = args[0].lower(), args[1:]
    out, write = [], False
    if cmd == '--scan':
        pat = rest[rest.index('--pattern') + 1] if '--pattern' in rest else '*'
        out = sorted(k for k in d if fnmatch.fnmatchcase(k, pat.replace('[', '[[]')))
    elif cmd == 'keys':
        out = sorted(k for k in d if fnmatch.fnmatchcase(k, rest[0]))
    elif cmd == 'ping':
        out = ['PONG']
    elif cmd == 'type':
        out = [d[rest[0]]['t'] if rest[0] in d else 'none']
    elif cmd == 'exists':
        out = [str(sum(1 for k in rest if k in d))]
    elif cmd == 'get':
        out = [d[rest[0]]['v'] if rest[0] in d and d[rest[0]]['t'] == 'string' else '']
    elif cmd == 'hget':
        out = [d.get(rest[0], {'v': {}})['v'].get(rest[1], '')]
    elif cmd == 'hgetall':
        for f, v in d.get(rest[0], {'v': {}})['v'].items():
            out += [f, v]
    elif cmd == 'hset':
        if len(rest) < 3 or len(rest) % 2 == 0:
            print('(error) ERR wrong number of arguments for hset'); return 1
        h = d.setdefault(rest[0], {'t': 'hash', 'v': {}})['v']
        n = 0
        for f, v in zip(rest[1::2], rest[2::2]):
            n += f not in h; h[f] = v
        out, write = [str(n)], True
    elif cmd == 'hdel':
        h = d.get(rest[0], {'v': {}})['v']
        n = sum(1 for f in rest[1:] if h.pop(f, None) is not None)
        if rest[0] in d and not h: del d[rest[0]]
        out, write = [str(n)], True
    elif cmd == 'del':
        n = sum(1 for k in rest if d.pop(k, None) is not None)
        out, write = [str(n)], True
    else:
        print('(error) fake redis-cli: %s not supported' % cmd); return 1
    if write:
        with open(F + '.writes.log', 'a') as f:
            f.write('%s %s\n' % (db, ' '.join(args)))
        managers(s)
        save(s)
    for o in out:
        print(o)
    return 0


def ip(argv):
    s = load(); k = kernel(s); a = argv
    def opt(name):
        return a[a.index(name) + 1] if name in a else None
    if a[:1] == ['vrf'] and a[1:2] == ['exec']:
        os.execvp(a[3], a[3:])
    if 'neigh' in a:
        return 0
    if 'link' in a and 'show' in a:
        typ, dev = opt('type'), opt('dev')
        if typ == 'vrf':
            for i, v in enumerate(sorted(k['vrf'])): print('%d: %s: <NOARP,MASTER,UP,LOWER_UP> mtu 65575' % (50 + i, v))
            return 0
        if typ == 'vlan':
            for i, v in enumerate(sorted(k['vlan'])): print('%d: %s@Bridge: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 9100' % (60 + i, v))
            return 0
        if dev:
            m = k['master'].get(dev)
            print('5: %s: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 9100 qdisc mq %sstate UP' % (dev, 'master %s ' % m if m else ''))
        return 0
    if 'addr' in a and 'show' in a:
        dev = opt('dev')
        for x in sorted(k['inet'].get(dev, ())):
            print('5: %s    inet %s scope global %s\\       valid_lft forever' % (dev, x, dev))
        return 0
    stuck = s.get('stuck', [])
    if 'addr' in a and 'del' in a:
        e = 'addr %s %s' % (opt('dev'), a[a.index('del') + 1])
    elif 'link' in a and 'del' in a:
        e = next((x for x in stuck if x.split()[1] == opt('dev') and x.split()[0] in ('vrf', 'vlan')), None)
    elif 'nomaster' in a:
        e = next((x for x in stuck if x.startswith('master %s ' % opt('dev'))), None)
    else:
        return 0
    if e in stuck:
        stuck.remove(e); save(s)
        with open(F + '.forced.log', 'a') as f: f.write(' '.join(a) + '\n')
        return 0
    print('RTNETLINK answers: No such device or address'); return 2


def sysctl(argv):
    s = load(); sc = s.setdefault('sysctl', {})
    if '-n' in argv:
        k = argv[-1]
        if k not in sc: return 255
        print(sc[k]); return 0
    if '-w' in argv:
        k, v = argv[-1].split('=', 1)
        if k.startswith('net.ipv6.conf.Vlan'):   # netdev of an applied VLAN only
            if k.split('.')[3] not in kernel(s)['vlan']: return 255
        sc[k] = v; save(s); return 0
    return 0


def swssconfig(argv):
    s = load(); d = s['db'].setdefault('0', {})
    for e in json.load(open(argv[0])):
        op = e.pop('OP'); (k, v), = e.items()
        if op == 'SET': d[k] = {'t': 'hash', 'v': v}
        else: d.pop(k, None)
    save(s); return 0


def ping(argv):
    m = os.environ.get('FAKE_HANG_MARKER')
    if m and not os.path.exists(m):
        open(m, 'w').close(); time.sleep(float(os.environ.get('FAKE_HANG_SECONDS', '60')))
    x = os.environ.get('FAKE_PING_DOES')   # once: 'stuck <item>' (a kernel leftover) or 'hset <key> <f> <v>' (a foreign write)
    if x and not os.path.exists(F + '.pingdid'):
        open(F + '.pingdid', 'w').close()
        s = load(); w = x.split()
        if w[0] == 'stuck': s['stuck'].append(' '.join(w[1:]))
        if w[0] == 'hset': s['db']['4'].setdefault(w[1], {'t': 'hash', 'v': {}})['v'][w[2]] = w[3]
        save(s)
    n = argv[argv.index('-c') + 1] if '-c' in argv else '1'
    if '-q' in argv:
        print('%s packets transmitted, %s received, 0%% packet loss, time 1ms\nrtt min/avg/max/mdev = 0.1/0.1/0.1/0.0 ms' % (n, n))
    return 0


def bridge(argv):
    return 0


tool = os.path.basename(sys.argv[0])
sys.exit({'redis-cli': redis, 'ip': ip, 'sysctl': sysctl, 'swssconfig': swssconfig, 'ping': ping, 'bridge': bridge}[tool](sys.argv[1:]))

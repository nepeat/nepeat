#!/usr/bin/env python3
"""switchd_dryrun.py - offline dry-run generator for a CS8210-32X "mini-switchd".

Takes a small JSON desired state (guard / lags / bridges) and the vendor port
spec string (the same string cs8210-switch-up passes to auto_load_user.sh -m),
and prints the ordered command sequence for APPLY and for TEARDOWN.

NOTHING here talks to hardware. It only reads the JSON, and (for validation)
the extracted vendor SDK sources, which stay where they are (never copied).

Two kinds of operation come out, because the vendor CLI has no "create" verbs
(cmds/ifcscmd.py: ifcs sub-commands are show/enable/disable/clear/set/list):

  cli  a line for `cls_shell '<cmd>'` (attribute sets, port enable/disable,
       show/check commands). Validated against the vendor CLI's own argparse
       definitions, which are extracted from ifcs_cmds/*.py at run time and
       executed under Python 3.
  api  an ifcs_* C API call (object create/delete/member add/remove). These
       have to run inside innovium.user as Python 2, the way
       `diagtest knet config` does. Validated against pyctypes/ifcs_ctypes.py
       (function exists, argument count, every IFCS_* constant exists).
       --emit-py2 writes a module that performs the whole sequence through
       `cls_shell 'run <module> apply|teardown'`.

Confidence tags on every operation:
  V  form verified from vendor source (the vendor code does exactly this)
  I  inferred from API signatures / strings; semantics not confirmed
  U  unknown; needs hardware to confirm (may be unnecessary or rejected)

Usage:
  switchd_dryrun.py STATE.json --ports '<spec>' [--sdk DIR] [--emit-py2 FILE]
  switchd_dryrun.py --selftest [--sdk DIR]
Exit status: 0 ok, 1 validation failure, 2 bad input.
"""
import argparse
import contextlib
import io
import json
import os
import re
import sys
import textwrap

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SDK = os.environ.get('CS8210_SDK_SRC') or os.path.normpath(os.path.join(
    HERE, '..', 'analysis', 'onie-live-2026-11-10', 'sdk',
    'R3181-J0001-01_V0.1.7_Briggs_SDK'))

# Lanes per logical port for each breakout mode (dynamic_config.sh `dic`).
# Cross-checked against the vendor script by --selftest when the SDK is there.
LANES = {'1x400': 8, '1x200': 8, '1x100': 4, '1x40': 4, '2x200': 4,
         '2x100': 4, '4x100': 2, '4x25': 1, '8x50': 1, '4x10': 1}
CPU_DEVPORT = 0          # dynamic_config.sh: devport 0, sysport 1000, type cpu
LINK_LOCAL = ['01:80:c2:00:00:00', '01:80:c2:00:00:02', '01:80:c2:00:00:0e']


class InputError(Exception):
    pass


# --------------------------------------------------------------------------
# Port spec -> devport / sysport / netdev (dynamic_config.sh + knet.py logic)
# --------------------------------------------------------------------------
class Port(object):
    def __init__(self, devport, cage, sub, nbreak, speed, lanes, lane0, media,
                 admin_up):
        self.devport = devport
        self.sysport = devport           # dynamic_config.sh: sysport "$lport"
        self.cage = cage
        self.sub = sub                   # 1-based breakout index
        self.nbreak = nbreak
        self.speed = speed
        self.lanes = lanes
        self.lane0 = lane0
        self.media = media
        self.admin_up = admin_up
        # knet.py devPort2fpp(): fpp<cage> or fpp<cage>-<k>
        self.netdev = 'fpp%d' % cage if nbreak == 1 else 'fpp%d-%d' % (cage, sub)
        self.knet_vlan = devport + 1     # knet.py config(): vlan = i + 1


def split_entry(raw):
    """One ';' entry -> (first, last, media, nbreak, speed, opts)."""
    e = raw.strip().lower().replace('g', '')      # tr A-Z a-z | sed s/g//g
    f = e.split(':')
    if len(f) < 2 or not f[0] or not f[1]:
        raise InputError('bad port spec entry %r' % raw)
    rng = f[0].split('-')
    try:
        first = int(rng[0])
        last = int(rng[1]) if len(rng) == 2 else first
    except ValueError:
        raise InputError('bad cage range in %r' % raw)
    mode = f[1].split('_')
    if len(mode) == 2:
        media, sp = mode
    else:
        media, sp = 'optics', mode[0]
    if 'x' in sp:
        n, speed = sp.split('x', 1)
    else:
        n, speed = '1', sp
    if not n.isdigit() or not speed.isdigit():
        raise InputError('bad mode in %r' % raw)
    return first, last, media, int(n), speed, f[2:4]


def parse_port_spec(spec):
    ports, warnings, seen = [], [], set()
    lport = 1
    for raw in spec.split(';'):
        if not raw.strip():
            continue
        first, last, media, n, speed, opts = split_entry(raw)
        key = '%dx%s' % (n, speed)
        if key not in LANES:
            raise InputError('breakout mode %s (%r) is not in the vendor table'
                             % (key, raw))
        if media not in ('copper', 'optics'):
            warnings.append('%r: knet.py devPort2fpp() only matches copper|'
                            'optics; media %r would make knet config fail'
                            % (raw, media))
        ifadmin = 'up-all'
        for o in opts:                    # vendor: any option containing "p"
            if 'p' in o:
                ifadmin = o
        for cage in range(first, last + 1):
            if cage in seen or not 1 <= cage <= 32:
                raise InputError('cage %d repeated or out of range' % cage)
            seen.add(cage)
            for k in range(1, n + 1):
                down = 'down' in ifadmin and (ifadmin == 'down-pall'
                                              or ('p%d' % k) in ifadmin)
                ports.append(Port(lport, cage, k, n, speed, LANES[key],
                                  (k - 1) * LANES[key], media, not down))
                lport += 1
    if sorted(seen) != list(range(1, 33)):
        warnings.append('spec does not cover cages 1-32 exactly once in order;'
                        ' devport numbering follows the order written')
    if [p.cage for p in ports] != sorted(p.cage for p in ports):
        warnings.append('cages are not ascending; knet netdev names follow '
                        'spec order, check them')
    if len(ports) > 128:
        raise InputError('%d logical ports (max 128)' % len(ports))
    return ports, warnings


def guard_spec(spec):
    """Same spec with every port admin-down (vendor option down-pall)."""
    out = []
    for raw in spec.split(';'):
        if not raw.strip():
            continue
        f = raw.strip().split(':')
        keep = f[:2] + [o for o in f[2:4] if 'fec' in o.lower()]
        out.append(':'.join(keep + ['down-pall']))
    return ';'.join(out)


# --------------------------------------------------------------------------
# Operations
# --------------------------------------------------------------------------
class E(object):
    """A handle/value expression: human form and Python 2 form."""
    def __init__(self, text, py):
        self.text, self.py = text, py


def sp(d):
    if d == CPU_DEVPORT:
        return E('sysport(cpu)', 'SP(0)')
    return E('sysport(dp%d)' % d, 'SP(%d)' % d)


def l2vni(v):
    return E('L2VNI(%d)' % v, 'IFCS_HANDLE_L2VNI(%d)' % v)


def st(name):
    return E('$' + name, "S['%s']" % name)


def kmdg(d):
    return E('Knet.mdg_hdl[%d]' % d, 'Knet.mdg_hdl[%d]' % d)


def kmcd(d):
    return E('Knet.mcdest_hdl[%d]' % d, 'Knet.mcdest_hdl[%d]' % d)


KSTP = E('Knet.stp_hdl', 'Knet.stp_hdl')


def const(name):
    return E(name, name)


def num(n):
    return E(str(n), str(n))


ARGC = {'create': 4, 'delete': 2, 'madd': 6, 'mrem': 4, 'pset': 5,
        'l2add': 4, 'l2del': 2, 'aset': 4}


class Op(object):
    def __init__(self, kind, text, conf, note='', py=None, func=None,
                 form=None, attrs=(), optional=False):
        self.kind, self.text, self.conf, self.note = kind, text, conf, note
        self.py, self.func, self.form = py, func, form
        self.attrs, self.optional = attrs, optional
        self.result = None


def _attr_text(attrs):
    return '[%s]' % ', '.join('%s=%s' % (a, v.text) for a, _f, v in attrs)


def _attr_py(attrs):
    return '[%s]' % ', '.join("(%s, '%s', %s)" % (a, f, v.py)
                              for a, f, v in attrs)


def _opt(optional):
    return ', optional=True' if optional else ''


def api_create(func, out, conf, note='', attrs=(), preset=None, optional=False,
               out_py=None):
    n = len(attrs)
    lhs = ('$%s = ' % out) if out else ''
    text = '%s%s(0, &%s, %d, %s)' % (
        lhs, func, preset.text if preset else 'h', n,
        _attr_text(attrs) if n else 'NULL')
    target = out_py or ("S['%s']" % out if out else '_')
    py = "%s = create(%r, %s, %s, %s%s)" % (
        target, text, func, _attr_py(attrs), preset.py if preset else 'None',
        _opt(optional))
    return Op('api', text, conf, note, py, func, 'create', attrs, optional)


def api_delete(func, h, conf, note='', optional=False):
    text = '%s(0, %s)' % (func, h.text)
    py = 'delete(%r, %s, %s%s)' % (text, func, h.py, _opt(optional))
    return Op('api', text, conf, note, py, func, 'delete', (), optional)


def api_madd(func, h, members, conf, note='', attrs=(), optional=False):
    text = '%s(0, %s, %d, [%s], %d, %s)' % (
        func, h.text, len(members), ', '.join(m.text for m in members),
        len(attrs), _attr_text(attrs) if attrs else 'NULL')
    py = 'madd(%r, %s, %s, [%s], %s%s)' % (
        text, func, h.py, ', '.join(m.py for m in members), _attr_py(attrs),
        _opt(optional))
    return Op('api', text, conf, note, py, func, 'madd', attrs, optional)


def api_mrem(func, h, members, conf, note='', optional=False):
    text = '%s(0, %s, %d, [%s])' % (
        func, h.text, len(members), ', '.join(m.text for m in members))
    py = 'mrem(%r, %s, %s, [%s]%s)' % (
        text, func, h.py, ', '.join(m.py for m in members), _opt(optional))
    return Op('api', text, conf, note, py, func, 'mrem', (), optional)


def api_pset(func, h, port, attrs, conf, note='', optional=False):
    text = '%s(0, %s, %s, %d, %s)' % (func, h.text, port.text, len(attrs),
                                      _attr_text(attrs))
    py = 'pset(%r, %s, %s, %s, %s%s)' % (text, func, h.py, port.py,
                                         _attr_py(attrs), _opt(optional))
    return Op('api', text, conf, note, py, func, 'pset', attrs, optional)


def api_l2add(mac, vlan, dest, conf, note='', optional=False):
    attrs = (('IFCS_L2_ENTRY_ATTR_ENTRY_DEST', 'handle', dest),
             ('IFCS_L2_ENTRY_ATTR_ENTRY_TYPE', 'u32',
              const('IFCS_L2_ENTRY_TYPE_STATIC')))
    text = 'ifcs_l2_entry_create(0, key{mac=%s, l2vni=%d}, 2, %s)' % (
        mac, vlan, _attr_text(attrs))
    py = "l2add(%r, '%s', %d, %s%s)" % (text, mac, vlan, _attr_py(attrs),
                                        _opt(optional))
    return Op('api', text, conf, note, py, 'ifcs_l2_entry_create', 'l2add',
              attrs, optional)


def api_l2del(mac, vlan, conf, note='', optional=False):
    text = 'ifcs_l2_entry_delete(0, key{mac=%s, l2vni=%d})' % (mac, vlan)
    py = "l2del(%r, '%s', %d%s)" % (text, mac, vlan, _opt(optional))
    return Op('api', text, conf, note, py, 'ifcs_l2_entry_delete', 'l2del',
              (), optional)


def cli(cmd, conf, note='', py=None):
    return Op('cli', cmd, conf, note, py)


def cli_sysport(d, attr, value, conf, note=''):
    typ = {'default_cvid': 'u16', 'egress_untagged_vlan_id': 'u16'}.get(
        attr, 'u32')
    py = "aset(%r, ifcs_sysport_attr_set, SP(%d), [(IFCS_SYSPORT_ATTR_%s, '%s', %d)])" % (
        'sysport dp%d %s=%d' % (d, attr, value), d, attr.upper(), typ, value)
    return cli('ifcs set sysport %d %s %d' % (d, attr, value), conf, note, py)


def compress(devports):
    """[2,3,4,9] -> '2-4,9' (port.py enable() accepts a,b-c lists)."""
    out, ds = [], sorted(set(devports))
    i = 0
    while i < len(ds):
        j = i
        while j + 1 < len(ds) and ds[j + 1] == ds[j] + 1:
            j += 1
        out.append(str(ds[i]) if i == j else '%d-%d' % (ds[i], ds[j]))
        i = j + 1
    return ','.join(out)


def cli_port(action, devports, conf, note=''):
    lst = compress(devports)
    py = 'admin(%r, [%s], %s)' % (
        'port %s %s' % (action, lst), ', '.join(str(d) for d in sorted(set(devports))),
        'IFCS_BOOL_TRUE' if action == 'enable' else 'IFCS_BOOL_FALSE')
    return cli('port %s %s' % (action, lst), conf, note, py)


def pre(text, conf, note=''):
    return Op('pre', text, conf, note)


# --------------------------------------------------------------------------
# Plan
# --------------------------------------------------------------------------
STP = 'IFCS_STP_PORT_ATTR_STP_STATE'


def stp_state(name):
    return ((STP, 'u32', const('IFCS_STP_PORT_STATE_' + name)),)


def pvid_ops(d, vlan, conf, note):
    return [cli_sysport(d, 'default_cvid', vlan, conf, note),
            cli_sysport(d, 'egress_untagged_vlan_id', vlan, conf),
            cli_sysport(d, 'egress_untagged_vlan_id_enable', 1, conf)]


def build_plan(state, spec, ports):
    by_name = dict((p.netdev, p) for p in ports)
    nfront = len(ports)
    knet_vlans = (2, nfront + 1)
    guard = bool(state.get('guard', True))
    lags = state.get('lags', [])
    bridges = state.get('bridges', [])

    # ---- input checks --------------------------------------------------
    lag_of, lag_by_name = {}, {}
    for lag in lags:
        name = lag.get('name', '')
        if not re.match(r'^[A-Za-z][A-Za-z0-9_]*$', name) or name in by_name:
            raise InputError('bad lag name %r' % name)
        if name in lag_by_name:
            raise InputError('lag %s defined twice' % name)
        ds = []
        for m in lag.get('members', []):
            if m not in by_name:
                raise InputError('lag %s: unknown port %s (known: fpp<cage> or '
                                 'fpp<cage>-<k> from the port spec)' % (name, m))
            if m in lag_of:
                raise InputError('%s is in two lags (the SDK allows one '
                                 'primary LAG per sysport)' % m)
            lag_of[m] = name
            ds.append(by_name[m].devport)
        if not ds:
            raise InputError('lag %s has no members' % name)
        if len(set(by_name[m].speed for m in lag['members'])) != 1:
            raise InputError('lag %s mixes port speeds' % name)
        lag_by_name[name] = sorted(ds)
    bridged_lags, untagged_in, used_vlans = set(), {}, {}
    for br in bridges:
        name, vlan = br.get('name', ''), br.get('vlan')
        if not re.match(r'^[A-Za-z][A-Za-z0-9_]*$', name):
            raise InputError('bad bridge name %r' % name)
        if not isinstance(vlan, int) or not 1 <= vlan <= 4094:
            raise InputError('bridge %s: vlan must be 1..4094' % name)
        if knet_vlans[0] <= vlan <= knet_vlans[1]:
            raise InputError(
                'bridge %s: vlan %d collides with the knet private VLAN of '
                'devport %d (%s). `diagtest knet config` gives every front '
                'port its own L2VNI = devport+1, so with this port spec VLANs '
                '%d..%d are taken. Use a VLAN above %d, or replace knet config '
                '(see DESIGN.md, "VLAN id collision").'
                % (name, vlan, vlan - 1, ports[vlan - 2].netdev,
                   knet_vlans[0], knet_vlans[1], knet_vlans[1]))
        if vlan in used_vlans:
            raise InputError('vlan %d used by two bridges' % vlan)
        used_vlans[vlan] = name
        if not br.get('ports'):
            raise InputError('bridge %s has no ports' % name)
        for p in br['ports']:
            if p in lag_by_name:
                bridged_lags.add(p)
            elif p in by_name:
                if p in lag_of:
                    raise InputError('bridge %s: %s is a member of lag %s; '
                                     'list the lag instead' % (name, p, lag_of[p]))
            else:
                raise InputError('bridge %s: unknown port %s' % (name, p))
            if p not in br.get('tagged', []):
                if p in untagged_in:
                    raise InputError('%s is untagged in two bridges' % p)
                untagged_in[p] = name
        for p in br.get('tagged', []):
            if p not in br['ports']:
                raise InputError('bridge %s: tagged port %s not in ports' % (name, p))
        mac = br.get('mac')
        if mac and not re.match(r'^([0-9a-f]{2}:){5}[0-9a-f]{2}$', mac):
            raise InputError('bridge %s: mac must be lower-case aa:bb:cc:dd:ee:ff' % name)

    used = set()
    for ds in lag_by_name.values():
        used.update(ds)
    for br in bridges:
        for p in br['ports']:
            if p in by_name:
                used.add(by_name[p].devport)
    enable = state.get('enable')
    if enable is None:
        en_ports = sorted(used)
    else:
        en_ports = []
        for p in enable:
            if p in lag_by_name:
                en_ports += lag_by_name[p]
            elif p in by_name:
                en_ports.append(by_name[p].devport)
            else:
                raise InputError('enable: unknown port %s' % p)
        en_ports = sorted(set(en_ports))
    front = [p.devport for p in ports]

    A, T = [], []          # apply, teardown

    # ---- guard -----------------------------------------------------------
    if guard:
        A.append(pre('innoviumPorts = "%s"' % guard_spec(spec), 'V',
                     'before SDK start: down-pall makes dynamic_config.sh emit '
                     '`port disable N` instead of `port enable N` '
                     '(dynamic_config.sh:278-292) for both init passes'))
        A.append(cli_port('disable', front, 'V',
                          'belt and braces after knet config; port.py:428-498 '
                          'sets IFCS_DEVPORT_ATTR_ADMIN_STATE false'))
        A.append(cli('diagtest knet show', 'V',
                     'check: every L2VNI must list only cpu + one port'))
    if used:
        A.append(cli_port('disable', sorted(used), 'V',
                          'ports being re-plumbed stay admin-down until the '
                          'last step'))

    # ---- LAGs ------------------------------------------------------------
    for lag in lags:
        name = lag['name']
        ds = lag_by_name[name]
        d1 = ds[0]
        h, mcd = st('lag_' + name), st('mcd_' + name)
        for d in ds:
            A.append(api_mrem('ifcs_mdg_mc_dest_remove', kmdg(d), [kmcd(d)], 'I',
                              'detach dp%d from its knet flood group: the SDK '
                              'refuses a LAG member that is still in an '
                              'mc_dest/L2VNI/L2 entry (libifcs.so strings)' % d))
            A.append(api_delete('ifcs_mc_dest_delete', kmcd(d), 'I'))
            A.append(api_mrem('ifcs_l2vni_member_remove', l2vni(d + 1), [sp(d)], 'I'))
        A.append(api_create('ifcs_lag_create', 'lag_' + name, 'I',
                            'no vendor script creates a LAG; form from the '
                            'ctypes signature only'))
        A.append(api_madd('ifcs_lag_member_add', h, [sp(d) for d in ds], 'I',
                          attrs=(('IFCS_LAG_MEMBER_ATTR_TYPE', 'u32',
                                  const('IFCS_LAG_MEMBER_TYPE_PRIMARY')),)))
        A.append(api_create('ifcs_mc_dest_create', 'mcd_' + name, 'I',
                            'flood destination for the LAG as a whole',
                            attrs=(('IFCS_MC_DEST_ATTR_LOCAL_DESTINATION',
                                    'handle', h),)))
        if name not in bridged_lags:
            A.append(api_madd('ifcs_l2vni_member_add', l2vni(d1 + 1), [h], 'I',
                              'standalone LAG: reuse dp%d\'s private L2VNI '
                              '(cpu + LAG only), so the CPU path works as '
                              'before' % d1))
            A.append(api_madd('ifcs_mdg_mc_dest_add', kmdg(d1), [mcd], 'I'))
            A.append(api_madd('ifcs_stp_port_add', KSTP, [h], 'U',
                              'unknown whether STP state is kept per LAG or '
                              'per member sysport (members are already '
                              'FORWARDING in the knet STP instance)',
                              attrs=stp_state('FORWARDING'), optional=True))
            for d in ds:
                if d != d1:
                    A += pvid_ops(d, d1 + 1, 'I',
                                  'untagged frames on every member map to the '
                                  'LAG\'s L2VNI (CLI form V, need I)')
        # teardown for this lag (prepended later, so written in run order)
        t = []
        if name not in bridged_lags:
            t.append(api_mrem('ifcs_stp_port_remove', KSTP, [h], 'U', optional=True))
            t.append(api_mrem('ifcs_mdg_mc_dest_remove', kmdg(d1), [mcd], 'I'))
            t.append(api_mrem('ifcs_l2vni_member_remove', l2vni(d1 + 1), [h], 'I'))
        t.append(api_delete('ifcs_mc_dest_delete', mcd, 'I'))
        t.append(api_mrem('ifcs_lag_member_remove', h, [sp(d) for d in ds], 'I'))
        t.append(api_delete('ifcs_lag_delete', h, 'I'))
        for d in ds:
            t.append(api_create(
                'ifcs_mc_dest_create', None, 'V',
                'restore knet plumbing for dp%d exactly as knet.py:164-257 '
                'built it, so `diagtest knet unconfig` still works' % d,
                attrs=(('IFCS_MC_DEST_ATTR_LOCAL_DESTINATION', 'handle', sp(d)),),
                out_py='Knet.mcdest_hdl[%d]' % d))
            t[-1].text = 'Knet.mcdest_hdl[%d] = ' % d + t[-1].text
            t.append(api_madd('ifcs_mdg_mc_dest_add', kmdg(d), [kmcd(d)], 'V'))
            t.append(api_madd('ifcs_l2vni_member_add', l2vni(d + 1), [sp(d)], 'V'))
            t += pvid_ops(d, d + 1, 'V', 'knet.py:244-257 values')
        T.insert(0, t)

    # ---- bridges -----------------------------------------------------------
    stp_fwd = []
    for br in bridges:
        name, vlan = br['name'], br['vlan']
        stp, mdg, vni = st('stp_' + name), st('mdg_' + name), l2vni(vlan)
        members, mcds, untag = [], [], []
        for p in br['ports']:
            tagged = p in br.get('tagged', [])
            if p in lag_by_name:
                members.append(st('lag_' + p))
                mcds.append(st('mcd_' + p))
                if not tagged:
                    untag += lag_by_name[p]
            else:
                d = by_name[p].devport
                members.append(sp(d))
                mcds.append(kmcd(d))
                if not tagged:
                    untag.append(d)
        cpu = sp(CPU_DEVPORT)
        A.append(api_create('ifcs_stp_create', 'stp_' + name, 'V',
                            'own STP instance per bridge (knet.py:175-180 form)'))
        A.append(api_madd('ifcs_stp_port_add', stp, [cpu] + members, 'V' if
                          not any(p in lag_by_name for p in br['ports']) else 'U',
                          'members start BLOCKING; forwarding is the explicit '
                          'last step. LAG handle as STP port is U',
                          attrs=stp_state('BLOCKING')))
        A.append(api_create('ifcs_mdg_create', 'mdg_' + name, 'V',
                            'flood group (knet.py:193-199 form)'))
        A.append(api_create('ifcs_l2vni_create', None, 'V',
                            'knet.py:201-214 form, VLAN id = L2VNI value',
                            attrs=(('IFCS_L2VNI_ATTR_STP_INSTANCE', 'handle', stp),
                                   ('IFCS_L2VNI_ATTR_FLOOD_GROUP', 'handle', mdg)),
                            preset=vni))
        A.append(api_madd('ifcs_l2vni_member_add', vni, [cpu] + members, 'V',
                          'cpu is a member so floods/traps reach the netdevs '
                          '(knet.py:216-222 form)'))
        A.append(api_madd('ifcs_mdg_mc_dest_add', mdg, [kmcd(0)] + mcds, 'V',
                          'reuses the knet mc_dest objects (knet puts '
                          'mcdest[0] in every MDG, so sharing is allowed)'))
        for d in untag:
            A += pvid_ops(d, vlan, 'V', 'access port: PVID + egress untagged '
                          '(knet.py:244-257 form)')
        for mac in LINK_LOCAL:
            A.append(api_l2add(mac, vlan, cpu, 'U',
                               'keep link-local (STP/LACP/LLDP) out of the '
                               'flood group: static entry to cpu. Whether a '
                               'group MAC is accepted here is U',
                               optional=True))
        if br.get('mac'):
            A.append(api_l2add(br['mac'], vlan, cpu, 'I',
                               'bridge/SVI MAC to cpu (test_l2.py:156-186 form)'))
        for m in [cpu] + members:
            is_lag = m.text.startswith('$lag_')
            stp_fwd.append(api_pset('ifcs_stp_port_attr_set', stp, m,
                                    stp_state('FORWARDING'),
                                    'U' if is_lag else 'V',
                                    'LAG handle as STP port is U' if is_lag
                                    else 'knet.py:232-242 form'))
        t = []
        if br.get('mac'):
            t.append(api_l2del(br['mac'], vlan, 'I'))
        for mac in LINK_LOCAL:
            t.append(api_l2del(mac, vlan, 'U', optional=True))
        t.append(cli('l2learn flush', 'U',
                     'l2learn.py:70-72 flushes the learn DB; whether learned '
                     'hardware entries in this L2VNI must be deleted before '
                     'ifcs_l2vni_delete is U'))
        t.append(api_mrem('ifcs_mdg_mc_dest_remove', mdg, [kmcd(0)] + mcds, 'V'))
        t.append(api_mrem('ifcs_l2vni_member_remove', vni, [cpu] + members, 'V'))
        t.append(api_delete('ifcs_l2vni_delete', vni, 'V'))
        t.append(api_delete('ifcs_mdg_delete', mdg, 'V'))
        t.append(api_mrem('ifcs_stp_port_remove', stp, [cpu] + members, 'I'))
        t.append(api_delete('ifcs_stp_delete', stp, 'V'))
        for p in br['ports']:
            if p in by_name and p not in br.get('tagged', []):
                d = by_name[p].devport
                t += pvid_ops(d, d + 1, 'V', 'back to the knet private VLAN')
        T.insert(0, t)

    # ---- enable ------------------------------------------------------------
    A += stp_fwd
    if en_ports:
        A.append(cli_port('enable', en_ports, 'V',
                          'explicit enable, last (port.py:428-498)'))
    if lags:
        A.append(cli('ifcs show lag', 'V', 'check'))
    for br in bridges:
        A.append(cli('ifcs show l2vni %d members' % br['vlan'], 'V', 'check'))
    if bridges:
        A.append(cli('ifcs show l2_entry', 'V', 'check (vendor knet-l2-show)'))

    teardown = []
    if used:
        teardown.append(cli_port('disable', sorted(used), 'V',
                                 'first: nothing forwards while objects go away'))
    # bridges were inserted after lags, so T is [bridges reversed..., lags reversed...]
    for t in T:
        teardown += t
    if used and not guard:
        teardown.append(cli_port('enable', sorted(used), 'V',
                                 'guard is off: back to vendor default (all up)'))
    elif used:
        teardown.append(pre('ports %s stay admin-down (guard)' % compress(used), 'V'))
    return A, teardown


# --------------------------------------------------------------------------
# Validation against the vendor sources (read in place, never copied)
# --------------------------------------------------------------------------
class Vendor(object):
    def __init__(self, sdk):
        self.sdk = sdk
        self._src = {}
        self._ct = None

    def src(self, rel):
        if rel not in self._src:
            with open(os.path.join(self.sdk, rel), 'rb') as f:
                self._src[rel] = f.read().decode('utf-8', 'replace')
        return self._src[rel]

    # ---- CLI ---------------------------------------------------------------
    def top_cmds(self):
        return (set(re.findall(r'add_cmd\("(\w+)"', self.src('ifcs_cli.py')))
                | set(re.findall(r'def do_(\w+)', self.src('cli.py'))))

    @staticmethod
    def _dict_keys(text, start):
        i = text.index(start)
        return re.findall(r"'([\w?]+)'\s*:", text[i:text.index('}', i)])

    def sub_cmds(self, rel, after=''):
        s = self.src(rel)
        s = s[s.index(after):] if after else s
        return self._dict_keys(s, 'self.sub_cmds = {')

    def ifcs_objs(self):
        return self._dict_keys(self.src('ifcs_cmds/all.py'), 'self.ifcs_objs_dict = {')

    def obj(self, name):
        s = self.src('ifcs_cmds/%s.py' % name)
        i = s.index('self.set_methods = {')
        setm = dict(re.findall(r"'(\w+)'\s*:\s*self\.(\w+)", s[i:s.index('}', i)]))
        types = dict(re.findall(r"attrlist\.append\(\s*\{\s*'(\w+)'\s*:\s*'(\w+)'\s*\}\s*\)", s))
        m = re.search(r'self\.supported_operands = \[(.*?)\]', s, re.S)
        operands = re.findall(r"'(\w*)'", m.group(1)) if m else []
        return s, setm, types, operands

    def run_parser(self, name, cmd, tokens):
        """Run the vendor's own preParse() + argparse set-up for
        `ifcs <cmd> <obj> ...` (extracted from ifcs_cmds/<obj>.py parseArgs;
        postParse is skipped, it only converts numbers to handles)."""
        s = self.src('ifcs_cmds/%s.py' % name)
        i = s.index('def parseArgs(self, cmd, args):')
        a = s.rfind('\n', 0, s.index('def preParse(', i)) + 1
        b = s.rfind('\n', 0, s.index('def postParse(', a)) + 1
        c = s.rfind('\n', 0, s.index('preParse(self.supported_operands', b)) + 1
        end = 'parsed_res = parser.parse_args(arg_list)'
        d = s.index(end, c) + len(end)
        code = (textwrap.dedent(s[a:b]) + textwrap.dedent(s[c:d])).replace(
            "cmd is '", "cmd == '")

        class Self(object):
            supported_operands = self.obj(name)[3]
        errs = []
        env = {'argparse': argparse, 'cmd': cmd, 'args': ' '.join(tokens),
               'self': Self(), 'log_err': errs.append}
        err = io.StringIO()
        try:
            with contextlib.redirect_stderr(err):
                exec(compile(code, name + '.parseArgs', 'exec'), env)
        except SystemExit:
            raise ValueError('vendor argparse rejects it: '
                             + err.getvalue().strip().splitlines()[-1])
        except TypeError:
            raise ValueError('vendor preParse rejects it: ' + '; '.join(errs))
        return env['parsed_res']

    def check_cli(self, line, devports):
        t = line.split()
        if t[0] not in self.top_cmds():
            return False, 'unknown top-level command %s' % t[0]
        if t[0] == 'ifcs':
            verbs = self.sub_cmds('cmds/ifcscmd.py', 'class Ifcs(Command)')
            if t[1] not in verbs:
                return False, 'ifcs has no verb %r (has: %s)' % (t[1], ' '.join(verbs))
            if t[2] not in self.ifcs_objs():
                return False, 'unknown ifcs object %s' % t[2]
            s, setm, types, operands = self.obj(t[2])
            if t[1] == 'set':
                if "self.cmd_options['set']" not in s:
                    return False, '%s has no set handler' % t[2]
                try:
                    ns = self.run_parser(t[2], 'set', t)
                except ValueError as e:
                    return False, str(e)
                attr, val = ns.cmd_opts.lower(), ns.more_opts
                if attr not in setm:
                    return False, '%s has no settable attr %s' % (t[2], attr)
                body = re.search(r'def %s\(.*?(?=\n    def )' % setm[attr], s, re.S).group(0)
                if 'int(' not in body:
                    return False, ('setter %s assigns the raw CLI string to a '
                                   'ctypes field (only int() setters work from '
                                   'the CLI)' % setm[attr])
                try:
                    v = int(val)
                except (TypeError, ValueError):
                    return False, 'value %r is not an int' % val
                lim = {'u8': 255, 'u16': 65535, 'data': 1}.get(types.get(attr), 2 ** 32 - 1)
                if not 0 <= v <= lim:
                    return False, 'value %d out of range for %s' % (v, types.get(attr))
                if ns.cmd_opt_args not in devports:
                    return False, 'sysport/devport %s not in the port map' % ns.cmd_opt_args
                return True, 'vendor preParse+argparse ok; %s.%s (%s) via %s()' % (
                    t[2], attr, types.get(attr), setm[attr])
            if t[1] == 'show':
                try:
                    ns = self.run_parser(t[2], 'show', t)
                except ValueError as e:
                    return False, str(e)
                if ('def show_%s_%s' % (t[2], ns.cmd_opts)) not in s and \
                        "cmd_options['%s']" % ns.cmd_opts not in s:
                    return False, 'no show handler for %r' % ns.cmd_opts
                return True, 'vendor preParse+argparse ok; show handler %s' % ns.cmd_opts
            return False, 'verb %s not handled by this validator' % t[1]
        if t[0] == 'port':
            if t[1] not in self.sub_cmds('cmds/port.py'):
                return False, 'port has no sub-command %s' % t[1]
            if len(t) != 3 or not re.match(r'^\d+(-\d+)?(,\d+(-\d+)?)*$', t[2]):
                return False, 'port list must be a,b-c'
            for part in t[2].split(','):
                r = [int(x) for x in part.split('-')]
                for d in range(r[0], r[-1] + 1):
                    if d not in devports:
                        return False, 'devport %d not in the port map' % d
            return True, 'port.py sub-command + list syntax ok'
        if t[0] == 'diagtest':
            if t[1] not in self.sub_cmds('cmds/diagtest.py'):
                return False, 'diagtest has no sub-command %s' % t[1]
            if t[1] == 'knet' and t[2] not in self.sub_cmds('cmds/knet.py'):
                return False, 'knet has no sub-command %s' % t[2]
            return True, 'diagtest sub-command exists'
        if t[0] == 'l2learn':
            if t[1] not in self.sub_cmds('cmds/l2learn.py'):
                return False, 'l2learn has no sub-command %s' % t[1]
            return True, 'l2learn sub-command exists'
        return False, 'command %s not handled by this validator' % t[0]

    # ---- C API ---------------------------------------------------------------
    def ctypes(self):
        if self._ct is None:
            funcs, names, slots = {}, set(), []
            text = self.src('pyctypes/ifcs_ctypes.py')
            for m in re.finditer(r'^\s+(\w+)\.argtypes = \[(.*)\]\s*$', text, re.M):
                depth, n = 0, 1 if m.group(2).strip() else 0
                for ch in m.group(2):
                    depth += ch in '(['
                    depth -= ch in ')]'
                    n += ch == ',' and depth == 0
                funcs[m.group(1)] = n
            names.update(re.findall(r'^\s*(\w+) = ', text, re.M))
            names.update(re.findall(r'^def (\w+)\(', text, re.M))
            names.update(re.findall(r'^class (\w+)\(', text, re.M))
            i = text.index('union_ifcs_attr_value_u.__slots__ = [')
            slots = re.findall(r"'(\w+)'", text[i:text.index(']', i)])
            self._ct = (funcs, names | set(funcs), slots)
        return self._ct

    def check_api(self, op):
        funcs, names, slots = self.ctypes()
        if op.func not in funcs:
            return False, '%s not in ifcs_ctypes.py' % op.func
        if funcs[op.func] != ARGC[op.form]:
            return False, '%s takes %d args, generator uses %d' % (
                op.func, funcs[op.func], ARGC[op.form])
        for ident in set(re.findall(r'\bIFCS_\w+', op.py)):
            if ident not in names:
                return False, 'constant/macro %s not in ifcs_ctypes.py' % ident
        for _a, field, _v in op.attrs:
            if field not in slots:
                return False, 'ifcs_attr_t.value has no field %s' % field
        knet = self.src('cmds/knet.py')
        for a in set(re.findall(r'Knet\.(\w+)', op.py)):
            if not re.search(r'^\t%s\s*=' % a, knet, re.M):
                return False, 'Knet has no class attribute %s' % a
        return True, '%s exists, %d args, constants ok' % (op.func, funcs[op.func])

    def check_py2(self, text):
        """Every ifcs_*/IFCS_* identifier of the emitted module must resolve."""
        _f, names, _s = self.ctypes()
        own = set(re.findall(r'^def (\w+)\(', text, re.M))
        code = re.sub(r"'(?:[^'\\]|\\.)*'|\"(?:[^\"\\]|\\.)*\"|#.*", '', text)
        missing = sorted(i for i in set(re.findall(r'\b(?:ifcs|IFCS)_\w+', code))
                         if i not in names and i not in own and i != 'ifcs_ctypes')
        compile(text, 'emitted-py2', 'exec')      # shared py2/py3 syntax subset
        return missing


# --------------------------------------------------------------------------
# Python 2 module emission (runs inside innovium.user; never run here)
# --------------------------------------------------------------------------
PY2_RUNTIME = r'''# Generated by switchd_dryrun.py from %(source)s
# port spec: %(spec)s
# NOT YET RUN ON HARDWARE. Python 2, runs inside innovium.user:
#   copy to <SDK>/scripts/%(module)s.py in the chroot, then
#   cls_shell 'run %(module)s apply'   /   cls_shell 'run %(module)s teardown'
# (cli.py run_script(): __import__(module); reload; module.main(args))
# Any failed mandatory step raises, so the final `port enable` never runs.
import json
import os
from ctypes import *
from ifcs_ctypes import *
from knet import Knet

STATE_FILE = '/tmp/cs8210-%(module)s.json'
S = {}


def H(x):
    return getattr(x, 'value', x)


def SP(devport):
    attr = ifcs_attr_t()
    count = c_uint32()
    attr.id = IFCS_DEVPORT_ATTR_SYSPORT
    rc = ifcs_devport_attr_get(0, devport, 1, pointer(attr), pointer(count))
    if rc != IFCS_SUCCESS:
        raise RuntimeError('no sysport for devport %%d' %% devport)
    return attr.value.handle


def _attrs(lst):
    arr = (ifcs_attr_t * max(len(lst), 1))()
    for i, (attr_id, field, val) in enumerate(lst):
        ifcs_attr_t_init(pointer(arr[i]))
        arr[i].id = attr_id
        setattr(arr[i].value, field, H(val))
    return len(lst), arr


def _handles(lst):
    arr = (ifcs_handle_t * len(lst))()
    for i, h in enumerate(lst):
        arr[i] = H(h)
    return len(lst), arr


def _ck(what, rc, optional=False):
    reason = IFCS_STATUS_REASON(rc)
    if reason == IFCS_SUCCESS:
        print('ok   ' + what)
    elif optional:
        print('SKIP ' + what + ' rc=%%d (optional step)' %% reason)
    else:
        raise RuntimeError('FAIL ' + what + ' rc=%%d' %% reason)


def create(what, fn, attrs, preset, optional=False):
    h = ifcs_handle_t()
    h.value = IFCS_NULL_HANDLE if preset is None else H(preset)
    n, arr = _attrs(attrs)
    _ck(what, fn(0, pointer(h), n, arr), optional)
    return h.value


def delete(what, fn, h, optional=False):
    _ck(what, fn(0, H(h)), optional)


def madd(what, fn, h, members, attrs, optional=False):
    n, arr = _handles(members)
    an, aarr = _attrs(attrs)
    _ck(what, fn(0, H(h), n, arr, an, aarr), optional)


def mrem(what, fn, h, members, optional=False):
    n, arr = _handles(members)
    _ck(what, fn(0, H(h), n, arr), optional)


def pset(what, fn, h, port, attrs, optional=False):
    an, aarr = _attrs(attrs)
    _ck(what, fn(0, H(h), H(port), an, aarr), optional)


def aset(what, fn, h, attrs, optional=False):
    an, aarr = _attrs(attrs)
    _ck(what, fn(0, H(h), an, aarr), optional)


def admin(what, devports, state):
    for d in devports:
        an, aarr = _attrs([(IFCS_DEVPORT_ATTR_ADMIN_STATE, 'u32', state)])
        _ck(what + ' (dp%%d)' %% d, ifcs_devport_attr_set(0, d, an, aarr))


def _l2key(mac, vlan):
    key = ifcs_l2_entry_key_t()
    ifcs_l2_entry_key_t_init(pointer(key))
    ifcs_l2_entry_key_t_key_type_set(pointer(key), IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI)
    ifcs_l2_entry_key_t_mac_l2vni_set(pointer(key), key.key.mac_l2vni)
    addr = (c_uint8 * 6)(*[int(b, 16) for b in mac.split(':')])
    ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(pointer(key.key.mac_l2vni), addr)
    ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(pointer(key.key.mac_l2vni),
                                            IFCS_HANDLE_L2VNI(vlan))
    return key


def l2add(what, mac, vlan, attrs, optional=False):
    an, aarr = _attrs(attrs)
    _ck(what, ifcs_l2_entry_create(0, pointer(_l2key(mac, vlan)), an, aarr), optional)


def l2del(what, mac, vlan, optional=False):
    _ck(what, ifcs_l2_entry_delete(0, pointer(_l2key(mac, vlan))), optional)


def _load():
    S.clear()
    if os.path.exists(STATE_FILE):
        with open(STATE_FILE) as f:
            saved = json.load(f)
        if saved.get('_pid') == os.getpid():      # handles die with the SDK
            S.update(saved)
    S['_pid'] = os.getpid()


def _save():
    with open(STATE_FILE, 'w') as f:
        json.dump(S, f)


def main(args=''):
    step = {'apply': apply, 'teardown': teardown}.get(args.strip())
    if step is None:
        print('usage: run %(module)s apply|teardown')
        return
    if Knet.max_ports == 0:
        print('refusing: `diagtest knet config` has not run in this SDK process')
        return
    _load()
    try:
        step()
    finally:
        _save()
'''


def emit_py2(module, source, spec, apply_ops, teardown_ops):
    out = [PY2_RUNTIME % {'module': module, 'source': source, 'spec': spec}]
    for fname, ops in (('apply', apply_ops), ('teardown', teardown_ops)):
        out.append('\n\ndef %s():\n' % fname)
        body = 0
        for op in ops:
            if op.py:
                out.append('    %s\n' % op.py)
                body += 1
            else:
                out.append('    # %s: %s\n' % (op.kind, op.text))
        if not body:
            out.append('    pass\n')
    return ''.join(out)


# --------------------------------------------------------------------------
# Output
# --------------------------------------------------------------------------
def render(ops, prefix, vendor, devports, lines):
    bad = 0
    for i, op in enumerate(ops, 1):
        if op.kind == 'cli':
            body = "cls_shell '%s'" % op.text
        elif op.kind == 'api':
            body = op.text + ('   (optional)' if op.optional else '')
        else:
            body = op.text
        res = ''
        if vendor and op.kind == 'cli':
            ok, why = vendor.check_cli(op.text, devports)
            res = ('ok: ' if ok else 'INVALID: ') + why
            bad += not ok
        elif vendor and op.kind == 'api':
            ok, why = vendor.check_api(op)
            res = ('ok: ' if ok else 'INVALID: ') + why
            bad += not ok
        elif op.kind == 'pre':
            res = 'not a command'
        else:
            res = 'not validated (no SDK sources)'
        lines.append('[%s%02d] %-3s %s  %s' % (prefix, i, op.kind, op.conf, body))
        if op.note:
            for w in textwrap.wrap(op.note, 88):
                lines.append('            # ' + w)
        lines.append('            = ' + res)
    return bad


def selftest(sdk):
    """Port mapper vs the vendor's own generated dynamic_config.yaml."""
    v = Vendor(sdk)
    sh = v.src('dynamic_config.sh')
    dic = dict((k, int(n)) for k, n in re.findall(r'\["(\dx\d+)"\]=(\d+)', re.search(r'^dic=\((.*)\)', sh, re.M).group(1)))
    isg = [int(x) for x in re.search(r'^isgMap=\(([\d\s]+)\)', sh, re.M).group(1).split()]
    ok = dic == LANES
    print('lanes table matches dynamic_config.sh dic: %s' % ok)
    y = v.src('cel_configs/dynamic_config.yaml')
    spec = re.search(r'# mode=(.+)', y).group(1).strip()
    ports, _w = parse_port_spec(spec)
    want = re.findall(r'id: "(\d+)"\s+lanes: "(\d+):(\d+)"\s+serdes_group: "(\d+)"\s+speed: "(\d+)G"\s+sysport: "(\d+)"', y)
    got = [(str(p.devport), str(p.lane0), str(p.lanes), str(isg[p.cage - 1]),
            p.speed, str(p.sysport)) for p in ports]
    same = got == want
    print('vendor dynamic_config.yaml (mode=%s): %d devports, mapper output identical: %s'
          % (spec, len(want), same))
    # The validator must reject forms the vendor CLI cannot execute.
    devports = set(p.devport for p in ports)
    neg = True
    for line, why in (
            ('ifcs create lag 1', 'no create verb'),
            ('ifcs set sysport 2 lag 5', 'handle setter takes the raw string'),
            ('ifcs set sysport 2 no_such_attr 1', 'unknown attribute'),
            ('ifcs set sysport 2 default_cvid 70000', 'u16 range'),
            ('ifcs set sysport 2 default_cvid', 'missing value'),
            ('ifcs set sysport 999 default_cvid 5', 'port not in map'),
            ('ifcs show nosuchobj', 'unknown object'),
            ('port shutdown 2', 'unknown sub-command'),
            ('port disable 2..5', 'bad list syntax'),
            ('diagtest knet reconfig', 'unknown sub-command'),
            ('lag create 1', 'unknown top-level command')):
        good, msg = v.check_cli(line, devports)
        print('reject %-40r %-36s -> %s' % (line, '(' + why + ')',
                                             'ACCEPTED (bad)' if good else msg))
        neg = neg and not good
    bad_api = api_create('ifcs_lag_create', 'x', 'I')
    bad_api.form = 'delete'
    good, msg = v.check_api(bad_api)
    print('reject api with wrong arg count -> %s' % ('ACCEPTED (bad)' if good else msg))
    neg = neg and not good
    bad_api = api_madd('ifcs_lag_member_add', st('x'), [sp(2)], 'I',
                       attrs=(('IFCS_LAG_MEMBER_ATTR_NOPE', 'u32', num(0)),))
    good, msg = v.check_api(bad_api)
    print('reject api with unknown constant -> %s' % ('ACCEPTED (bad)' if good else msg))
    neg = neg and not good
    print('negative tests all rejected: %s' % neg)
    return 0 if ok and same and neg else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('state', nargs='?', help='desired-state JSON')
    ap.add_argument('--ports', help='vendor port spec (innoviumPorts); '
                    'default: "port_spec" in the JSON')
    ap.add_argument('--sdk', default=DEFAULT_SDK, help='extracted vendor SDK dir')
    ap.add_argument('--no-validate', action='store_true')
    ap.add_argument('--emit-py2', metavar='FILE', help='write the Python 2 module')
    ap.add_argument('--selftest', action='store_true')
    args = ap.parse_args()
    if args.selftest:
        return selftest(args.sdk)
    if not args.state:
        ap.error('STATE.json required')
    try:
        with open(args.state) as f:
            state = json.load(f)
        spec = args.ports or state.get('port_spec')
        if not spec:
            raise InputError('no port spec (--ports or "port_spec")')
        ports, warnings = parse_port_spec(spec)
        apply_ops, teardown_ops = build_plan(state, spec, ports)
    except (InputError, ValueError, OSError) as e:
        print('ERROR: %s' % e)
        return 2

    vendor = None
    if not args.no_validate and os.path.isdir(args.sdk):
        vendor = Vendor(args.sdk)
    devports = set(p.devport for p in ports)
    L = ['# switchd_dryrun: %s' % os.path.basename(args.state),
         '# port spec: %s' % spec,
         '# DRY RUN - nothing was executed. Tags: V verified-from-source form, '
         'I inferred, U unknown (hardware).', '']
    for w in warnings:
        L.append('WARNING: ' + w)
    L.append('## PORT MAP (front ports: %d, knet private VLANs %d..%d, cpu = devport 0 / sysport 1000)'
             % (len(ports), 2, len(ports) + 1))
    L.append('%-8s %7s %7s %5s %6s %6s %9s %5s' % (
        'netdev', 'devport', 'sysport', 'cage', 'lanes', 'speed', 'knet-vlan', 'admin'))
    named = set()
    for lag in state.get('lags', []):
        named.update(lag.get('members', []))
    for br in state.get('bridges', []):
        named.update(br.get('ports', []))
    shown = [p for p in ports if p.netdev in named] or ports[:4]
    for p in shown:
        L.append('%-8s %7d %7d %5d %6s %5sG %9d %5s' % (
            p.netdev, p.devport, p.sysport, p.cage,
            '%d:%d' % (p.lane0, p.lanes), p.speed, p.knet_vlan,
            'up' if p.admin_up else 'down'))
    L.append('(%d of %d ports shown)' % (len(shown), len(ports)))
    L += ['', '## APPLY']
    bad = render(apply_ops, 'A', vendor, devports, L)
    L += ['', '## TEARDOWN']
    bad += render(teardown_ops, 'T', vendor, devports, L)
    L += ['', '## VALIDATION']
    n_cli = sum(o.kind == 'cli' for o in apply_ops + teardown_ops)
    n_api = sum(o.kind == 'api' for o in apply_ops + teardown_ops)
    if vendor:
        L.append('%d cli + %d api operations checked against %s: %d invalid'
                 % (n_cli, n_api, os.path.basename(args.sdk), bad))
    else:
        L.append('SKIPPED: vendor SDK sources not found at %s' % args.sdk)
    module = 'switchd_ops_' + re.sub(r'\W', '_', os.path.splitext(
        os.path.basename(args.state))[0])
    py2 = emit_py2(module, os.path.basename(args.state), spec, apply_ops, teardown_ops)
    if vendor:
        missing = vendor.check_py2(py2)
        L.append('python2 module (%s): compiles as shared py2/py3 syntax; '
                 'unresolved ifcs_*/IFCS_* identifiers: %s'
                 % (module, ', '.join(missing) if missing else 'none'))
        bad += len(missing)
    conf = {}
    for o in apply_ops + teardown_ops:
        if o.kind != 'pre':
            conf[o.conf] = conf.get(o.conf, 0) + 1
    L.append('confidence: ' + ', '.join('%s=%d' % (k, conf[k]) for k in sorted(conf)))
    L.append('not validated offline: runtime semantics of every I/U step, '
             'return codes, handle values, ordering constraints inside libifcs.')
    print('\n'.join(L))
    if args.emit_py2:
        with open(args.emit_py2, 'w') as f:
            f.write(py2)
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())

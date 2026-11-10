#!/usr/bin/env python3
"""Shared graph helpers for layers 6/7 (modules.py, channelrtl.py).

Logical analysis graph over the layer-4b primitives (xc7bels.py): every LUT
output (O6, O5) is its own node with its function reduced to its support;
every other primitive is one node.  LUT inputs get a symmetry role: the orbit
of the input under the permutation automorphisms of the function, numbered in
the canonical input order (so two LUTs with the same canonical function have
inputs with comparable roles even when the router put them on other pins).
"""
import collections
import itertools
import os
import pickle
import re
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '..', 'work')
sys.path.insert(0, HERE)

_PERMS = {k: list(itertools.permutations(range(k))) for k in range(7)}
_MAPS = {}


def _maps(k):
    """[n_perm, 2^k] array: for perm p, address a -> permuted address."""
    if k not in _MAPS:
        n = 1 << k
        m = np.zeros((len(_PERMS[k]), n), dtype=np.int64)
        for pi, perm in enumerate(_PERMS[k]):
            for a in range(n):
                b = 0
                for j in range(k):
                    if a >> j & 1:
                        b |= 1 << perm[j]
                m[pi, a] = b
        _MAPS[k] = m
    return _MAPS[k]


_INV = {}


def _inv(k):
    if k not in _INV:
        m = _maps(k)
        inv = np.zeros_like(m)
        rows = np.arange(m.shape[0])[:, None]
        inv[rows, m] = np.arange(m.shape[1])[None, :]
        _INV[k] = inv
    return _INV[k]


def permute_table(t, k, perm):
    """Table u with u(x') = t(x) where input j of t becomes input perm[j] of u."""
    u = 0
    for a in range(1 << k):
        if t >> a & 1:
            b = 0
            for j in range(k):
                if a >> j & 1:
                    b |= 1 << perm[j]
            u |= 1 << b
    return u


_CANON = {}


def canon(t, k):
    """(canonical table, [perms achieving it]) over permutations of k inputs.
    perm maps input j of t to input perm[j] of the canonical table."""
    key = (t, k)
    r = _CANON.get(key)
    if r is not None:
        return r
    if k == 0:
        r = (t & 1, [()])
    else:
        n = 1 << k
        bits = np.array([(t >> a) & 1 for a in range(n)], dtype=np.uint64)
        inv = _inv(k)                       # [perm, b] -> a with m[perm, a] = b
        u = bits[inv]                       # permuted bit vectors
        w = (np.uint64(1) << np.arange(n, dtype=np.uint64))
        vals = (u * w).sum(axis=1, dtype=np.uint64)
        best = vals.min()
        r = (int(best), [_PERMS[k][i] for i in np.nonzero(vals == best)[0]])
    _CANON[key] = r
    return r


def load_canon_cache():
    p = os.path.join(WORK, 'cache', 'lutcanon.pickle')
    if os.path.exists(p):
        _CANON.update(pickle.load(open(p, 'rb')))


def save_canon_cache():
    p = os.path.join(WORK, 'cache', 'lutcanon.pickle')
    os.makedirs(os.path.dirname(p), exist_ok=True)
    pickle.dump(_CANON, open(p + '.tmp', 'wb'))
    os.replace(p + '.tmp', p)


def input_roles(t, k):
    """Role of each input j (0..k-1): 'I' + sorted canonical positions it can
    take (its orbit under the function's permutation automorphisms)."""
    cv, perms = canon(t, k)
    roles = []
    for j in range(k):
        pos = sorted({p[j] for p in perms})
        roles.append('I' + '.'.join(map(str, pos)))
    return cv, roles


OUT_PORTS = {'O', 'O5', 'O6', 'Q', 'MC31'} | {'O[%d]' % i for i in range(4)} | {'CO[%d]' % i for i in range(4)}


class Graph:
    """nodes: id -> dict(prim, out, kind, label, ports {port: net}, role {port: role})
    net_drv: net -> (node, port); net_snk: net -> [(node, port)]"""

    def __init__(self, logic):
        self.P = {p['name']: p for p in logic['prims']}
        self.N = logic['nets']
        self.nodes = {}
        for p in logic['prims']:
            if p.get('fn'):
                for o, fn in p['fn'].items():
                    sup, t = fn['support'], fn['table']
                    k = len(sup)
                    cv, roles = input_roles(t, k)
                    ports = {'A%d' % i: p['conns'].get('A%d' % i) for i in sup}
                    ports[o] = p['conns'].get(o)
                    role = {'A%d' % i: roles[j] for j, i in enumerate(sup)}
                    role[o] = 'O'
                    self.nodes['%s#%s' % (p['name'], o)] = dict(
                        prim=p['name'], out=o, kind='LUT', label='L%d:%x' % (k, cv), ports=ports, role=role)
            else:
                ports = {k: v for k, v in p['conns'].items() if not v.startswith('PAD:')}
                self.nodes[p['name']] = dict(prim=p['name'], out=None, kind=p['prim'], label=self.label(p),
                                             ports=ports, role={k: k for k in ports})
        self.net_drv = {}
        self.net_snk = collections.defaultdict(list)
        for x, nd in self.nodes.items():
            pr = self.P[nd['prim']]
            for port, n in nd['ports'].items():
                if n is None:
                    continue
                d = self.N.get(n, {}).get('driver')
                if d and d[0] == nd['prim'] and d[1] == port:
                    self.net_drv[n] = (x, port)
                else:
                    self.net_snk[n].append((x, port))
        self.by_prim = collections.defaultdict(list)
        for x, nd in self.nodes.items():
            self.by_prim[nd['prim']].append(x)

    @staticmethod
    def label(p):
        t = p['prim']
        if t.startswith(('FD', 'LD')):
            c = p['conns']
            sr = c.get('R', c.get('S', c.get('CLR', c.get('PRE'))))
            return '%s:%s:ce%s:sr%s' % (t, p['params']['INIT'],
                                        'V' if c.get('CE') == 'VCC' else 'n',
                                        '0' if sr == 'GND' else 'n')
        if t == 'CARRY4':
            return 'CARRY4:' + p['params'].get('PRECYINIT', '')
        if t == 'RAMB18E1':
            ps = sorted(k for k in p['params'] if not k.startswith(('INIT_', 'INITP_')))
            return 'RAMB18E1:' + ','.join('%s=%s' % (k, p['params'][k]) for k in ps)
        if t in ('IOBUF', 'IBUF', 'OBUF', 'OBUFT'):
            return t + ':' + ','.join(p['params'].get('DRIVE', []))
        return t

    def neighbours(self, x):
        """[(my_port, my_role, net, other_node, other_port, other_role)]"""
        nd = self.nodes[x]
        out = []
        for port, n in nd['ports'].items():
            if n is None or n in ('VCC', 'GND'):
                continue
            d = self.net_drv.get(n)
            if d and d[0] == x and d[1] == port:
                for y, q in self.net_snk.get(n, ()):
                    out.append((port, nd['role'][port], n, y, q, self.nodes[y]['role'][q]))
            else:
                if d:
                    out.append((port, nd['role'][port], n, d[0], d[1], self.nodes[d[0]]['role'][d[1]]))
        return out


def site_xy(name):
    m = re.search(r'_X(\d+)Y(\d+)', name)
    return (int(m.group(1)), int(m.group(2))) if m else (0, 0)

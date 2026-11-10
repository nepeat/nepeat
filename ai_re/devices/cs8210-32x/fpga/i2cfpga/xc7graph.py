#!/usr/bin/env python3
"""Routing graph of a 7-series fabric, built from the Project X-Ray database.

Used by fasmnet.py (layer 4).  Everything here is derived from prjxray-db
files only (tilegrid.json, tileconn.json, tile_type_*.json, site_type_*.json,
ppips_*.db, segbits_*.db) and is cached as a pickle under work/cache/.

Terms (as in prjxray):
  wire  a piece of metal inside one tile, named (tile, wire)
  node  the set of wires joined by tileconn.json (no switch in between)
  pip   a programmable switch inside a tile, from src wire to dst wire
  ppip  a pseudo pip that has no configuration bits (ppips_*.db):
          always   permanently connected
          default  connected when nothing else drives the dst wire
                   (INT IMUX/BYP_ALT/FAN_ALT <- VCC_WIRE)
          hint     path through a site (LUT route-through etc.); handled by
                   the site model, never used as routing here

Pip kinds stored per tile type, for every pip in tile_type_*.json:
  'fasm'    the pip has segbits; it is on iff its FASM feature TILE.DST.SRC is set
  'always' / 'default' / 'hint'   from ppips_*.db
  'unknown' neither segbits nor ppip entry: the database cannot say whether it
            is on.  Never followed; fasmnet reports pins it could have driven.
"""
import glob
import json
import os
import pickle
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '..', 'work')
DB = os.path.join(WORK, 'src', 'prjxray-db', 'artix7')
FABRIC = 'xc7a100t'
CACHE = os.path.join(WORK, 'cache')
VERSION = 5

SITE_XY = re.compile(r'^(.*)_X(\d+)Y(\d+)$')


def _site_sort_key(name):
    m = SITE_XY.match(name)
    return (m.group(1), int(m.group(2)), int(m.group(3)))


def _build(db, fabric):
    tg = json.load(open(os.path.join(db, fabric, 'tilegrid.json')))
    tiles = {}
    coord = {}
    for name, t in tg.items():
        tiles[name] = (t['grid_x'], t['grid_y'], t['type'])
        coord[(t['grid_x'], t['grid_y'])] = name
    used_tt = sorted({t[2] for t in tiles.values()})
    alias = {}   # tile type -> tile type whose segbits it uses (tilegrid 'alias')
    for t in tg.values():
        for b in (t.get('bits') or {}).values():
            if 'alias' in b:
                alias[t['type']] = b['alias']['type']

    site_types = {}
    for f in glob.glob(os.path.join(db, 'site_type_*.json')):
        d = json.load(open(f))
        site_types[d['type']] = {p: v['direction'] for p, v in d['site_pins'].items()}

    tt = {}
    for t in used_tt:
        d = json.load(open(os.path.join(db, 'tile_type_%s.json' % t)))
        seg = set()
        segt = alias.get(t, t)
        for f in glob.glob(os.path.join(db, 'segbits_%s.db' % segt.lower())):
            for line in open(f):
                k = line.split()[0]
                seg.add(t + k[len(segt):] if k.startswith(segt + '.') else k)
        pp = {}
        f = os.path.join(db, 'ppips_%s.db' % t.lower())
        if os.path.exists(f):
            for line in open(f):
                k, v = line.split()
                pp[k] = v
        by_dst, by_src, fasm_pips = {}, {}, {}
        for v in (d['pips'] or {}).values():
            s, dd, bidir = v['src_wire'], v['dst_wire'], v['is_directional'] == '0'
            orient = [(dd, s)] + ([(s, dd)] if bidir else [])
            kind = 'unknown'
            for a, b in orient:
                k = '%s.%s.%s' % (t, a, b)
                if k in seg:
                    kind = 'fasm'
                    fasm_pips[(a, b)] = (dd, s, bidir)
                elif k in pp and kind == 'unknown':
                    kind = pp[k]
            for a, b in orient:   # a driven from b
                by_dst.setdefault(a, []).append((b, kind, bidir))
                by_src.setdefault(b, []).append((a, kind, bidir))
        sites = []
        wire_pin = {}
        for i, s in enumerate(d['sites'] or []):
            pins = {}
            for p, pv in (s['site_pins'] or {}).items():
                if pv is None:
                    continue
                direc = site_types.get(s['type'], {}).get(p, '?')
                pins[p] = (pv['wire'], direc)
                wire_pin.setdefault(pv['wire'], []).append((i, p, direc))
            sites.append(dict(prefix=s['prefix'], name=s['name'], type=s['type'],
                              x=s['x_coord'], y=s['y_coord'], pins=pins))
        tt[t] = dict(wires=sorted(d['wires'] or {}), wireset=set(d['wires'] or {}), by_dst=by_dst, by_src=by_src,
                     fasm_pips=fasm_pips, sites=sites, wire_pin=wire_pin)

    # relative site -> global site name, per tile
    tile_sites = {}
    for name, t in tg.items():
        glob_sites = t.get('sites') or {}
        rel = tt[t['type']]['sites']
        out = [None] * len(rel)
        byp = {}
        for i, s in enumerate(rel):
            byp.setdefault(s['prefix'], []).append(i)
        gbyp = {}
        for g in glob_sites:
            gbyp.setdefault(SITE_XY.match(g).group(1), []).append(g)
        for p, idxs in byp.items():
            gs = sorted(gbyp.get(p, []), key=_site_sort_key)
            idxs = sorted(idxs, key=lambda i: (rel[i]['x'], rel[i]['y']))
            # several site types can share one location (RAMB18 Y0 = FIFO18E1)
            locs = sorted({(rel[i]['x'], rel[i]['y']) for i in idxs})
            if len(gs) != len(locs):
                continue
            loc2g = dict(zip(locs, gs))
            for i in idxs:
                out[i] = loc2g[(rel[i]['x'], rel[i]['y'])]
        tile_sites[name] = out

    conn = {}
    for c in json.load(open(os.path.join(db, fabric, 'tileconn.json'))):
        (dx, dy), (t0, t1) = c['grid_deltas'], c['tile_types']
        for w0, w1 in c['wire_pairs']:
            conn.setdefault((t0, w0), []).append((dx, dy, t1, w1))
            conn.setdefault((t1, w1), []).append((-dx, -dy, t0, w0))
    return dict(version=VERSION, alias=alias, tiles=tiles, coord=coord, tt=tt, conn=conn,
                tile_sites=tile_sites, site_types=site_types)


class Fabric:
    def __init__(self, db=DB, fabric=FABRIC, cache=CACHE):
        path = os.path.join(cache, 'xc7graph-%s.pickle' % fabric)
        data = None
        if os.path.exists(path):
            with open(path, 'rb') as f:
                data = pickle.load(f)
            if data.get('version') != VERSION:
                data = None
        if data is None:
            data = _build(db, fabric)
            os.makedirs(cache, exist_ok=True)
            with open(path + '.tmp', 'wb') as f:
                pickle.dump(data, f, protocol=pickle.HIGHEST_PROTOCOL)
            os.replace(path + '.tmp', path)
        self.__dict__.update(data)
        self._node = {}

    def ttype(self, tile):
        return self.tiles[tile][2]

    def node(self, tile, wire):
        """Node containing (tile, wire): a sorted tuple of (tile, wire)."""
        k = (tile, wire)
        n = self._node.get(k)
        if n is not None:
            return n
        seen = {k}
        todo = [k]
        while todo:
            t, w = todo.pop()
            x, y, ty = self.tiles[t]
            for dx, dy, t2, w2 in self.conn.get((ty, w), ()):
                n2 = self.coord.get((x + dx, y + dy))
                if n2 is None or self.tiles[n2][2] != t2:
                    continue
                kk = (n2, w2)
                if kk not in seen:
                    seen.add(kk)
                    todo.append(kk)
        n = tuple(sorted(seen))
        for kk in n:
            self._node[kk] = n
        return n

    def site_name(self, tile, idx):
        return self.tile_sites[tile][idx]

    def site_pins_on_node(self, node):
        """[(tile, site_idx, pin, direction)] of site pins whose wire is in node."""
        out = []
        for t, w in node:
            for i, p, d in self.tt[self.ttype(t)]['wire_pin'].get(w, ()):
                out.append((t, i, p, d))
        return out


if __name__ == '__main__':
    f = Fabric()
    print('tiles', len(f.tiles), 'tile types', len(f.tt), 'conn keys', len(f.conn))
    for a in sys.argv[1:]:
        t, w = a.split('/')
        print(a, '->', f.node(t, w))

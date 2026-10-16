#!/usr/bin/env python3
"""Minimal MPEG-2 video MB-layer parser for debugging mpeg2_cuda output.

Handles the subset the encoder emits: frame pictures, I/P, 4:2:0,
intra_dc_precision 0, no MB-level quantiser changes.

usage: m2vparse.py <file.m2v> <picture index> <slice row> [--quiet]
"""
import json, os, sys

T = json.load(open(os.path.join(os.path.dirname(__file__), "m2v_tables.json")))


def vlc(pairs, values=None):
    d = {}
    for i, (code, length) in enumerate(pairs):
        if length:
            d[(length, code)] = i if values is None else values[i]
    return d


ADDR = vlc(T["addr"][:33])
CBP = vlc(T["cbp"])
MV = vlc(T["mv"])
DCL, DCC = vlc(T["dcl"]), vlc(T["dcc"])
N = len(T["run"])


def ac_table(name):
    vals = [(T["run"][i], T["level"][i]) for i in range(N)] + ["ESC", "EOB"]
    return vlc(T[name], vals)


B14, B15 = ac_table("b14"), ac_table("b15")
PTYPE = {(1, 1): "MC_CODED", (2, 1): "NOMC_CODED", (3, 1): "MC_NOTCODED",
         (5, 3): "INTRA", (5, 2): "MC_CODED_Q", (5, 1): "NOMC_CODED_Q", (6, 1): "INTRA_Q"}


class Bits:
    def __init__(self, data, pos=0):
        self.d, self.p = data, pos

    def peek(self, n):
        v = 0
        for i in range(n):
            byte = self.p + i
            v = (v << 1) | ((self.d[byte >> 3] >> (7 - (byte & 7))) & 1 if (byte >> 3) < len(self.d) else 0)
        return v

    def get(self, n):
        v = self.peek(n)
        self.p += n
        return v

    def vlc(self, table, maxlen=17, what="?"):
        for n in range(1, maxlen + 1):
            k = (n, self.peek(n))
            if k in table:
                self.p += n
                return table[k]
        raise ValueError(f"bad {what} VLC at bit {self.p}: {self.peek(16):016b}")


def motion(b, fcode):
    code = b.vlc(MV, 11, "motion_code")
    if code == 0:
        return 0
    sign = b.get(1)
    r = b.get(fcode - 1) if fcode > 1 else 0
    v = ((code - 1) << (fcode - 1)) + r + 1
    return -v if sign else v


def block(b, intra, chroma):
    coefs = 0
    if intra:
        size = b.vlc(DCC if chroma else DCL, 10, "dc")
        if size:
            b.get(size)
        tab = B15
    else:
        tab = B14
        if b.peek(1) == 1:            # first coefficient '1s'
            b.get(2)
            coefs += 1
    while True:
        e = b.vlc(tab, 17, "ac")
        if e == "EOB":
            return coefs
        if e == "ESC":
            b.get(6)
            b.get(12)
        else:
            b.get(1)
        coefs += 1


def parse(path, pic_index, row, quiet=False):
    data = open(path, "rb").read()
    pic = -1
    ptype = fcode = fpfd = None
    i = 0
    while i < len(data) - 4:
        if data[i:i + 3] != b"\x00\x00\x01":
            i += 1
            continue
        sc = data[i + 3]
        b = Bits(data, (i + 4) * 8)
        if sc == 0x00:
            pic += 1
            b.get(10)
            ptype = b.get(3)
        elif sc == 0xB5 and b.peek(4) == 8:
            b.get(4)
            fcode = b.get(4)
            b.get(12)
            b.get(2); b.get(2); b.get(1)
            fpfd = b.get(1)
        elif 0x01 <= sc <= 0xAF and pic == pic_index and sc - 1 == row:
            q = b.get(5)
            b.get(1)
            print(f"picture {pic} type {'IPB'[ptype - 1]} fcode {fcode} fpfd {fpfd} slice {row} q {q}")
            mbx = -1
            end = data.find(b"\x00\x00\x01", i + 4)
            end_bit = (end if end > 0 else len(data)) * 8
            while b.p < end_bit - 8 or b.peek(min(8, end_bit - b.p)):
                start = b.p
                inc = 0
                while b.peek(11) == 0x008:
                    b.get(11)
                    inc += 33
                inc += b.vlc(ADDR, 11, "addr") + 1
                mbx += inc
                if ptype == 1:
                    t = "INTRA" if b.get(1) else ("INTRA_Q" if b.get(1) else None)
                else:
                    t = None
                    for n in range(1, 7):
                        if (n, b.peek(n)) in PTYPE:
                            t = PTYPE[(n, b.peek(n))]
                            b.get(n)
                            break
                if t is None:
                    raise ValueError(f"bad mb_type at mb {mbx}")
                intra = t.startswith("INTRA")
                fwd = t.startswith("MC")
                pattern = "CODED" in t and "NOTCODED" not in t
                mt = dct = None
                if fwd and not fpfd:
                    mt = b.get(2)
                if (intra or pattern) and not fpfd:
                    dct = b.get(1)
                if t.endswith("_Q"):
                    b.get(5)
                mvs = []
                if fwd:
                    if mt in (None, 2):
                        mvs = [(motion(b, fcode), motion(b, fcode))]
                    elif mt == 1:
                        for _ in range(2):
                            sel = b.get(1)
                            mvs.append((sel, motion(b, fcode), motion(b, fcode)))
                    else:
                        raise ValueError(f"motion_type {mt} at mb {mbx}")
                cbp = 63 if intra else (b.vlc(CBP, 9, "cbp") if pattern else 0)
                ncoef = []
                for blk in range(6):
                    if cbp & (32 >> blk):
                        ncoef.append(block(b, intra, blk >= 4))
                if not quiet:
                    print(f"  mb {mbx:3d} +{b.p - start:5d} bits {t:12s} mt={mt} dct={dct} "
                          f"mv={mvs} cbp={cbp} coefs={ncoef}")
            return
        i += 4


if __name__ == "__main__":
    parse(sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), "--quiet" in sys.argv)

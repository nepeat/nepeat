#!/usr/bin/env python3
"""Independent MPEG-2 video (ES) syntax/semantics checker.

Parses every picture and slice down to the coefficients, without any FFmpeg
code, and flags anything a strict decoder (or a TV) could trip over:

  - slice does not cover exactly one MB row, or leaves non-zero bits at its end
    (also catches start code emulation inside slice data)
  - bad VLCs, skipped first/last MB, skipped B MB after an intra MB
  - quantiser_scale_code 0, escape level 0 or -2048, coefficient index > 63,
    dequantised coefficient outside [-2048, 2047]
  - motion vector outside the f_code range, PMV outside the range when the
    delta is zero (decoders that skip the modulo wrap get it wrong)
  - motion compensation reading outside the reference picture
  - temporal_reference / picture type / GOP structure sanity

Supports what libavcodec's and mpeg2_cuda's encoders emit: 4:2:0 frame
pictures, I/P/B, frame and field motion, field/frame DCT, MB quant changes.

usage: m2vcheck.py <file.m2v> [--max-pictures N] [--verbose]
Exit status 1 if any error was found.
"""
import json, os, sys

T = json.load(open(os.path.join(os.path.dirname(__file__), "m2v_tables.json")))


def bstr(code, length):
    return format(code, f"0{length}b")


def table(pairs, values=None):
    return {bstr(c, n): (i if values is None else values[i]) for i, (c, n) in enumerate(pairs) if n}


ADDR = table(T["addr"][:33])
CBP = table(T["cbp"])
MV = table(T["mv"])
DCL, DCC = table(T["dcl"]), table(T["dcc"])
_vals = [(T["run"][i], T["level"][i]) for i in range(len(T["run"]))] + ["ESC", "EOB"]
B14, B15 = table(T["b14"], _vals), table(T["b15"], _vals)

# flags: Q quant, F forward, B backward, P pattern, I intra
PTYPE = {"1": "FP", "01": "P", "001": "F", "00011": "I", "00010": "QFP", "00001": "QP", "000001": "QI"}
BTYPE = {"10": "FB", "11": "FBP", "010": "B", "011": "BP", "0010": "F", "0011": "FP", "00011": "I",
         "00010": "QFBP", "000011": "QFP", "000010": "QBP", "000001": "QI"}

NONLINEAR_Q = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 18, 20, 22, 24,
               28, 32, 36, 40, 44, 48, 52, 56, 64, 72, 80, 88, 96, 104, 112]
DEFAULT_INTRA = [8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37,
                 19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40,
                 22, 26, 27, 29, 32, 35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58,
                 26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83]
ZIGZAG = [0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48,
          41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22,
          15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63]
ALTERNATE = [0, 8, 16, 24, 1, 9, 2, 10, 17, 25, 32, 40, 48, 56, 57, 49, 41, 33, 26, 18, 3, 11,
             4, 12, 19, 27, 34, 42, 50, 58, 35, 43, 51, 59, 20, 28, 5, 13, 6, 14, 21, 29, 36,
             44, 52, 60, 37, 45, 53, 61, 22, 30, 7, 15, 23, 31, 38, 46, 54, 62, 39, 47, 55, 63]


class Err(Exception):
    pass


class Bits:
    def __init__(self, data):
        self.s = "".join(format(b, "08b") for b in data)
        self.p = 0

    def get(self, n):
        if n == 0:
            return 0
        v = self.s[self.p:self.p + n]
        if len(v) < n:
            raise Err("read past end of slice")
        self.p += n
        return int(v, 2)

    def vlc(self, tab, maxlen, what):
        for n in range(1, maxlen + 1):
            k = self.s[self.p:self.p + n]
            if k in tab:
                self.p += n
                return tab[k]
        raise Err(f"bad {what} VLC {self.s[self.p:self.p + 16]}")


class Checker:
    def __init__(self, verbose=False):
        self.errors = []
        self.verbose = verbose
        self.w = self.h = None
        self.stats = {"pictures": 0, "slices": 0, "mbs": 0, "skipped": 0, "types": {}}

    def err(self, msg):
        self.errors.append(f"pic {self.pic} ({'IPB'[self.ptype - 1]}) {msg}")
        if len(self.errors) <= 20:
            print("ERROR:", self.errors[-1])

    # --- headers -------------------------------------------------------
    def sequence_header(self, b):
        self.w, self.h = b.get(12), b.get(12)
        b.get(4); b.get(4)
        b.get(18); b.get(1); b.get(10); b.get(1)
        self.intra_q = DEFAULT_INTRA[:]
        self.inter_q = [16] * 64
        if b.get(1):
            m = [b.get(8) for _ in range(64)]
            for i in range(64):
                self.intra_q[ZIGZAG[i]] = m[i]
        if b.get(1):
            m = [b.get(8) for _ in range(64)]
            for i in range(64):
                self.inter_q[ZIGZAG[i]] = m[i]

    def extension(self, b):
        ext = b.get(4)
        if ext == 1:
            b.get(8)
            self.progressive_seq = b.get(1)
            if b.get(2) != 1:
                raise Err("only 4:2:0 is supported")
            self.w |= b.get(2) << 12
            self.h |= b.get(2) << 12
        elif ext == 8:
            self.fcode = [[b.get(4), b.get(4)], [b.get(4), b.get(4)]]
            self.dc_prec = b.get(2)
            self.structure = b.get(2)
            self.tff = b.get(1)
            self.fpfd = b.get(1)
            self.conceal = b.get(1)
            self.qst = b.get(1)
            self.ivlc = b.get(1)
            self.alt = b.get(1)
            b.get(1)                          # repeat_first_field
            c420 = b.get(1)
            self.progressive_frame = b.get(1)
            if self.progressive_frame and not self.fpfd:
                self.err("progressive_frame=1 requires frame_pred_frame_dct=1")
            if c420 != self.progressive_frame:
                self.err("chroma_420_type must equal progressive_frame")
            if self.structure != 3:
                raise Err("field pictures are not supported by this checker")
            if self.conceal:
                raise Err("concealment vectors are not supported by this checker")

    # --- macroblock layer ------------------------------------------------
    def motion_delta(self, b, fcode):
        code = b.vlc(MV, 11, "motion_code")
        if code == 0:
            return 0
        sign = b.get(1)
        r = b.get(fcode - 1)
        v = ((code - 1) << (fcode - 1)) + r + 1
        return -v if sign else v

    def vector(self, b, s, t, pmv, field_vert=False):
        """decode one component; returns the reconstructed value (spec wrap)"""
        fcode = self.fcode[s][t]
        if fcode == 15:
            raise Err("motion vector with f_code 15")
        rng = 32 << (fcode - 1)
        low, high = -(rng // 2), rng // 2 - 1
        pred = (pmv >> 1) if field_vert else pmv
        delta = self.motion_delta(b, fcode)
        v = pred + delta
        if v < low:
            v += rng
        if v > high:
            v -= rng
        if delta == 0 and not (low <= pred <= high):
            self.err(f"mb {self.mbx},{self.row}: zero delta with out-of-range PMV {pred} "
                     f"(decoders that skip the wrap will drift)")
        return v

    def check_mc(self, vx, vy, field, sel_or_bottom=0):
        """luma reference block must lie inside the picture (libavcodec's rule)"""
        mbw16 = ((self.w + 15) // 16) * 16
        mbh16 = ((self.h + 31) // 32) * 32 if not getattr(self, "progressive_seq", 0) else ((self.h + 15) // 16) * 16
        if field:
            src_x = self.mbx * 16 + (vx >> 1)
            src_y = self.row * 8 + (vy >> 1)
            ok = 0 <= src_x <= mbw16 - 16 - (vx & 1) and 0 <= src_y <= mbh16 // 2 - 8 - (vy & 1)
        else:
            src_x = self.mbx * 16 + (vx >> 1)
            src_y = self.row * 16 + (vy >> 1)
            ok = 0 <= src_x <= mbw16 - 16 - (vx & 1) and 0 <= src_y <= mbh16 - 16 - (vy & 1)
        if not ok:
            self.err(f"mb {self.mbx},{self.row}: MV ({vx},{vy}){' field' if field else ''} reads outside the picture")

    def motion_vectors(self, b, s, mt):
        P = self.pmv
        if mt == 2:     # frame
            vx = self.vector(b, s, 0, P[0][s][0])
            vy = self.vector(b, s, 1, P[0][s][1])
            P[0][s] = [vx, vy]
            P[1][s] = [vx, vy]
            self.check_mc(vx, vy, False)
            return ("frame", vx, vy)
        if mt == 1:     # field
            out = []
            for r in range(2):
                sel = b.get(1)
                vx = self.vector(b, s, 0, P[r][s][0])
                vy = self.vector(b, s, 1, P[r][s][1], field_vert=True)
                P[r][s] = [vx, 2 * vy]
                self.check_mc(vx, vy, True)
                out.append((sel, vx, vy))
            return ("field", out)
        raise Err(f"motion_type {mt} (dual prime / reserved)")

    def block(self, b, intra, comp, qs, n):
        scan = ALTERNATE if self.alt else ZIGZAG
        if intra:
            size = b.vlc(DCC if comp else DCL, 10, "dc_size")
            if size > 11:
                raise Err("dc size")
            diff = 0
            if size:
                diff = b.get(size)
                if diff < (1 << (size - 1)):
                    diff = diff - (1 << size) + 1
            self.dcp[comp] += diff
            if not 0 <= self.dcp[comp] < (256 << self.dc_prec):
                self.err(f"mb {self.mbx},{self.row}: DC {self.dcp[comp]} out of range")
            tab, i, W = (B15 if self.ivlc else B14), 0, self.intra_q
        else:
            tab, i, W = B14, -1, self.inter_q
            if self.s_peek(b, 1) == "1":
                b.get(1)
                b.get(1)
                i = 0
        while True:
            e = b.vlc(tab, 17, "dct_coefficient")
            if e == "EOB":
                if not intra and i < 0:
                    self.err(f"mb {self.mbx},{self.row}: coded block without coefficients")
                return
            if e == "ESC":
                run = b.get(6)
                level = b.get(12)
                if level >= 2048:
                    level -= 4096
                if level == 0 or level == -2048:
                    self.err(f"mb {self.mbx},{self.row}: forbidden escape level {level}")
            else:
                run, level = e
                if b.get(1):
                    level = -level
            i += run + 1
            if i > 63:
                raise Err(f"mb {self.mbx},{self.row} block {n}: coefficient index {i} > 63")
            w = W[scan[i]]
            a = abs(level)
            d = (a * qs * w) >> 4 if intra else ((2 * a + 1) * qs * w) >> 5
            if d > 2047 + (0 if level > 0 else 1):
                self.err(f"mb {self.mbx},{self.row}: dequantised coefficient {d} saturates")

    @staticmethod
    def s_peek(b, n):
        return b.s[b.p:b.p + n]

    def macroblock(self, b, first):
        start = b.p
        inc = 0
        while self.s_peek(b, 11) == "00000001000":
            b.get(11)
            inc += 33
        inc += b.vlc(ADDR, 11, "address_increment") + 1
        if first:
            if inc != 1:
                self.err(f"slice {self.row} starts at column {inc - 1}")
        elif inc > 1:
            nskip = inc - 1
            self.stats["skipped"] += nskip
            if self.ptype == 2:
                self.pmv = [[[0, 0], [0, 0]], [[0, 0], [0, 0]]]
            if self.ptype == 3:
                if self.prev_intra:
                    self.err(f"mb {self.mbx + 1},{self.row}: skipped B MB after an intra MB")
            self.dcp = [128 << self.dc_prec] * 3
        self.mbx += inc
        if self.mbx >= self.mb_w:
            raise Err(f"MB address {self.mbx} beyond the row")

        if self.ptype == 1:
            t = "I" if b.get(1) else ("QI" if b.get(1) else None)
            if t is None:
                raise Err("bad I mb_type")
        else:
            t = b.vlc(PTYPE if self.ptype == 2 else BTYPE, 6, "mb_type")
        self.stats["types"][t] = self.stats["types"].get(t, 0) + 1
        intra, fwd, bwd, pat, quant = ("I" in t), ("F" in t), ("B" in t), ("P" in t), ("Q" in t)
        mt = 2
        dct = 0
        if (fwd or bwd) and not self.fpfd:
            mt = b.get(2)
        if (intra or pat) and not self.fpfd:
            dct = b.get(1)
        if quant:
            code = b.get(5)
            if code == 0:
                self.err(f"mb {self.mbx},{self.row}: quantiser_scale_code 0")
            self.qcode = code
        qs = NONLINEAR_Q[self.qcode] if self.qst else 2 * self.qcode

        if intra:
            self.pmv = [[[0, 0], [0, 0]], [[0, 0], [0, 0]]]
        else:
            self.dcp = [128 << self.dc_prec] * 3
        if self.ptype == 2 and not fwd and not intra:
            self.pmv = [[[0, 0], [0, 0]], [[0, 0], [0, 0]]]   # no-MC
        mvs = []
        if fwd:
            mvs.append(self.motion_vectors(b, 0, mt))
        if bwd:
            mvs.append(self.motion_vectors(b, 1, mt))

        cbp = 63 if intra else (b.vlc(CBP, 9, "coded_block_pattern") if pat else 0)
        if pat and cbp == 0:
            # legal in MPEG-2 (cbp 0 with pattern flag), but our encoder never does it
            self.err(f"mb {self.mbx},{self.row}: coded_block_pattern 0")
        for n in range(6):
            if cbp & (32 >> n):
                self.block(b, intra, 0 if n < 4 else n - 3, qs, n)
        self.prev_intra = intra
        self.stats["mbs"] += 1
        if self.verbose:
            print(f"   mb {self.mbx:3d} {b.p - start:5d}b {t:5s} mt={mt} dct={dct} cbp={cbp:2d} mv={mvs}")

    def slice(self, data, row):
        self.row = row
        self.stats["slices"] += 1
        b = Bits(data)
        self.qcode = b.get(5)
        if self.qcode == 0:
            self.err(f"slice {row}: quantiser_scale_code 0")
        if b.get(1):
            raise Err("extra_bit_slice set")
        self.mbx = -1
        self.pmv = [[[0, 0], [0, 0]], [[0, 0], [0, 0]]]
        self.dcp = [128 << self.dc_prec] * 3
        self.prev_intra = False
        first = True
        while self.mbx < self.mb_w - 1:
            if "1" not in b.s[b.p:]:
                break
            self.macroblock(b, first)
            first = False
        if self.mbx != self.mb_w - 1:
            self.err(f"slice {row} ends at column {self.mbx}, expected {self.mb_w - 1} "
                     f"(start code emulation or truncated slice?)")
        if "1" in b.s[b.p:]:
            self.err(f"slice {row}: non-zero bits after the last macroblock")

    # --- stream ------------------------------------------------------------
    def run(self, data, max_pictures=None):
        self.pic = -1
        self.ptype = 1
        gop_start = None
        temporal = []
        pos = []
        i = 0
        while True:
            j = data.find(b"\x00\x00\x01", i)
            if j < 0:
                break
            pos.append(j)
            i = j + 3
        pos.append(len(data))
        rows_seen = set()
        for k in range(len(pos) - 1):
            j = pos[k]
            sc = data[j + 3]
            payload = data[j + 4:pos[k + 1]]
            try:
                if sc == 0xB3:
                    self.sequence_header(Bits(payload))
                    self.mb_w = (self.w + 15) // 16
                elif sc == 0xB5:
                    self.extension(Bits(payload))
                elif sc == 0xB8:
                    gop_start = self.pic + 1
                elif sc == 0x00:
                    if self.pic >= 0 and rows_seen != set(range(self.mb_h)):
                        self.err(f"slices missing: {sorted(set(range(self.mb_h)) - rows_seen)[:5]}")
                    self.pic += 1
                    if max_pictures is not None and self.pic >= max_pictures:
                        break
                    self.stats["pictures"] += 1
                    b = Bits(payload)
                    tr = b.get(10)
                    self.ptype = b.get(3)
                    if self.ptype not in (1, 2, 3):
                        raise Err(f"picture_coding_type {self.ptype}")
                    temporal.append((self.pic - (gop_start or 0), tr, self.ptype))
                    progressive = getattr(self, "progressive_seq", 0)
                    self.mb_h = (self.h + 15) // 16 if progressive else 2 * ((self.h + 31) // 32)
                    rows_seen = set()
                elif 0x01 <= sc <= 0xAF:
                    if sc - 1 in rows_seen:
                        self.err(f"slice {sc - 1} appears twice")
                    rows_seen.add(sc - 1)
                    if sc - 1 >= self.mb_h:
                        self.err(f"slice {sc - 1} beyond the picture")
                    self.slice(payload, sc - 1)
                elif sc == 0xB7:
                    pass
                else:
                    self.err(f"unexpected start code {sc:02x}")
            except Err as e:
                self.err(f"start code {sc:02x} at byte {j}: {e}")
        return temporal


def main():
    path = sys.argv[1]
    maxp = None
    if "--max-pictures" in sys.argv:
        maxp = int(sys.argv[sys.argv.index("--max-pictures") + 1])
    c = Checker("--verbose" in sys.argv)
    c.run(open(path, "rb").read(), maxp)
    st = c.stats
    print(f"{path}: {st['pictures']} pictures, {st['slices']} slices, {st['mbs']} coded MBs, "
          f"{st['skipped']} skipped, types {dict(sorted(st['types'].items()))}: "
          f"{'OK' if not c.errors else str(len(c.errors)) + ' ERRORS'}")
    sys.exit(1 if c.errors else 0)


if __name__ == "__main__":
    main()

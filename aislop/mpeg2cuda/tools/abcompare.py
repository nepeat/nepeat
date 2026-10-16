#!/usr/bin/env python3
"""A/B comparison of two decoded encodes (A = software mpeg2video, B =
mpeg2_cuda) against their common source, all raw yuv420p of the same size.

Checks that B behaves like A, not just that it is good:
  - same number of frames
  - B's luma PSNR within --psnr-tol dB of A's (whole clip, MSE-averaged)
  - B's worst frame within --worst-tol dB of A's worst frame
  - mean Y/U/V of the two decodes within --mean-tol code values (catches range
    / colour / chroma-plane mistakes that PSNR alone can hide)
  - temporal alignment: frame i of B is closer to source frame i than to
    source i-1/i+1, duplicates skipped (catches off-by-one reordering), and
    field alignment: B is closer to the source than to the source with its
    fields swapped (catches field order errors)

usage: abcompare.py <src.yuv> <a.yuv> <b.yuv> <w> <h> [--json out.json]
       [--psnr-tol 0.25] [--worst-tol 1.5] [--mean-tol 0.75]
Exit status 1 if a check fails.
"""
import argparse, json, sys
import numpy as np

ap = argparse.ArgumentParser()
ap.add_argument("src"); ap.add_argument("a"); ap.add_argument("b")
ap.add_argument("w", type=int); ap.add_argument("h", type=int)
ap.add_argument("--json")
ap.add_argument("--psnr-tol", type=float, default=0.25)
ap.add_argument("--worst-tol", type=float, default=1.5)
ap.add_argument("--mean-tol", type=float, default=0.75)
o = ap.parse_args()

w, h = o.w, o.h
fs = w * h * 3 // 2
mm = {k: np.memmap(p, np.uint8, "r") for k, p in (("src", o.src), ("a", o.a), ("b", o.b))}
n = {k: len(v) // fs for k, v in mm.items()}


def planes(k, i):
    f = mm[k][i * fs:(i + 1) * fs]
    y = f[:w * h].reshape(h, w)
    u = f[w * h:w * h + w * h // 4].reshape(h // 2, w // 2)
    v = f[w * h + w * h // 4:].reshape(h // 2, w // 2)
    return y, u, v


def mse(p, q):
    d = p.astype(np.int32) - q.astype(np.int32)
    return float(np.mean(d * d))


def psnr(m):
    return 99.0 if m == 0 else 10 * np.log10(255 * 255 / m)


nf = min(n.values())
res = {"frames": n, "checks": {}}
mse_a, mse_b, fa, fb = [], [], [], []
means = {"a": np.zeros(3), "b": np.zeros(3)}
align_ok = field_ok = align_n = field_n = 0
for i in range(nf):
    sy = planes("src", i)[0]
    ya, ua, va = planes("a", i)
    yb, ub, vb = planes("b", i)
    ma, mb_ = mse(ya, sy), mse(yb, sy)
    mse_a.append(ma); mse_b.append(mb_)
    fa.append(psnr(ma)); fb.append(psnr(mb_))
    means["a"] += [ya.mean(), ua.mean(), va.mean()]
    means["b"] += [yb.mean(), ub.mean(), vb.mean()]
    # temporal alignment against the source: B's frame i must be at least as
    # close to source frame i as to source i-1 / i+1. Neighbours that are
    # (near) duplicates of frame i are skipped — frame-rate-converted sources
    # repeat frames, which makes the comparison a coin flip.
    d0 = mse(yb, sy)
    nb = []
    for j in (i - 1, i + 1):
        if 0 <= j < nf:
            sj = planes("src", j)[0]
            if mse(sj, sy) > 1.0:
                nb.append(mse(yb, sj))
    if nb:
        align_n += 1
        align_ok += all(d0 <= x for x in nb)
    # field alignment: the source with its fields swapped must be further away
    sw = np.empty_like(sy)
    sw[0::2], sw[1::2] = sy[1::2], sy[0::2]
    if mse(sw, sy) > 1.0:
        field_n += 1
        field_ok += d0 <= mse(yb, sw)

pa, pb = psnr(np.mean(mse_a)), psnr(np.mean(mse_b))
means = {k: (v / nf).round(3).tolist() for k, v in means.items()}
mdiff = max(abs(x - y) for x, y in zip(means["a"], means["b"]))
res.update(psnr_a=round(pa, 3), psnr_b=round(pb, 3), worst_a=round(min(fa), 2),
           worst_b=round(min(fb), 2), means=means, mean_diff=round(mdiff, 3),
           temporal_aligned=f"{align_ok}/{align_n}", field_aligned=f"{field_ok}/{field_n}")

c = res["checks"]
c["frame_count"] = bool(len(set(n.values())) == 1)
c["psnr"] = bool(pb >= pa - o.psnr_tol)
c["worst_frame"] = bool(min(fb) >= min(fa) - o.worst_tol)
c["mean_levels"] = bool(mdiff <= o.mean_tol)
c["temporal_alignment"] = bool(align_ok >= 0.9 * align_n)
c["field_alignment"] = bool(field_ok >= 0.9 * field_n)
res["pass"] = all(c.values())

if o.json:
    json.dump(res, open(o.json, "w"), indent=1)
bad = [k for k, v in c.items() if not v]
print(f"PSNR-Y A {pa:.2f} B {pb:.2f} ({pb - pa:+.2f})  worst A {min(fa):.2f} B {min(fb):.2f}  "
      f"mean diff {mdiff:.2f}  aligned {align_ok}/{align_n} fields {field_ok}/{field_n}  "
      f"{'OK' if not bad else 'FAIL: ' + ', '.join(bad)}")
sys.exit(0 if res["pass"] else 1)

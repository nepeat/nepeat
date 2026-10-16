#!/usr/bin/env python3
"""Bjontegaard delta rate between two RD curves, piecewise-linear in
(PSNR, log rate) instead of the classic cubic fit, which overshoots badly
with few points.
usage: bdrate.py <ref.csv> <test.csv>   (csv lines: kbps,psnr)
Negative = test needs fewer bits for the same PSNR."""
import sys
import numpy as np

def load(p):
    pts = sorted((float(q), np.log(float(r))) for r, q in (l.split(",") for l in open(p) if l.strip()))
    return np.array([a for a, _ in pts]), np.array([b for _, b in pts])

(p1, r1), (p2, r2) = load(sys.argv[1]), load(sys.argv[2])
lo, hi = max(p1.min(), p2.min()), min(p1.max(), p2.max())
if hi <= lo:
    print("BD-rate n/a (no PSNR overlap)")
    sys.exit(1)
grid = np.linspace(lo, hi, 200)
d = np.trapezoid(np.interp(grid, p2, r2) - np.interp(grid, p1, r1), grid) / (hi - lo)
print(f"BD-rate {(np.exp(d) - 1) * 100:+.2f}%  (overlap {lo:.2f}-{hi:.2f} dB)")

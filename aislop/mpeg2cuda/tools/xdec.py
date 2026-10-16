#!/usr/bin/env python3
"""Cross-decode with libmpeg2 (mpeg2dec) and compare against FFmpeg's decode.

The two decoders use different IDCTs, so they drift slightly within a GOP;
anything below the PSNR threshold means they disagree about the bitstream.

usage: xdec.py <file.m2v> <ffmpeg_decoded.yuv> <width> <height> [min_psnr]
"""
import subprocess, sys
import numpy as np

path, ref, w, h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
thr = float(sys.argv[5]) if len(sys.argv) > 5 else 40.0
raw = subprocess.run(["mpeg2dec", "-o", "pgmpipe", path], capture_output=True).stdout

frames, i = [], 0
while i < len(raw):
    # "P5\n<w> <h>\n255\n"
    parts = []
    while len(parts) < 4:
        j = raw.index(b"\n", i)
        parts += raw[i:j].split()
        i = j + 1
    cw, ch = int(parts[1]), int(parts[2])
    img = np.frombuffer(raw, np.uint8, cw * ch, i).reshape(ch, cw)
    i += cw * ch
    frames.append(img[: (ch * 2) // 3][:h, :w])

fs = w * h * 3 // 2
ff = np.fromfile(ref, np.uint8)
n_ff = len(ff) // fs
# libmpeg2 holds back the last picture unless the stream ends with a
# sequence_end_code; tolerate exactly that one
if len(frames) not in (n_ff, n_ff - 1):
    print(f"FAIL frame count: ffmpeg {n_ff}, libmpeg2 {len(frames)}")
    sys.exit(1)
n_ff = len(frames)
worst = 99.0
for k in range(n_ff):
    a = ff[k * fs:k * fs + w * h].reshape(h, w).astype(np.float64)
    mse = np.mean((a - frames[k]) ** 2)
    p = 99.0 if mse == 0 else 10 * np.log10(255 ** 2 / mse)
    worst = min(worst, p)
print(f"{'OK' if worst >= thr else 'FAIL'} libmpeg2 vs ffmpeg: {n_ff} frames, worst luma PSNR {worst:.1f} dB")
sys.exit(0 if worst >= thr else 1)

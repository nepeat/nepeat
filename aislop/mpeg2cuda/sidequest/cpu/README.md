# Side quest: how fast can CPU MPEG-2 get?

Not part of the GPU journey. The question: how fast is FFmpeg's software
`mpeg2video` on this box (Ryzen 9 7950X, 16C/32T, Zen 4), what limits it, and
can FFmpeg itself be made faster. Upstream n9.0.2 plus the patch in this
directory, built minimal in `build/cpu` (`run.sh setup`), so the live
channels' binary is never touched.

Settings throughout match the live channels: 1080, 15M CBR (bufsize 7.5M),
GOP 15, `+ilme+ildct`, alternate scan, intra VLC. Clips: touchdown 1080p
(progressive source) and park_joy 1080i (truly interlaced). Encoder-only fps
from `tools/m2c_bench` (60 preloaded frames), quality from `scripts/psnr.sh`
over 150 frames.

## Where it stands

| | 1 thread | 1 instance, threads=auto (16) / 32 | whole box (32 x threads=1) |
|---|---|---|---|
| I/P, touchdown | 78 fps | 506 / 666 | 1332 |
| I/P, park_joy | 73 | | 1194 |
| B2, touchdown | 42 | 302 | 732 |
| B2, park_joy | 38 | | 632 |

Whole box, 16 instances on 16 cores give ~1100 fps; SMT adds ~20%. Encode
only, ~40-44 channels of 1080i at 29.97 I/P. The RTX 3090 does ~4800 fps
here, but live channels are NVDEC-bound at ~24 either way.

## What the time goes to

One thread, touchdown I/P (`perf`; IPC ~1.95, L1 misses ~10% of loads,
practically no LLC misses: not memory-bound):

- motion estimation ~65%: `ff_sad16_sse2` alone 28%, half-pel SADs
  (`approx_xy2`/`x2`/`y2`) ~16%, EPZS/`cmp_fpel` C overhead ~12%
- of which field ME (`interlaced_search`: four 16x8 EPZS searches plus four
  half-pel refinements per MB, run for every MB) is ~40% of total time:
  `flags=+ildct` alone (no ilme) is 1.7x faster
- the rest: VLC (`mpeg1_encode_block`) ~5%, DCT/quant/IDCT ~10%, ildct
  decision (`vsad16_approx`) ~4%, diff/put pixels

Knobs (all at the live settings, one thread):

| change | touchdown | park_joy 1080i |
|---|---|---|
| none | 78 fps, 37.215 dB | 73 fps, 25.130 dB, SSIM 0.7230 |
| no ilme | 134, 37.217 | 131, **24.604**, 0.7005 |
| no ilme, no ildct | 140, 37.217 | 139, 24.466 |
| mbd=2 | 48 | 45 |
| dia_size=-1 | 47 | 43 |
| last_pred=2 | 76 | 69 |
| me_range=16 | 80 | 68 (and overshoots the rate) |

So the defaults are already the cheapest search, and dropping field ME costs
0.5 dB on interlaced content. Every other knob is slower.

## What doesn't help: wider SIMD

The SAD kernels are already tight (2 rows per iteration, current block as the
`psadbw` memory operand). Both blocks are strided 16-pixel rows, so a 16x16
SAD is ~32 loads at any vector width; AVX2/AVX-512 would only turn plain loads
into load+insert pairs. With IPC ~2 and the load ports busy there is no
meaningful gain without changing the `me_cmp` calling convention (packed
current block), which touches every user of it.

## What does: `field_me_thresh` (patch 0001)

Skip `interlaced_search` for a macroblock when its best frame score
(SAD + MV penalty) is already below the threshold. Upstream even marks the
spot (`//FIXME varc/d checks`). Default 0 = unchanged, bit-exact (md5 checked
for I/P and B2 on both clips); gates the simple macroblock decision path and B
pictures.

One thread:

| field_me_thresh | touchdown I/P | park_joy I/P | touchdown B2 | park_joy B2 |
|---|---|---|---|---|
| 0 | 78, 37.215 / 0.9119 | 73, 25.130 / 0.7230 | 42, 37.567 / 0.9173 | 38, 25.086 / 0.7239 |
| 1024 | 118, 37.213 | 78, 25.134 / 0.7230 | | |
| **2048** | **127, 37.220 / 0.9119** | **85.5, 25.128 / 0.7221** | **67, 37.572 / 0.9174** | **44, 25.093 / 0.7236** |
| 3072 | 130, 37.208 | 91, 25.113 / 0.7206 | | |
| 4096 | 133, 37.212 | 100.5, 25.057 / 0.7179 | 70, 37.577 | 54, 25.058 / 0.7211 |

At 8M CBR, 2048 gives touchdown 82 -> 144 fps and park_joy 53 -> 62 fps,
both within 0.01 dB (park_joy can't hold 8M at all and overshoots to ~12M in
every run, gated or not).

The threshold was tuned on two clips: progressive, and high-motion
interlaced. Moderate-motion true 1080i (a slow pan) is untested and is where
a fixed threshold could skip field ME that still pays.

Whole box (32 x threads=1), threshold 2048:

| | before | after |
|---|---|---|
| touchdown I/P | 1332 | **1776** (+33%) |
| park_joy I/P | 1194 | **1353** (+13%) |
| touchdown B2 | 732 | **1172** (+60%) |
| park_joy B2 | 632 | **743** (+18%) |

Also tried: skipping field ME when the source MB shows no combing (frame vs
field vertical SAD, like the ildct decision). Worse trade-off at every point
than the score threshold (e.g. -0.155 dB on park_joy for +10%), dropped.

## Not done

- the RD macroblock decision path (`mbd>0`) is not gated
- the threshold is in absolute SAD units; scaling it with lambda might
  generalise better across bitrates (2048 held at 8M and 15M)
- batching the diamond's 4 neighbours into one SAD call (saves call and map
  overhead, ~10% at most) and a packed-current-block SAD convention
- CPU decode for a GPU-less box (the live pipeline decodes on NVDEC today)

## Measurement notes

- `nice -n 19` plus `taskset` halves multi-instance throughput (the scheduler
  stacks the low-weight threads); 16 pinned instances looked like 8 cores.
  Whole-box runs here are unpinned.
- the psnr filter's frame pairing on raw ES is racy; always use
  `scripts/psnr.sh`.

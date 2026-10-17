# mpeg2cuda — progress

Start here. Layout:
- `flake.nix` — dev shell (CUDA 13.2 pinned to match the 595.x driver; nvcc 13.4
  emits PTX 9.4 the driver refuses to JIT). Run everything via `nix develop`.
  `nix build .#ffmpeg` = upstream FFmpeg 9.0.2 release tarball + `patches/*.patch`
  (flags in `scripts/ffmpeg_flags`, shared with `scripts/configure.sh`); the
  binary carries the driver runpath, no LD_LIBRARY_PATH needed.
- `patches/` — the encoder, as a patch series on n9.0.2. This is the source of
  truth. `ffmpeg/` (gitignored) is a scratch dev tree: `scripts/ffmpeg_tree.sh`
  makes it (clone tag + `git am patches/*`, branch `mpeg2_cuda`); edit and commit
  there, then `scripts/export_patches.sh` writes the series back. Encoder:
  `libavcodec/mpeg2_cuda.{c,cu,h}`.
- `scripts/configure.sh`, `scripts/build.sh` (forces PTX rebuild; nvcc has no
  dep tracking), `scripts/verify.sh` (bit-exact recon check),
  `scripts/compare.sh` (CPU vs GPU quality at given RC args),
  `scripts/build_bench.sh` + `tools/m2c_bench.c` (encoder-only fps, frames
  preloaded on GPU, M instances on M threads).
- `media/` (gitignored): Xiph park_joy 1080p50 -> `parkjoy_1080i.y4m` (real TFF
  interlace via tinterlace, 250 frames), `touchdown_1080p.y4m` (300 frames).

## 2026-10-15 — phase 1 (I/P, frame DCT) working
- Bit-exact vs FFmpeg decoder on first run; `-err_detect explode` clean; NVDEC
  zero-copy input (nv12 CUDA frames) also bit-exact.
- Encoder-only, touchdown, 15 Mbps: GPU 1 instance 1300 fps, aggregate ~1700 fps
  (M=4..8). CPU mpeg2video same harness: 1 instance (threads=auto) ~285-300 fps,
  32 instances x threads=1 ~617 fps aggregate.
- End to end NVDEC H.264 -> mpeg2_cuda + AC-3 + mpegts: 756 fps in ONE process
  (NVDEC-bound, same ceiling as the earlier 25-process CPU result).
- Quality @15M CBR (PSNR-Y / SSIM-Y):
  - touchdown: cpu IP 38.52/0.9276, cpu B2 38.83/0.9315, gpu 38.23/0.9270
  - parkjoy 1080i: cpu IP 25.14/0.7278, cpu B2 25.09/0.7264, gpu 24.64/0.6960

## 2026-10-15 — phase 2: interlace tools, speed, VBV
- Field DCT (per-MB dct_type, vertical-SAD decision like ildct_cmp) and field
  motion prediction in frame pictures (k_me_field: 2 ref fields x 15 full-pel +
  half-pel per field). Bit-exact.
- Bugs found by `scripts/stress.sh` (30 configs, all BITEXACT now):
  1. put_motion tested `val == 0` before the modulo wrap; after a field MB the
     PMV is 2x the field vector, so a -128 delta must code as '1'.
  2. libavcodec's mpeg_decode_motion skips the wrap for a zero delta, so an
     out-of-range PMV decodes wrong. Field vertical vectors are limited to
     +-31 half-pel so 2*PMV always stays in range (safe for any decoder).
- Speed: ME SAD via __vsadu4/__vavgu4 + funnel-shift loads, lane-per-candidate
  refine; k_encode_mb rewritten as thread-per-8-pixel-row (warp transposes,
  one __syncthreads). Kernel time ~270 us/frame.
- VBV: re-code a picture (ME reused) when it would underflow; auto non-linear
  q_scale above q 62; DC-only, then repeated-picture fallback; P pictures capped
  so the next I fits. `tools/vbv.py`: 0 underflows on park_joy at 3/4/8/15 Mbps.
- Debug tools: `-dump_mb`, `tools/mbdump.py`, `tools/mbcompare.py` (vs decoder
  `-debug mb_type`), `tools/m2vparse.py` (independent MB-layer parser).

### Scoreboard (touchdown 1080p unless noted, 15 Mbps)
NOTE: earlier harness CPU rows were all-intra (rawvideo frames arrive tagged
pict_type=I and mpegvideo_enc obeys it). Fixed in m2c_bench; numbers below are
the corrected ones.

Encoder-only fps, frames preloaded (`m2c_bench`):

| | 1 stream | whole machine |
|---|---|---|
| mpeg2_cuda, RTX 3090 (I/P, field ME/DCT) | ~2900 | ~4100 (M=2..8) |
| mpeg2video CBR I/P, 7950X | ~480 (threads=auto) | ~1260 (8 x threads=4) |
| mpeg2video CBR B2 | ~300 | ~710 |
| (mpeg2video ABR I/P / B2) | ~520 / ~310 | ~1290 / ~750 |

End to end (`scripts/e2e.sh`): NVDEC H.264 1080p -> mpeg2_cuda 15M + AC-3 ->
MPEG-TS, live (-re): 24 streams hold realtime (25 at 0.991x, 26 at 0.97x),
whole-box CPU ~1-2%, NVDEC dec% ~84-98, SM ~23%. NVDEC is the ceiling.
(CPU-encoder pipeline earlier: 25 streams at ~42% CPU with the fast preset,
6 with the quality preset.)

Quality, PSNR-Y / SSIM-Y at 15M CBR (VBV-clean, tools/vbv.py):

| | touchdown 1080p | park_joy 1080i |
|---|---|---|
| mpeg2video I/P (+ilme+ildct) | 38.52 / 0.9276 | 25.14 / 0.7278 |
| mpeg2video B2 | 38.83 / 0.9315 | 25.09 / 0.7264 |
| mpeg2_cuda | 38.22 / 0.9260 | 24.93 / 0.7181 |

At fixed q the GPU spends ~7% more bits than mpeg2video. First suspects: ME cost
assumes PMV = 0 (adjacent MBs pick wildly different vectors, ~20 bits each), and
no skip threshold.

## 2026-10-16 — phase 3: tests, quality parity+, B pictures, bandwidth
- Tests (`tests/run.sh quick|full`): every case = strict FFmpeg decode
  (`-err_detect explode`) + bit-exact recon + `tools/m2vcheck.py` (independent
  MB-layer syntax/semantics checker: slice coverage / start code emulation,
  escapes, coefficient index, dequant saturation, MV range and the
  zero-delta/out-of-range-PMV hazard, MC bounds) + VBV (`tools/vbv.py`) +
  libmpeg2 cross-decode (`tools/xdec.py`) + compute-sanitizer memcheck cases.
  `FFMPEG_BIN=` runs it against a snapshot binary while developing.
- Codex review (docs/reviews/codex-2026-10-15.txt): OOB half-pel reads in ME, build.sh masking
  make failures, mv_bits missing the modulo wrap — all fixed, response appended.
- Quality work, measured with BD-rate (`scripts/rd.sh` + `tools/bdrate.py`,
  piecewise-linear; operating range q 2-13 touchdown, 10-31 park_joy):
  temporal ME predictors, f_code 4 (adaptive per picture), dead zone
  inter -0.25 / intra 0.25, damped rate control (q no longer oscillates
  3<->28), I/P/B complexity calibrated from the first I picture.
- B pictures (default bf=2): receive_packet API, GPU reorder buffer, open
  GOPs, temporal_reference, sequence_end_code; B ME = scaled predictors,
  fwd/bwd search incl. field vectors, k_me_bidir picks among
  {fwd,bwd,bi} x {frame,field}; B skip rules; per-direction PMVs.
- MEASUREMENT BUG fixed: ffmpeg `-lavfi psnr` with `-frames:v n` can run ahead
  and average extra (repeated last frame, later source frame) pairs, flipping
  results nondeterministically (badly with B pictures). `scripts/psnr.sh`
  decodes to y4m, trims both inputs to n and uses shortest=1. All numbers
  below are re-measured with it.
- Interlaced height bug: progressive_sequence=0 needs mb_height = 2*ceil(h/32)
  (720 lines -> 46 rows). Frame rates within 0.1% snap to MPEG-2 rates.
- Bandwidth/placement: k_load vectorised (23 -> 9 us); coefficient VLC fused
  into k_encode_mb (8 lanes per block, segmented prefix max/sum, shared-memory
  bit assembly) — the 6.3 MB/picture dense level array is gone, k_vlc 48 -> 19
  us; uniform tables (DCT basis, scan, intra matrix) in __constant__
  (k_encode_mb 84 -> 73 us); one-round 32-candidate refine (62 -> 52 us).

### Scoreboard (2026-10-16)
BD-rate vs CPU mpeg2video I/P (negative = fewer bits at equal PSNR):

| | touchdown | park_joy 1080i |
|---|---|---|
| CPU mpeg2video bf=2 | -3.4% | +7.7% |
| GPU bf=0 | -2.6% | +0.7% |
| GPU bf=2 (default) | -5.8% | +3.2% |

GPU kernel time ~372 us/picture at bf=2 (ME is ~70% of it), 267 us at bf=0.
`field_skip` was tried and removed (no measurable effect).

### Test channels (2026-10-16)
`media/channels/lineup.tsv` (900 TESTCAT, 901 TESTSAX, 902 TESTNYA, 903 TSTCRAB,
904 TSTCARA [1 h version], 905 TSTFISH) -> `scripts/make_channels.sh` builds
1-hour 1920x1080 29.97 H.264 + AC-3 TS sources (lanczos upscale, pillarbox).
`scripts/channels.sh start [eqam-ip|null]` loops every channel with
`-stream_loop -1` through NVDEC -> mpeg2_cuda (15M, bf=2) + AC-3 -> SPTS
(program = channel number, service_name = short name), udp port 5000+ch-900.
Stress: 6 channels x 4 copies = 24 live 1080i streams at 1.01x, whole-box CPU
~1.3%, GPU SM ~33%, NVDEC ~86% (NVDEC is the ceiling, not the encoder).
Encoder-only, idle machine (earlier ~3700 bf=0 figure was taken while NVENC
channel builds ran): bf=2 ~2240 fps single / ~2690 aggregate (M=8);
bf=0 ~3100 / ~4170. Kernel time 372 us/picture (bf=2), 267 us (bf=0).

### A/B vs software mpeg2video (2026-10-16)
`tests/ab.sh quick|full`: matched settings (CBR, gop 15, bf 0/2); requires
equal stream properties (size, rate, aspect, pix_fmt, profile), field order
matching the *source*, uniform timestamps, no overshoot (+3%), VBV clean, and
`tools/abcompare.py`: PSNR-Y >= mpeg2video - 0.25 dB, worst frame >= its worst
- 1.5 dB, mean Y/U/V within 0.75, temporal + field alignment. Quick: 6/6 pass,
mpeg2_cuda ahead on PSNR in every case (+0.13 .. +2.70 dB).
Findings it drove: (1) field order now follows the source (interlaced keep
tff/bff; progressive frames coded as progressive_frame with frame DCT and
prediction, `-frame_mode interlaced` to force); mpeg2video codes progressive
input as BFF interlaced. (2) the first I picture is re-coded once when it
lands far below its share (no RC history yet). (3) non-linear q also below 2
(quantiser_scale 1) for content that cannot use the rate.
Prior art survey: docs/prior_art.md. EQAM how-to: docs/eqam_howto.md.

### On air on the dev EQAM (2026-10-16)
Final validation on e14023c: conformance 95/95, A/B 14/14 (A/B re-encodes
mpeg2_cuda at mpeg2video's actual rate when mpeg2video overshoots). A/B found
a real RC bug first: easy content left unspent budget piling up (CBR carry-over
>20 Mbit) that a scene change spent in a burst -> VBV emergency -> 17 dB
pictures (ch904). Fixed by clamping the carry-over to +-VBV/2 and stepping
re-codes proportionally (up to 6) before dropping AC.
`scripts/eqam_channels.py` (re-posts every form field, changes only PLAN,
verifies) configured EQAM-420B slots: 5/6 -> RF48 (900 TESTCAT, 901 TESTSAX),
9/10 -> RF49 (902 TESTNYA, 903 TSTCRAB), 13/14 -> RF50 (904 TSTCARA,
905 TSTFISH); Stream = UDP://10.60.2.208:59xx (sender + port), program and
major = channel number, minor 1, CVCT. Config mode already Manual. Senders:
`PORT_BASE=5900 scripts/channels.sh start 10.60.30.47` (paced UDP at the mux
rate, no -re startup burst). All IP lights green, PIDs 256/257/258 detected,
RF 47-50 Locked; per-carrier average 22-26 Mbps of 38.8.
Fable review (out/fable-review.md): no HIGH; MEDIUM: SD level limits vs VBV,
dts for VFR input, hw-frame stream ordering with a non-NULL hwctx stream,
m2vcheck not checking temporal_reference/GOP — to fix next.

### Channels 906/907 (2026-10-16)
Source: "Seattle In 8K" (youtube f1Jj2jpQhIA), downloaded untranscoded.
906 TESTHD (slot 17, RF 54): 4K60 SDR VP9 original -> NVDEC -> fps 59.94 ->
scale_cuda 1080 -> mpeg2_cuda `-field_weave 1` (new: frame pairs woven into
top/bottom fields on the GPU in k_load -> true 1080i29.97 TFF). Validated:
bit-exact, m2vcheck OK, VBV clean, decoded bottom fields match a CPU
`interlace=scan=tff:lowpass=off` reference better than the same-frame
alternative on moving content (32.3 vs 31.1 dB at t=240 s).
907 TEST264 (slot 18, RF 54): YouTube's H.264 1080p60 MP4 -> NVDEC -> 29.97 ->
scale_cuda -> h264_nvenc High@4.0 CBR 8M, 1 s GOP, AUDs, 10M mux (TV
H.264-over-QAM test). EQAM detects MPEG-2 / AVC video + AC-3, RF 54 Locked.
UDP pacing brought the EQAM's per-stream Max readings down to 12-17 Mbps.
lineup.tsv has a mode column (mezz / raw-mpeg2 / raw-h264);
`ONLY="906 907" scripts/channels.sh start ...` starts a subset.

## Next
- More fps: ME dominates (each B picture runs two directional searches);
  host syncs per picture; overlap independent B pictures on two streams.
- Adaptive B decision (B helps progressive content, costs on fast interlace).
- Put test channels 900-905 on the EQAM (needs the user's go for slot config).

### H.264 test channels: on-screen labels (2026-10-16)
- 907-911 now burn in "NNN-1  NAME" + the A/B description (lineup.tsv column 6)
  via drawtext (system ffmpeg 7.1.5; NVDEC -> scale_cuda -> hwdownload ->
  drawtext -> hwupload_cuda -> NVENC). Box at 96 px (overscan-safe), 720p scaled.
  Font vendored at media/fonts/LiberationSans-Bold.ttf.
- 911 (libx264) now decodes on NVDEC + preset veryfast: ~76% of one core, realtime
  (was 0.90x with CPU decode + preset fast).
- channels.sh stop now honors ONLY (it used to stop every sender).
- Testing gotcha: in zsh, "$F:textfile" applies the :t modifier to $F; use ${F}.

### H.264 TV breakup: transport ruled out on the IP side (2026-10-16)
- TV report: 908 (bf0, 1 ref) unusable, "breaks up every second"; 909/910/911 bad too.
  The simplest stream being worst argues against a decoder-feature problem.
- EQAM: 0 TS sync errors, no oversubscription, input rates nominal (6.4 avg / 8.0 max).
  Host: 0 UDP SndbufErrors, 0 qdisc drops. Replica of 908 (40 s, tstd.py-style
  check): no late AUs (min margin 384 ms), EB peak 5.07 Mbit, IDRs 1.3-2.5 Mbit.
- EQAM QAM module 2 (RF 54-57): 256-QAM Annex B, STD map, output 41 dBmV/carrier.
  With the user's 15 dB pad that is ~+26 dBmV at the TV (typical target ~0 dBmV):
  possible tuner overload; bit errors hurt single-slice H.264 far more than
  MPEG-2 (one slice per MB row, errors stay local).
- Swap to separate RF channel from codec: 908 moved to slot 19 (RF 54, beside the
  clean 906); slot 21 (RF 55) now 912 TESTM2 = MPEG-2 1080i 12M of the 907 source
  (ONLY=912 VBITRATE=12M MUXRATE=13M).
- BUG found+fixed: the overlay chain (hwdownload -> drawtext -> hwupload_cuda,
  system ffmpeg 7.1) died with ENOSYS at the -stream_loop wrap (~10 min); 907-910
  were dead from ~02:18. Labelled modes now decode NVDEC into system memory
  (-hwaccel cuda only) and scale/draw on the CPU.
- TV photos (909: clean down to a row, garbage below; 911/910: whole-frame
  reference corruption) look like bit errors/loss before the decoder, not a
  feature incompatibility. Not concluded; RF level (41 dBmV - 15 dB pad) still
  the top suspect. Next probes if resumed: extra attenuation, TV signal stats,
  H.264 with one slice per MB row.
- Spun down at the user's request: senders 908-912 stopped, EQAM slots 19 and
  21-24 emptied (eqam_channels.py CLEAR list), lineup lines commented out.
  900-907 still on air.
- 908 reused for TEST265 (HEVC acceptance test): NVENC hevc Main@4.1 1080p29.97
  6M CBR, bf0, AUD, mux 8M, labelled; EQAM slot 21 (RF 55, alone). EQAM accepts
  and passes it but lists the video PID (stream_type 0x24) as "Unknown".

## 2026-10-16 — perf pass 2: keep the GPU busy, kernel trims
Measured with 9 live channels sharing the GPU, so only interleaved A/B runs
count (base vs new back to back). nsys timeline gap analysis
(`out/gtrace*.csv`): the GPU sat idle 148 of 428 us per picture waiting on the
host (sync before the next ME 62, second read-back round trip 48, next frame 31).
- Patch 0011, pipelining: ME of picture k is queued before picture k-1 is
  finished; read-backs on a copy stream behind an event (pageable DtoH overlaps
  kernels on the other stream: microbench, 10 us per read while a kernel runs).
  Idle 148 -> 64 us/pic; what remains is the I picture bubble (~70 us, no ME to
  hide behind) and GPU time slices taken by the live channels' processes.
  touchdown bf2 single stream 2555 -> 2946 fps. ME lambda now comes from the
  last q of the picture type (exact in fixed-q mode: pj-q2/q31 byte-identical).
- Patch 0012, coarse ME: one lane per 4-pel column group row, loads shared by
  4 candidates; bit-identical; ~30 -> ~23 us/call.
- Patch 0013, k_encode_mb shared strides 72/74 (s_f rows 9): bank conflicts
  were 8-way (s_f), 4-way (s_s, s_lev), 2-way (s_pred/s_rec); now none;
  bit-identical; ~77.7 -> ~72.1 us.
- No ncu: RmProfilingAdminOnly=1 on this box (needs root).
- Found while validating: ch904-nvdec A/B fails on the unmodified baseline too
  (worst frame 34.91 vs 38.67 dB, -0.32 dB overall, deterministic). Bisected
  with a nix build of patches 0001-0009 (= e14023c): fails identically, so the
  "14/14 on e14023c" above does not reproduce (the harness changed around
  then); field_weave is not the cause. Open quality item, not fixed yet.
  Bisect recipe: copy flake.nix/flake.lock/scripts/ffmpeg_flags + a prefix of
  patches/ to a scratch dir, `nix build path:<dir>#ffmpeg`, FFMPEG_BIN=... .
- Workflow notes: ffmpeg reads stdin, so loops feeding it a heredoc need
  -nostdin; zsh does not word-split $var (use bash scripts for option lists).
- Full validation of the pipelined build: conformance 95/95; A/B 12/14.
  ch904 as above (pre-existing). ch900-nvdec newly fails worst_frame: one B
  picture (display 71, the last before an I) lands 14% over its VBV cap after
  slightly different bit counts upstream and is re-coded q 10 -> 16 (44.55 dB;
  the baseline passed that check by exactly 0.00 dB at 46.16). Cause is the
  pipeline's ME lambda (last q of the type): it differs from the exact one in
  57-78% of P and 9-83% of B pictures, so re-running ME with the exact lambda
  would cost most of the gain. Averages are unchanged (every A/B case within
  +-0.05 dB of the baseline). Kept; root fix belongs in RC: the last B before
  an I is starved by the I reservation (rc_cap). A 0.95 re-code target took
  frame 71 to 45.02 dB but still fails; not applied.
- Patch 0016: re-code steps always coarser (latent: q 2.55 rounded back to 2
  re-coded the same picture until AC was dropped). Bit-identical otherwise.
- Overall, original baseline vs now (single stream, bf 2, 15M, interleaved,
  live channels on the GPU): park_joy 1080i 2016 -> 2510 fps (496 -> 398
  us/pic), touchdown 1080p 2683 -> 3515 fps (373 -> 285 us/pic).


## 2026-10-16 — perf pass 3: SIMD/asm, then latency
Question asked: can SIMD/asm tricks squeeze more? Answer: barely; the ME
kernels are load-latency bound, not ALU bound. Refine's full-pel SAD loop is
~2M warp-instructions per call (~4 us of issue on the 3090) in a ~58 us kernel.
- SASS check (ptxas the embedded PTX for sm_86, cuobjdump -sass): `__vsadu4`
  is already native VABSDIFF4 on Ampere, but with a zero accumulator + IADD3.
  Microbench: PTX `vabsdiff4.u32.u32.u32.add` (fused accumulate) 1.33x the
  SAD op rate; vabsdiffu4 + dp4a ties it. Patch 0017: -2.4 us/pic park_joy, 0
  touchdown.
- Patch 0018: `(uintptr_t)p & ~3` in ldu32/sad_hpel_rows dropped the pointer's
  address space -> generic LD for every reference read in bidir/encode_mb/
  half-pel; pointer arithmetic keeps LDG. -1..2 us/pic.
- Refine ablation (runtime flag bits, scratch build; us per call, median):
  all 58.3, no half-pel 37.4, no out-of-window candidates 43.3, no full-pel
  SAD loop 36.0, no window staging 56.4, all off 15.5. The half-pel step (8
  neighbours x 4 rows from global after the argmin) was the biggest piece.
- Patch 0019: refine window 22 rows x 7 words from (-3,-3) also covers the
  half-pel neighbours of any best vector within +-2 of cv (warp-uniform test,
  else global). -17.0 us/pic park_joy, -14.4 touchdown.
- Patch 0020: field window 12 lines from (-3,-2) covers every half-pel read
  (best is always a window candidate); parity stride 84 (brute-forced: no
  bank conflicts, 88-90/95-97 would be 2-way). -8.2 us/pic park_joy.
- Tried, no effect, reverted: hoisting refine's coarse/cur/window global loads
  above the intra warp sums (the compiler already overlapped them).
- All of 0017-0020 bit-identical (md5, 7 configs). Pass total, interleaved
  medians: park_joy 1080i 365 -> 337 us/pic (2737 -> 2965 fps), touchdown
  258 -> 243 us/pic (3875 -> 4109 fps). (Absolute numbers are higher than pass
  2's because fewer GPU time slices were taken by the live channels today.)
- Kernel time per picture (park_joy bf2) now: refine 84, encode_mb 81,
  field 68, coarse 40, bidir 31, vlc 24. Remaining ideas: refine's 11
  out-of-window predictor candidates (~15 us/call, global latency), bidir's
  global prediction reads, k_vlc's local-memory spills (160 B stack).
- Not used, and why: tensor cores (no SAD op; the IDCT must stay bit-exact with
  simple_idct), packed fp16 fDCT (would change output and needs the full A/B).

## 2026-10-16 — perf pass 4: memory traffic, copies, streams
Per-stage profile first (nsys, bf2, 15M): input 20 us/pic (k_load 9.4, k_down4
11.1 = 2.53 calls/pic), ME 225 (1080i) / 127 (1080p), encode_mb 79/74,
VLC+pack+concat 33/30, host idle 21/23 (mostly the I/P bubble before
code_picture). Read-back copies were 3 per picture, ~63 KB, already overlapped.
Each step bit-identical (md5, 7 configs); deltas are interleaved medians
vs the previous step, park_joy 1080i / touchdown 1080p:
- 0021 cached reference quarter planes (an anchor was decimated up to 5x
  with two B-frames): -7.0 / -6.5 us.
- 0022 k_load writes the source quarter plane (8x4 px per thread, field
  weave reads the top field back): -2.4 / -3.1.
- 0023 k_encode_mb writes the reconstruction's quarter plane (2 shuffles),
  k_down4 deleted: neutral, kept as a simplification.
- 0024 zero-copy output: ffnvcodec lacks cuMemHostAlloc, so it is looked up
  in the same libcuda handle (FFNV_SYM_FUNC); k_concat writes the picture,
  size and mv_need into device-mapped pinned memory, one event wait instead
  of two pageable round trips. Host idle 22 -> 9 us/pic, but SM byte stores
  over PCIe made k_concat 3.4 -> 11 us and, on the main stream, a net loss
  (+5/+10). Fixed by a side stream, 16 B stores per thread, a parallel
  row-size load (thread 0 had 68 dependent global loads) and all 16 gather
  loads issued at once: -3.7 / -0.3.
- 0025 VLC + pack + concat on the output stream after k_encode_mb (the next
  search touches none of their buffers; the next code_picture waits for
  coded_ev): -24.2 / -19.3. The small grids now fill the GPU beside the next
  picture's motion search.
- 0026 uploads + k_load on a load stream, per-slot events (load_ev: loaded;
  use_ev: last queued reader, recorded after each k_encode_mb incl. re-codes):
  -5.7 / -4.7. Checked: field weave, NVDEC frames, system-memory NV12.
- Not done: reading decoder surfaces directly instead of k_load (1080->1088
  padding in every kernel, NV12 chroma in encode_mb, and holding NVDEC
  surfaces through the B delay could starve the live channels' pools);
  hiding the copy got most of its ~10 us anyway.
- Validation: tests/run.sh full 95/95 after 0025 and 0026 (memcheck incl.);
  repeat encodes identical (6x3 configs, 5x bf3).
- Pass total (interleaved medians, pass start vs 0026): park_joy 1080i
  338.0 -> 294.5 us/pic (2959 -> 3396 fps), touchdown 1080p 242.7 -> 207.2
  (4120 -> 4827 fps). Profiled now: wall 300/231, GPU idle 17/15 us/pic
  (was 21/23); per-kernel sums exceed the wall since the tail and k_load
  overlap the searches. The bench's HtoD (~14 us/pic) is its one-off upload
  of the 30 looped frames, not per picture.

## 2026-10-16 — why the live senders died ("Cannot allocate memory")
900-906 exited 16:09-16:34 after ~14 h with `Error submitting a packet to the
muxer: Cannot allocate memory`. Not memory: ffmpeg's UDP output returns
AVERROR(ENOMEM) when its pacing FIFO is full (`udp.c`, udp_write). Cause, in
`scripts/channels.sh`: the UDP pacer ran at exactly the mux rate. After any
stall, `-re` catches up at 1.05x (readrate_catchup default), or instantly for
stalls under 0.3 s, so each stall left about its own length of backlog that a
pacer at 1.00x never drains. The default FIFO is 7*4096 TS packets, 2.7 s at
16 Mb/s; stalls added up over 14 h, and the last ones came from this session's
heavy jobs (each death lines up with a `nix build .#ffmpeg` at 32 jobs or the
full test + A/B suites running together). Reproduced: SIGSTOP a sender for 4 s
-> the same error ~55 s later. Fix: pace 3% above the mux rate, FIFO 20 s;
survives 4 s and 8 s stalls (watched past the catch-up). No leak: the channel
pipeline at full speed over 156 loops of a 10 s cut stayed at 286 MB RSS.
907/908 (system ffmpeg) were stopped at 19:54:47 by a restart of the IDE
service whose cgroup they lived in; the senders now run in a user systemd
scope (`systemd-run --user --scope --unit=m2c-channels`). Separate, smaller:
the mezz channels log `Invalid DTS ... replacing by guess` once per loop wrap
(DTS 2.002 s past PTS at the discontinuity); ffmpeg repairs it, not fatal.

## Side quest: CPU mpeg2video (not part of the journey)
See `sidequest/cpu/README.md`: where software mpeg2video stands on the 7950X,
what limits it (motion estimation, field ME ~40%), and `field_me_thresh`
(+13-60% whole box at equal quality). Its patch lives in `sidequest/cpu/`,
not `patches/`.

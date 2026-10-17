# Side quest: mpeg2_cuda on rented GPUs

`bundle.sh` builds a self-contained bundle (ffmpeg + m2c_bench with their own
glibc, 60-frame clips, a 30 s H.264 mezz clip); `bench.sh` runs on any
container with the NVIDIA driver: md5 correctness vs the 3090, encoder-only
fps, NVDEC decode fps, and end-to-end NVDEC -> mpeg2_cuda fps. Results land in
`results/`. `run_remote.sh` does upload (rsync) + bench + fetch + destroy in
one shot, falling back to the vast ssh proxy when the direct port refuses.

**Pick hosts by CPU, not just GPU.** Per-frame launch work and ffmpeg's
decode pipeline run on the host; the same RTX 5090 did 1038 fps end to end on
a shared EPYC 7K62 and 2741 fps on a whole-host Ryzen 9 7900X. Filter offers
for `cpu_ghz >= 4.5` and `cpu_cores_effective / cpu_cores >= 0.9` (a
single-GPU machine nobody else is renting).

## Consumer GPU sweep (vast.ai, 2026-10-17)

`summarize.py` output, best of each run (fps; channels = end-to-end peak /
29.97). `*` = some parallel e2e runs failed at 24-32 streams (VRAM). Hosts in
the run name; the newish-AMD reruns are the comparable ones (our desktop is a
7950X). Single runs, no repeats.

| GPU | host CPU | W | md5 | enc touchdown I/P | enc park_joy B2 | NVDEC | e2e peak (M) | channels | run |
|---|---|---|---|---|---|---|---|---|---|
| GeForce RTX 5090 | Ryzen 9 7900X | 575.00 | ok | 21704 | 11089 | 2822 | 2741 (16) | 91 | RTX_5090_7900X |
| GeForce RTX 5060 Ti | Ryzen 5 7600 | 180.00 | ok | 5380 | 2653 | 2066 | 1986 (16) * | 66 | RTX_5060_Ti_7600 |
| GeForce RTX 5070 | Ryzen 9 7900X | 250.00 | ok | 7414 | 3602 | 2054 | 1954 (16) * | 65 | RTX_5070_7900X |
| GeForce RTX 4090 | Ryzen 9 7950X | 375.00 | ok | 14320 | 8001 | 1451 | 1042 (1) | 35 | RTX_4090_7950X |
| GeForce RTX 5090 | EPYC 7K62 | 480.00 | ok | 6851 | 5284 | 1178 | 1038 (16) | 35 | RTX_5090 |
| GeForce RTX 4080 | Intel Core i7-8700K | 320.00 | ok | 10960 | 5410 | 937 | 926 (8) | 31 | RTX_4080_bench |
| GeForce RTX 4070 | Ryzen 9 5900X | 200.00 | ok | 6530 | 3265 | 920 | 911 (24) * | 30 | RTX_4070_5900X |
| GeForce RTX 5080 | EPYC 7K62 | 260.00 | ok | 5387 | 5130 | 1070 | 862 (4) | 29 | RTX_5080_bench |
| GeForce RTX 4090 | Ryzen Threadripper PRO 5955WX s | 450.00 | ok | 14551 | 7935 | 853 | 831 (16) | 28 | RTX_4090_bench |
| GeForce RTX 3080 | 12th Gen Intel Core i7-12700KF | 370.00 | ok | 7149 | 3400 | 756 | 756 (8) * | 25 | RTX_3080_12700KF |
| GeForce RTX 3070 | Intel Xeon  E5-2680 v4 | 160.00 | ok | 4208 | 1963 | 660 | 649 (16) * | 22 | RTX_3070 |
| GeForce RTX 5070 | Ryzen Threadripper PRO 3955WX s | 220.00 | ok | 6694 | 3613 | 698 | 642 (8) | 21 | RTX_5070_bench |
| GeForce RTX 3080 Ti | Intel Xeon  E5-2680 v4 | 250.00 | ok | 5393 | 2758 | 636 | 512 (4) | 17 | RTX_3080_Ti_bench |
| GeForce RTX 4070 | Intel Xeon  E5-2690 v4 | 200.00 | ok | 6039 | 3215 | 577 | 467 (1) | 16 | RTX_4070_bench |

- The live-channel limit is the hardware decoder (NVDEC) plus the host CPU,
  not our encoder: even the 5060 Ti encodes ~5400 fps (~180 channels).
- Blackwell decodes H.264 1080p far faster: ~2050 fps on a 5060 Ti/5070 and
  ~2800 on the 5090 (two engines), vs ~900-1450 on Ada and ~650-760 on
  Ampere. So the cheap 5060 Ti (~66 channels) matches the 5070 and beats the
  4090 (~35) for channels.
- Host CPU matters as much as the GPU: the same 5090 did 35 channels on a
  shared EPYC 7K62 and 91 on a Ryzen 9 7900X; the 5070 went 21 -> 65 and the
  4070 16 -> 30 on newish desktop CPUs.
- Not rerun on a newish host: 3070 and 3080 Ti (no such offers), 4080 and
  5080 (the only such hosts refused our ssh key on two rentals each); their
  rows are host-limited floors.
- RTX 3090 (ours, same bundle, live channels running on it): encode 6312 /
  2982 (touchdown I/P / park_joy B2); NVDEC not measured cleanly; ~24 live
  channels in production.

## First run: RTX 5060 Ti 16 GB on a shared Threadripper 2950X (Korea)

Host: Threadripper 2950X (8 CPU share), PCIe 3.0 x8, driver 580.178.04
(CUDA 13.0), power limit 150 W, SM max 3090 MHz.

- **Correctness:** all 4 encodes byte-identical to the RTX 3090.
- **Encoder only** (aggregate fps; RTX 3090 from the same bundle, live
  channels running on it):

| | 5060 Ti, 1 inst | best (M) | RTX 3090, 1 / 4 inst | 5060 Ti / 3090 |
|---|---|---|---|---|
| touchdown I/P | 4784 | 4784 (1) | 6312 / 6414 | 75% |
| touchdown B2 | 3410 | 3559 (2) | 4231 / 4421 | 80% |
| park_joy I/P | 3740 | 3799 (2) | 4749 / 4751 | 80% |
| park_joy B2 | 2397 | 2454 (2) | 2982 / 3088 | 79% |

  One instance already saturates the card; more instances add nothing.

- **NVDEC** (H.264 1080p29.97, ~8 Mb/s): ~490 fps total, flat from 1 to 16
  decodes (one decode engine).
- **End to end** NVDEC -> mpeg2_cuda 15M: 283 fps for 1 stream, peak 452 fps at
  8 (~15 realtime 1080 channels), 427-436 at 16-32. NVDEC-bound, like the
  3090 (24 live channels there).

So: ~75-80% of the 3090's encoder at 150 W instead of 350 W, but the live
channel count is set by NVDEC (~15 per card here vs ~24 on the 3090).
NVDEC on the 3090 was not re-measured cleanly (its decoder is busy with the
live channels).

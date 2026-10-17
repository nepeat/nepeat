# Side quest: mpeg2_cuda on rented GPUs

`bundle.sh` builds a self-contained bundle (ffmpeg + m2c_bench with their own
glibc, 60-frame clips, a 30 s H.264 mezz clip); `bench.sh` runs on any
container with the NVIDIA driver: md5 correctness vs the 3090, encoder-only
fps, NVDEC decode fps, and end-to-end NVDEC -> mpeg2_cuda fps. Results land in
`results/`.

## RTX 5060 Ti 16 GB (vast.ai, 2026-10-17)

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

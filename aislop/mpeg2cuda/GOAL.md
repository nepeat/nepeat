# mpeg2cuda — goal

Build an MPEG-2 video encoder inside FFmpeg (`-c:v mpeg2_cuda`) that runs on
the RTX 3090 and "trades blows" with libavcodec's CPU `mpeg2video` encoder on
a Ryzen 9 7950X, for 1080i29.97 MPEG-2 TS feeding a Blonder Tongue EQAM-400B
(clear QAM to TVs at home).

Scoreboard (both must hold):
- **speed**: encoder-only fps (frames already in memory), single stream and
  aggregate, vs `mpeg2video` on the full CPU;
- **quality**: PSNR/SSIM at the same CBR bitrate (15 Mbps) vs `mpeg2video`.

Hard requirement: bit-exact reconstruction against FFmpeg's decoder
(`-idct simple`), so the encoder never drifts.

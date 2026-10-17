#!/usr/bin/env bash
# Build the portable benchmark bundle (build/vast/bundle) for rented GPUs.
# Run from aislop/mpeg2cuda. The bundle carries its own glibc and loader, so
# any container with the NVIDIA driver mounted runs it (bench.sh). Kernels are
# PTX 8.7 (CUDA 12.8) for compute_80: loads on drivers >= CUDA 12.8, runs on
# Ampere and newer (__reduce_max_sync rules out Turing). Output is
# byte-identical to the main sm_86 / CUDA 13.2 build.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$root"
b=build/vast; mkdir -p "$b"
export NIXPKGS_ALLOW_UNFREE=1
NV=$(nix build --impure --no-link --print-out-paths --inputs-from . nixpkgs#cudaPackages_12_8.cuda_nvcc | head -1)
# nvcc 12.8 can't parse gcc 14's libstdc++; gcc 13 is its host compiler
G13=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#gcc13 | grep -v -- '-man$' | head -1)
nix develop --command bash -c "
  cd $b && mapfile -t flags < <(grep -v '^\s*$' ../../scripts/ffmpeg_flags | grep -v nvccflags)
  ../../ffmpeg/configure --prefix=\$PWD/install \"\${flags[@]}\" --nvcc=$NV/bin/nvcc \
    '--nvccflags=-gencode arch=compute_80,code=compute_80 -O3 -lineinfo -ccbin $G13/bin/c++' >configure.log
  nice -n 19 make -j16 ffmpeg >make.log
  gcc -O2 -o m2c_bench ../../tools/m2c_bench.c -I../../ffmpeg -I. libavformat/libavformat.a \
    libavcodec/libavcodec.a libswresample/libswresample.a libavutil/libavutil.a -lpthread -lm -ldl"
V=$b/bundle; rm -rf "$V"; mkdir -p "$V"/{bin,lib,media}
cp $b/ffmpeg $b/m2c_bench "$V/bin/"
G=$(dirname "$(ldd $b/ffmpeg | awk '/libc.so.6/ { print $3 }')")
cp -L "$G"/{ld-linux-x86-64.so.2,libc.so.6,libm.so.6,libpthread.so.0,libdl.so.2,librt.so.1} "$V/lib/"
chmod u+w "$V"/lib/*
F=build/ffmpeg/ffmpeg
for c in touchdown_1080p parkjoy_1080i; do
  $F -v error -y -i media/$c.y4m -frames:v 60 -f yuv4mpegpipe "$V/media/$c.y4m"
done
$F -v error -y -ss 60 -i media/channels/900_TESTCAT.ts -t 30 -map 0:v:0 -c copy "$V/media/mezz_h264.ts"
cp sidequest/vast/bench.sh "$V/"
echo "bundle: $V ($(du -sh --apparent-size "$V" | cut -f1)); ship with:"
echo "  tar cf - -C $b bundle | zstd -3 -T8 | ssh -p PORT root@HOST 'mkdir -p m2c && cd m2c && zstd -d | tar xf -'"

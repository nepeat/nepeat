#!/usr/bin/env bash
# Build the portable benchmark bundle (build/vast/bundle) for rented GPUs.
# Run from anywhere; FORCE=1 reconfigures. The bundle carries its own glibc
# and loader, so any container with the NVIDIA driver mounted runs it
# (bench.sh). Kernels are PTX 8.7 (CUDA 12.8) for compute_80: loads on drivers
# >= CUDA 12.8, runs on Ampere and newer (__reduce_max_sync rules out Turing).
# Output is byte-identical to the main sm_86 / CUDA 13.2 build; the md5 check
# at the end proves it on the local GPU before anything is shipped.
set -euo pipefail

main() {
  local root b V F NV G13 G c
  root="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$root"
  b=build/vast; V=$b/bundle; F=build/ffmpeg/ffmpeg; mkdir -p "$b"
  export NIXPKGS_ALLOW_UNFREE=1
  if [[ ! -f $b/ffbuild/config.mak || ${FORCE:-0} == 1 ]]; then
    NV=$(nix build --impure --no-link --print-out-paths --inputs-from . nixpkgs#cudaPackages_12_8.cuda_nvcc | head -1)
    # nvcc 12.8 can't parse gcc 14's libstdc++; gcc 13 is its host compiler
    G13=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#gcc13 | grep -v -- '-man$' | head -1)
    nix develop --command bash -c "
      cd $b && mapfile -t flags < <(grep -v '^\s*$' ../../scripts/ffmpeg_flags | grep -v nvccflags)
      ../../ffmpeg/configure --prefix=\$PWD/install \"\${flags[@]}\" --nvcc=$NV/bin/nvcc \
        '--nvccflags=-gencode arch=compute_80,code=compute_80 -O3 -lineinfo -ccbin $G13/bin/c++' >configure.log"
  fi
  nix develop --command bash -c "
    cd $b && nice -n 19 make -j16 ffmpeg >make.log
    gcc -O2 -o m2c_bench ../../tools/m2c_bench.c -I../../ffmpeg -I. libavformat/libavformat.a \
      libavcodec/libavcodec.a libswresample/libswresample.a libavutil/libavutil.a -lpthread -lm -ldl"

  rm -rf "$V"; mkdir -p "$V"/{bin,lib,media}
  cp $b/ffmpeg $b/m2c_bench "$V/bin/"
  G=$(dirname "$(ldd $b/ffmpeg | awk '/libc.so.6/ { print $3 }')")
  cp -L "$G"/{ld-linux-x86-64.so.2,libc.so.6,libm.so.6,libpthread.so.0,libdl.so.2,librt.so.1} "$V/lib/"
  chmod u+w "$V"/lib/*
  # 60 frames each as lossless FFV1 (~40% of the y4m size); the encoder reads
  # them bit-exactly, which the md5 check below confirms
  for c in touchdown_1080p parkjoy_1080i; do
    $F -v error -y -i media/$c.y4m -frames:v 60 -c:v ffv1 -level 3 -slices 24 -threads 16 \
      -f matroska "$V/media/$c.mkv"
  done
  $F -v error -y -ss 60 -i media/channels/900_TESTCAT.ts -t 30 -map 0:v:0 -c copy "$V/media/mezz_h264.ts"
  cp sidequest/vast/bench.sh "$V/"

  echo "md5 self-check on the local GPU:"
  local ref=(4afa05c6b32d 2321f9d24fc4 4142c752f697 a48b911b8a1d) i=0 m bf
  for c in touchdown_1080p parkjoy_1080i; do
    for bf in 0 2; do
      m=$(LD_LIBRARY_PATH=/run/opengl-driver/lib "$V/bin/ffmpeg" -v error -i "$V/media/$c.mkv" \
          -c:v mpeg2_cuda -b:v 15M -g 15 -bf $bf -f mpeg2video - | md5sum | cut -c1-12)
      [[ $m == "${ref[$i]}" ]] || { echo "  $c bf=$bf: MISMATCH $m (want ${ref[$i]})"; exit 1; }
      echo "  $c bf=$bf: match"; i=$((i + 1))
    done
  done
  echo "bundle: $V ($(du -sh --apparent-size "$V" | cut -f1))"
}

main "$@"; exit

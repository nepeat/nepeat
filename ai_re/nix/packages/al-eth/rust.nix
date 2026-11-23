# Rust ports of al_eth pieces (./rust), built and tested on the build host; not
# part of the al_eth module yet (plan: ../kernel-rust/README.md).
#   board_params.rs  the board-parameter registers (al_eth_board_params_get/set)
# Tests: its unit tests, 400k vectors from the C functions themselves
# (./board-params-vectors.c on the HAL code extracted from ./src, the way
# ../uboot/alpine-v2/tests.nix does), and a no_std build with unsafe forbidden.
{ lib, runCommandCC, rustc, gawk }:
runCommandCC "al-eth-rs" {
  nativeBuildInputs = [ rustc gawk ];
  hal = builtins.path { path = ./src; name = "al_eth-src"; };
  shims = builtins.path { path = ../uboot/alpine-v2/eth-test; name = "alpine-v2-eth-test"; };
  rs = builtins.path { path = ./rust; name = "al-eth-rs-src"; };
  gen = builtins.path { path = ./board-params-vectors.c; name = "board-params-vectors.c"; };
} ''
  set -euo pipefail
  mkdir hal
  cp $hal/*.h hal/; chmod u+w hal/*
  cp $shims/al_hal_plat_types.h $shims/al_hal_plat_services.h hal/
  awk '/^\/\* board params register 1 \*\//{p=1} p{print} p&&/^int al_eth_board_params_get/{g=1} g&&/^}/{exit}' \
    $hal/al_hal_eth_main.c > hal/params_extract.c
  grep -q '^int al_eth_board_params_set' hal/params_extract.c
  grep -q '^int al_eth_board_params_get' hal/params_extract.c
  $CC -O1 -Wall -Werror -Wno-unused-function -Ihal -o gen $gen
  ./gen > vectors.txt
  wc -l vectors.txt

  rustc --edition 2021 -D warnings --test $rs/board_params.rs -o tests
  AL_ETH_C_VECTORS=$PWD/vectors.txt ./tests --nocapture | tee log.txt
  grep -q 'same as the C functions: 200100 decodes, 200000 encodes' log.txt

  # as kernel code would use it: core only, no unsafe
  printf '#![no_std]\n#![forbid(unsafe_code)]\npub mod board_params;\n' > lib.rs
  cp $rs/board_params.rs .
  rustc --edition 2021 -D warnings --crate-type rlib --crate-name al_eth_rs -C panic=abort lib.rs
  echo "no_std + forbid(unsafe_code): ok" | tee -a log.txt
  mkdir $out; cp log.txt $out/
''

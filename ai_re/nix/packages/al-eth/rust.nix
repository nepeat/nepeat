# Rust ports of al_eth (./rust), built and tested on the build host; not
# linked into the al_eth module yet (plan: ../kernel-rust/README.md, scope and
# test map: ./rust/hal/README.md).
#   board_params.rs  the board-parameter registers (al_eth_board_params_get/set)
#   hal/             the HAL + driver hardware logic of the DMC-12 / CS8210 paths
# Tests (one derivation, no network, about a minute):
#   1. board_params: unit tests + 400k vectors from the C functions themselves
#      (./board-params-vectors.c on HAL code extracted from ./src)
#   2. hal/regs.rs is what ./difftest/gen_regs.c prints from the vendor headers
#   3. difftest: ./difftest/{mock,glue,scenarios}.c run the UNMODIFIED vendor HAL
#      (./src/al_hal_*.c) over a recording register bank (ASan + UBSan); the
#      Rust HAL replays the trace and must produce every register access, PCI
#      config access, delay, barrier, descriptor and result line identically
#   4. hal unit tests (ring ownership model, board params)
#   5. hal as kernel code would build it: no_std, unsafe forbidden, the
#      kernel's rustc lints, -D warnings
{ lib, runCommandCC, rustc, gawk }:
runCommandCC "al-eth-rs" {
  nativeBuildInputs = [ rustc gawk ];
  hal = builtins.path { path = ./src; name = "al_eth-src"; };
  shims = builtins.path { path = ../uboot/alpine-v2/eth-test; name = "alpine-v2-eth-test"; };
  rs = builtins.path { path = ./rust; name = "al-eth-rs-src"; };
  dt = builtins.path { path = ./difftest; name = "al-eth-difftest"; };
  gen = builtins.path { path = ./board-params-vectors.c; name = "board-params-vectors.c"; };
} ''
  set -euo pipefail
  # --- 1. board params
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

  # --- 2. register map generated from the vendor headers
  mkdir genh
  cp $hal/*.h $hal/al_hal_*.c genh/; chmod u+w genh/*
  cp $shims/al_hal_plat_types.h $shims/al_hal_plat_services.h genh/
  $CC -O0 -w -DCONFIG_AL_ETH_ALPINE_V2 -Igenh -o gen_regs $dt/gen_regs.c \
    genh/al_hal_udma_config.c genh/al_hal_udma_iofic.c genh/al_hal_iofic.c
  ./gen_regs > regs.rs
  diff -u $rs/hal/regs.rs regs.rs
  echo "hal/regs.rs: same as gen_regs.c's output ($(grep -c 'pub const' regs.rs) constants)" | tee -a log.txt

  # --- 3. difftest: vendor C HAL vs Rust HAL
  mkdir dth
  cp $hal/*.h dth/; chmod u+w dth/*
  for f in al_hal_eth_main al_hal_udma_main al_hal_udma_config al_hal_udma_iofic al_hal_iofic; do
    cp $hal/$f.c dth/
  done
  chmod u+w dth/*
  cp $dt/plat/*.h dth/
  $CC -O1 -g -fsanitize=address,undefined -fno-sanitize=bounds -U_FORTIFY_SOURCE -DCONFIG_AL_ETH_ALPINE_V2 \
    -Idth -I$dt -o harness $dt/scenarios.c $dt/glue.c $dt/mock.c \
    dth/al_hal_eth_main.c dth/al_hal_udma_main.c dth/al_hal_udma_config.c dth/al_hal_udma_iofic.c dth/al_hal_iofic.c
  ASAN_OPTIONS=detect_leaks=0 UBSAN_OPTIONS=halt_on_error=1 ./harness > trace.txt
  echo "C trace: $(wc -l < trace.txt) lines, $(grep -c '^S ' trace.txt) scenarios, $(grep -c '^I CALL' trace.txt) calls" | tee -a log.txt
  mkdir dts; cp -r $rs dts/rust; cp -r $dt dts/difftest; chmod -R u+w dts
  rustc --edition 2021 -O -D warnings -o difftest dts/difftest/difftest.rs
  ./difftest trace.txt 2>&1 | tee -a log.txt
  grep -q ' 0 mismatching calls' log.txt
  # --hw mode (what a hardware recording goes through): a scenario stripped to
  # what /proc/al_eth_trace records, reads answered from the trace
  awk '/^S dmc12_filled_a/{p=1; print "S hw"; next} /^S /{p=0} p && (/^I CALL/ || /^(R|W|CR|CW|D) /)' trace.txt > hw.txt
  ./difftest --hw hw.txt 2>&1 | tee hw.log
  grep -q ' 0 mismatching calls' hw.log
  awk '!d && /^W m 00014/ {sub(/ [0-9a-f]+$/, " deadbeef"); d=1} {print}' hw.txt > hw-bad.txt
  if ./difftest --hw hw-bad.txt > /dev/null 2>&1; then echo "difftest --hw missed a changed write" >&2; exit 1; fi
  echo "difftest --hw: replays a recorded control path, catches a changed write" | tee -a log.txt

  # --- 4. hal unit tests
  printf '//! al_eth HAL unit tests\nmod hal;\n' > dts/rust/hal_tests.rs
  rustc --edition 2021 -D warnings --test -o hal_tests dts/rust/hal_tests.rs
  ./hal_tests | tee -a log.txt

  # --- 5. as the kernel builds it (rust_common_flags lints without clippy)
  printf '#![no_std]\n//! al_eth HAL check build\nmod hal;\n' > dts/rust/hal_lib.rs
  rustc --edition 2021 --crate-type rlib --crate-name al_eth_hal -C panic=abort \
    -Dnon_ascii_idents -Dunsafe_op_in_unsafe_fn -Wmissing_docs -Wrust_2018_idioms -Wunreachable_pub \
    -D warnings dts/rust/hal_lib.rs
  echo "hal: no_std, forbid(unsafe_code), kernel lints, -D warnings: ok" | tee -a log.txt

  mkdir $out
  cp log.txt $out/
  grep -E '^(S |I CALL)' trace.txt | awk '/^S /{s=$2} /^I CALL/{c[s]++} END{for (k in c) print c[k], k}' | sort -k2 > $out/scenarios.txt
''

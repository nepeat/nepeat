# qemu-ipd offline tests (x86_64 build host).
#   layout      ipd ioctl structures identical on x86_64 and aarch64 (pahole of
#               every inno_ioctl.h typedef, ipd 0.16.2 = the box's driver source)
#   passthrough qemu-ipd-native under a seccomp-notify fake driver: commands,
#               argument pointers and values reach the "driver" unchanged, other
#               fds/commands do not, device-map atomics fault the guest
#   codegen     QEMU_IPD_STRICT keeps a single-threaded guest's mfence and turns
#               16-byte SSE accesses from single-copy atomic (al16) into 8-byte
#               pairs (TCG ops; x86 host, so no per-access TSO barriers here)
#   aarch64     the box's own binary, run under qemu-aarch64 -cpu cortex-a72 (no
#               LSE2, like the CS8210's A72): plain qemu sends aligned 16-byte
#               SSE accesses of a threaded guest to the ld/st_i128 helpers (an
#               LDXP/STXP read-with-write-back on this CPU); strict mode has
#               none of those, no inline LDXP/STXP, and DMB OSH instead of ISH
{ lib, runCommand, runCommandCC, fetchFromGitHub, pahole, qemu-ipd, qemu-ipd-aarch64, cross, pkgs }:
let
  ipdSrc = fetchFromGitHub {
    owner = "Marvell-switching";
    repo = "mrvl-teralynx";
    rev = "53d8dcf6270719e7bdc42619e182015bb2245ac7";   # 0.16.2, as nepeat/nix kernel.nix
    hash = "sha256-fbBVgWRFxnBuWj9LGsSNr4qmD1xDhnXjGV7yLjC3u08=";
  };
  guestCC = flags: name: src: runCommandCC name { } ''
    $CC -O2 ${flags} -o $out ${src}
  '';
  ipdGuest = guestCC "-pthread" "ipd-guest" ./ipd_guest.c;
  probe = guestCC "-msse2 -pthread" "codegen-probe" ./codegen_probe.c;
  supervisor = guestCC "" "ipd-supervisor" ./supervisor.c;
  # outer CPU for the aarch64 test: qemu-aarch64 as a Cortex-A72
  outerQemu = pkgs.qemu-user.override { hostCpuTargets = [ "aarch64-linux-user" ]; };
in {
  layout = runCommandCC "ipd-ioctl-layout" { nativeBuildInputs = [ pahole cross.stdenv.cc ]; } ''
    h=${ipdSrc}/drivers/ipd/inno_ioctl.h
    types=$(grep -oE '^\} *[A-Za-z_0-9]+_t;' $h | sed -E 's/^\} *//; s/;$//' | sort -u)
    test "$(echo "$types" | wc -l)" -ge 25
    { echo '#include <stdint.h>'; echo '#include <sys/socket.h>'; echo "#include \"$h\""
      for t in $types; do echo "$t v_$t;"; done; } > tu.c
    $CC -g -c -o x86_64.o tu.c
    aarch64-unknown-linux-gnu-cc -g -c -o aarch64.o tu.c
    pahole x86_64.o > x86_64.txt
    pahole aarch64.o > aarch64.txt
    test -s x86_64.txt
    diff -u x86_64.txt aarch64.txt
    mkdir $out; cp tu.c x86_64.txt $out/
    echo "$(echo "$types" | wc -l) types, identical layouts" | tee $out/result
  '';

  passthrough = runCommand "qemu-ipd-passthrough" { } ''
    truncate -s 12288 fake
    export IPD_FAKE=$PWD/fake QEMU_IPD_TEST_PATH=$PWD/fake QEMU_IPD_TRACE=1
    ${supervisor} log -- ${qemu-ipd}/bin/qemu-x86_64 -B 0 ${ipdGuest} > out 2> err || { cat out err log; exit 1; }
    cat out log
    grep -q '^ALL OK' out
    ! grep -E 'LEAK|UNEXPECTED|BAD' log
    grep -q 'QUERY struct ok' log
    grep -q 'INT arg 7' log
    arg=$(awk '/^QUERY_ARG/ {print $2}' out)
    grep -q "IPD cmd 0xc0084902 arg $arg$" log          # guest pointer == host pointer
    test "$(grep -c '^IPD cmd' log)" = 3                # unknown nr never reached the driver
    test "$(grep -c 'guest fault instead' err)" = 6     # the six device-map atomics
    mkdir $out; cp out log err $out/
  '';

  codegen = runCommand "qemu-ipd-codegen" { } ''
    for s in 0 1; do
      QEMU_IPD_STRICT=$s ${qemu-ipd}/bin/qemu-x86_64 -cpu max -d op -D op$s.txt ${probe}
      echo "strict=$s mb=$(grep -c '^ *mb ' op$s.txt)" \
        "atomic16=$(grep -cE 'qemu_(ld|st)2_i128 .*,al16\+leo,' op$s.txt)" \
        "pair16=$(grep -cE 'qemu_(ld|st)2_i128 .*,pair\+al16\+leo,' op$s.txt)"
    done | tee summary
    grep -q 'strict=0 mb=0 atomic16=[1-9][0-9]* pair16=0' summary
    grep -q 'strict=1 mb=[1-9][0-9]* atomic16=0 pair16=[1-9]' summary
    mkdir $out; cp summary $out/
  '';

  aarch64 = runCommand "qemu-ipd-aarch64-codegen" { nativeBuildInputs = [ pkgs.gawk ]; } ''
    outer=${outerQemu}/bin/qemu-aarch64
    for s in 0 1; do
      QEMU_IPD_STRICT=$s $outer -cpu cortex-a72 ${qemu-ipd-aarch64}/bin/qemu-x86_64 -cpu max \
        -d op,out_asm -D log$s.txt ${probe}
      echo "strict=$s helper16=$(grep -cE 'call (ld|st)_i128,' log$s.txt) $(gawk -f ${./asm_words.awk} log$s.txt)"
    done | tee summary
    grep -q 'strict=0 helper16=[1-9]' summary
    grep -q 'strict=1 helper16=0 ldxp=0 stxp=0 dmb_ish=0 dmb_osh=[1-9]' summary
    mkdir $out; cp summary $out/
  '';
}

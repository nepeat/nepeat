#!/usr/bin/env bash
# The smoke program on the mock libifcs (crate sai-teralynx-smoke; nix/sai-teralynx-rs.nix's check):
# the H1 gate's full run (cables by tests/c/mock_wire.c), --recreate, --hold, its failure paths and
# refusals, and the C++ program's form against the C++ program itself on the same library.
#
#   mock-smoke.sh WORK SMOKE LIBDIR FIXTURES REGS [CPP_SMOKE CPP_LIBDIR]
#
# SMOKE: the Rust sai-teralynx-smoke; LIBDIR: libsai.so.1 + libsai-teralynx-shim.so (the package's lib/);
# FIXTURES: crates/sai-teralynx-parity/build-fixtures.sh's output (mock/libifcs.so, cfg/); REGS: the
# backend's tests/ (post-init-regs.txt, diag-regs.txt); CPP_SMOKE / CPP_LIBDIR: the C++ program and the
# C++ library's lib/ (optional: the C++ form's differential and the gate on the C++ library).
# CC: the C compiler (the wire fixture). One line per scenario; exit 1 if one is red.
set -uo pipefail
work=$1 smoke=$2 libdir=$3 fx=$4 regs=$5 cpp_smoke=${6:-} cpp_libdir=${7:-}
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$work"
# absolute: every run changes into its own directory
work=$(cd "$work" && pwd) libdir=$(cd "$libdir" && pwd) fx=$(cd "$fx" && pwd) regs=$(cd "$regs" && pwd)
[ -z "$cpp_libdir" ] || cpp_libdir=$(cd "$cpp_libdir" && pwd)
case $smoke in /*) ;; *) smoke=$PWD/$smoke ;; esac
${CC:-cc} -std=gnu11 -fPIC -shared -Wall -Wextra -Werror -O2 -o "$work/libmock-wire.so" "$here/c/mock_wire.c" -ldl -lpthread
sock=${SAI_RS_TEST_TMPDIR:-/tmp}/smoke-$$.sock
red=0
say() { echo "mock-smoke: $*"; }
bad() { say "RED: $*"; red=1; }
errs() { tail -n 15 "$work/$1/err.txt" 2>/dev/null | sed "s/^/  stderr: /"; }

profile() {   # dir [extra lines]
  local d=$1; shift
  mkdir -p "$d"
  {
    echo "SAI_OPENBCM_BACKEND=openbcm"
    echo "SAI_TERALYNX_NETDEV=create"
    echo "SAI_TERALYNX_LIBIFCS=$fx/mock/libifcs.so"
    echo "SAI_TERALYNX_PORT_MAP=$fx/cfg/ports32.map"
    echo "SAI_INIT_CONFIG_FILE=$fx/cfg/init.yaml"
    echo "SAI_TERALYNX_RUN_DIR=$d/run"
    echo "SAI_OPENBCM_DIAG_SOCKET=$sock"
    echo "SAI_TERALYNX_DIAG_REGS=$regs/diag-regs.txt"
    echo "SAI_TERALYNX_POST_INIT_REGS=$regs/post-init-regs.txt"
    for l in "$@"; do echo "$l"; done
  } > "$d/profile"
}

gate() {   # name lib wire args...: runs the gate in $work/<name>, sets rc
  local name=$1 lib=$2 wire=$3; shift 3
  local d=$work/$name
  rm -f "$sock"
  ( cd "$d" && LD_PRELOAD=$work/libmock-wire.so MOCK_WIRE=$wire timeout 300 "$smoke" "$@" --profile profile \
      --libsai "$lib/libsai.so.1" > out.txt 2> err.txt )
  rc=$?
}
has() { grep -q -- "$2" "$work/$1/out.txt"; }

# --dry, usage, refusals
d=$work/dry; mkdir -p "$d"
"$smoke" --dry > "$d/out.txt"; rc=$?
n=$(grep -c '^ *[0-9]*\. ' "$d/out.txt")
[ $rc -eq 0 ] && [ "$n" -eq 16 ] && say "dry: exit 0, 16 steps" || bad "dry: exit $rc, $n steps"
"$smoke" > /dev/null 2>&1; rc=$?; [ $rc -eq 2 ] && say "no arguments: exit 2" || bad "no arguments: exit $rc"
profile "$work/refuse"
( cd "$work/refuse" && "$smoke" --profile profile > out.txt 2> err.txt ); rc=$?
[ $rc -eq 2 ] && grep -q refusing "$work/refuse/err.txt" && say "without --sonic-is-stopped: refused, exit 2" || bad "without --sonic-is-stopped: exit $rc"
gate refuse "$libdir" "" --sonic-is-stopped --pairs 1-40
[ $rc -eq 2 ] && grep -q 'devport 40' "$work/refuse/err.txt" && say "a pair outside the port map: refused, exit 2" || bad "pair 1-40: exit $rc"

# the full gate: three cabled loops, --hold, --recreate
profile "$work/full"
gate full "$libdir" 1-2,3-17,31-32 --sonic-is-stopped --recreate --hold 2
if [ $rc -eq 0 ] && has full '^SMOKE_OK 16 PASS, 0 SKIP' && has full '^PASS guard got_read32=1' \
   && has full '^PASS regs post_init=4 mismatched=0 failed=0' && has full '^PASS oper-up 1<->2=up' && has full '^PASS recreate'; then
  say "full run (3 loops, --hold 2, --recreate): exit 0, $(grep '^SMOKE_OK' "$work/full/out.txt")"
else
  bad "full run: exit $rc"; grep -E '^(FAIL|SKIP|SMOKE)' "$work/full/out.txt"; errs full
fi

# a loop that stays dark: FAIL oper-up, the teardown still runs, exit 1
profile "$work/dark"
gate dark "$libdir" 1-2,31-32 --sonic-is-stopped --link-timeout 2
if [ $rc -eq 1 ] && has dark '^FAIL oper-up pair 3<->17 not up' && has dark '^PASS admin-down' && has dark '^PASS remove-switch' \
   && has dark '^SMOKE_FAILED 1 FAIL'; then
  say "loop 3<->17 not cabled: FAIL oper-up naming it, teardown done, exit 1"
else
  bad "dark loop: exit $rc"; grep -E '^(FAIL|SKIP|SMOKE)' "$work/dark/out.txt"; errs dark
fi

# no libifcs: create-switch fails, nothing else runs but uninitialize
profile "$work/nolib" "SAI_TERALYNX_LIBIFCS=/nonexistent/libifcs.so"
gate nolib "$libdir" "" --sonic-is-stopped
if [ $rc -eq 1 ] && has nolib '^FAIL create-switch' && has nolib '^SKIP remove-switch no switch' && has nolib '^PASS uninitialize'; then
  say "missing libifcs: FAIL create-switch, the rest skipped, exit 1"
else
  bad "missing libifcs: exit $rc"; grep -E '^(FAIL|SKIP|SMOKE)' "$work/nolib/out.txt"; errs nolib
fi

# the guard off: its step is skipped (no line), everything else passes
profile "$work/noguard" "SAI_TERALYNX_IFCS_GUARD=off"
gate noguard "$libdir" 1-2 --sonic-is-stopped --pairs 1-2
if [ $rc -eq 0 ] && has noguard '^SKIP guard SAI_TERALYNX_IFCS_GUARD=off'; then
  say "guard off: SKIP guard, exit 0"
else
  bad "guard off: exit $rc"; grep -E '^(FAIL|SKIP|SMOKE)' "$work/noguard/out.txt"; errs noguard
fi

# no shim beside the library: the guard is not installed, the guard step says so
mkdir -p "$work/noshim-lib"; cp -L "$libdir/libsai.so.1" "$work/noshim-lib/libsai.so.1"
profile "$work/noshim"
gate noshim "$work/noshim-lib" 1-2 --sonic-is-stopped --pairs 1-2
if [ $rc -eq 1 ] && has noshim '^FAIL guard not active' && has noshim '^SMOKE_FAILED 1 FAIL'; then
  say "no libsai-teralynx-shim.so beside the library: FAIL guard, exit 1"
else
  bad "no shim: exit $rc"; grep -E '^(FAIL|SKIP|SMOKE)' "$work/noshim/out.txt"; errs noshim
fi

if [ -n "$cpp_smoke" ] && [ -n "$cpp_libdir" ]; then
  # the gate on the C++ library: no regs command there
  profile "$work/cppgate"
  gate cppgate "$cpp_libdir" 1-2,3-17,31-32 --sonic-is-stopped --recreate
  if [ $rc -eq 0 ] && has cppgate '^SKIP regs the library has no regs command' && has cppgate '^SMOKE_OK 14 PASS, 2 SKIP'; then
    say "the gate on the C++ library: exit 0, regs and hold skipped"
  else
    bad "gate on the C++ library: exit $rc"; grep -E '^(FAIL|SKIP|SMOKE)' "$work/cppgate/out.txt"; errs cppgate
  fi
  # the C++ program's form: the Rust program against the C++ one, on each library (no cables: oper
  # states are the same at every read)
  for side in rust:$libdir cpp:$cpp_libdir; do
    n=${side%%:*} l=${side#*:}
    for args in "4 1:smoke0 9" "2 1 999" "2 0x41"; do
      d=$work/legacy-$n; profile "$d"
      # shellcheck disable=SC2086
      ( cd "$d" && rm -f "$sock" && LD_LIBRARY_PATH=$l SAI_LIBSAI=$l/libsai.so.1 timeout 120 "$cpp_smoke" profile $args > cpp.txt 2> cpp.err ); c=$?
      # shellcheck disable=SC2086
      ( cd "$d" && rm -f "$sock" && LD_LIBRARY_PATH=$l SAI_LIBSAI=$l/libsai.so.1 timeout 120 "$smoke" profile $args > rust.txt 2> rust.err ); r=$?
      if [ $c -eq $r ] && cmp -s "$d/cpp.txt" "$d/rust.txt"; then
        say "C++ form on the $n library, args '$args': identical to the C++ program ($(wc -l < "$d/rust.txt") lines, exit $r)"
      else
        bad "C++ form on the $n library, args '$args': C++ exit $c, Rust exit $r"; diff "$d/cpp.txt" "$d/rust.txt" | head -20
      fi
    done
    "$cpp_smoke" > /dev/null 2>&1; c=$?; "$smoke" a > /dev/null 2>&1; r=$?
    [ $c -eq 2 ] && [ $r -eq 2 ] && say "C++ form usage: exit 2 both" || bad "usage: C++ $c, Rust $r"
  done
fi

# the C++ program's golden (tests/golden/smoke-cpp.txt, captured from sai_smoke.cpp on the C++ library, never from the
# Rust program): the Rust program on the Rust library gives it (the 3 argument sets: exit code and stdout; usage: exit
# code), a changed golden is reported (negative control), and, where the C++ program is given, the live C++ program on
# the C++ library gives it too (live == golden). SMOKE_CAPTURE=<file> instead writes the golden's body from the C++
# program on the C++ library (CPP_SMOKE and CPP_LIBDIR needed) and ends.
golden=$here/golden/smoke-cpp.txt
smoke_blocks() {   # program libdir dir: the golden's body for it
  local prog=$1 l=$2 d=$3 a
  mkdir -p "$d"
  for a in "4 1:smoke0 9" "2 1 999" "2 0x41"; do
    profile "$d"
    # shellcheck disable=SC2086
    ( cd "$d" && rm -f "$sock" && LD_LIBRARY_PATH=$l SAI_LIBSAI=$l/libsai.so.1 timeout 120 "$prog" profile $a > b.out 2> b.err ); rc=$?
    echo "case $a exit $rc"
    sed 's/^/|/' "$d/b.out"
  done
  "$prog" a > /dev/null 2>&1; echo "case usage exit $?"
}
body() { grep -v '^#' "$1"; }
if [ -n "${SMOKE_CAPTURE:-}" ]; then
  [ -n "$cpp_smoke" ] && [ -n "$cpp_libdir" ] || { echo "SMOKE_CAPTURE needs CPP_SMOKE and CPP_LIBDIR" >&2; exit 2; }
  smoke_blocks "$cpp_smoke" "$cpp_libdir" "$work/capture" > "$SMOKE_CAPTURE"
  exit 0
fi
smoke_blocks "$smoke" "$libdir" "$work/golden-rust" > "$work/golden-rust.txt"
if body "$golden" | cmp -s - "$work/golden-rust.txt"; then
  say "the Rust program on the Rust library equals the golden of the C++ program ($(grep -c '^case' "$work/golden-rust.txt") argument sets incl. usage, $(body "$golden" | wc -l) lines)"
else
  bad "the Rust program differs from the golden of the C++ program"; body "$golden" | diff - "$work/golden-rust.txt" | head -20
fi
# negative control: one changed line of the golden must be a difference
body "$golden" | sed '0,/^|/s/^|/|X/' > "$work/golden-mutated.txt"
if cmp -s "$work/golden-mutated.txt" "$work/golden-rust.txt"; then bad "negative control: a changed golden was NOT reported"; else say "negative control: a changed golden line is reported"; fi
if [ -n "$cpp_smoke" ] && [ -n "$cpp_libdir" ]; then
  smoke_blocks "$cpp_smoke" "$cpp_libdir" "$work/golden-cpp" > "$work/golden-cpp.txt"
  if body "$golden" | cmp -s - "$work/golden-cpp.txt"; then
    say "live == golden: the C++ program on the C++ library equals the golden ($(grep -c '^case' "$work/golden-cpp.txt") argument sets incl. usage)"
  else
    bad "live == golden: the C++ program differs from the golden"; body "$golden" | diff - "$work/golden-cpp.txt" | head -20
  fi
fi
rm -f "$sock"
[ $red -eq 0 ] && say "GREEN" || say "RED"
exit $red

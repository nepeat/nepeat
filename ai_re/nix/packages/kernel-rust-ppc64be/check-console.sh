#!/usr/bin/env bash
# Checks a QEMU console log of tests.nix. Env: CPUS, KUNIT=1 (doctests ran),
# FAN=1 (the as7760-fan scenario ran: C and Rust lines must be identical).
set -euo pipefail
log=$1
fail() { echo "FAIL: $*"; exit 1; }
all=$(tr -d '\r' < "$log")
grep -q '^TEST: done' <<<"$all" || fail "no TEST: done"
# the part qemu-init.sh printed in one piece: step results, then the kernel log
c=$(sed -n '/^=== results ===$/,$p' <<<"$all")
[ -n "$c" ] || fail "no results section"
grep -q "^TEST: cpus $CPUS " <<<"$c" || fail "not $CPUS CPUs"
! grep -E '^TEST: FAIL' <<<"$c" || fail "a TEST step failed"
n=$(grep -cE 'ppc64abi: RESULT pass=[0-9]+ fail=0$' <<<"$c" || true)
[ "$n" = 2 ] || fail "ABI module: $n clean runs of 2"
! grep 'ppc64abi: FAIL' <<<"$c" || fail "ABI module reported failures"
! grep -E 'Oops|BUG:|Unable to handle|Kernel panic|Unknown ADD relocation|REL(24|32|34) .*out of range|out of range of kernel_toc|Expected nop|bad TOC|Unknown symbol|disagrees about version|Bad page|stack-protector|segfault' <<<"$c" || fail "kernel error in the log"
# the only WARNINGs are the ABI module's two deliberate warn_on!()
w=$(grep -E '^WARNING:' <<<"$c" || true)
[ "$(grep -c . <<<"$w")" = 2 ] && ! grep -v 'abi_main.rs:' <<<"$w" || fail "unexpected WARNING: $w"
line=$(grep -m1 -oE 'survived warn_on at abi_main.rs:[0-9]+' <<<"$c" | grep -oE '[0-9]+$')
grep -q "^WARNING: abi_main.rs:$line at " <<<"$c" || fail "warn_on file:line not reported"
for m in $(grep -oE '^TEST: loaded [a-z0-9_]+' <<<"$c" | awk '{print $3}' | sort -u); do
  grep -q "^TEST: unloaded $m$" <<<"$c" || fail "$m did not unload"
done
t=$(grep -m1 -oE '^TEST: tainted [0-9]+' <<<"$c" | awk '{print $3}')
# allowed: O (out-of-tree, 4096), W (the warn_on, 512), N (KUnit, 262144)
[ $(( t & ~(4096 | 512 | 262144) )) = 0 ] || fail "taint $t"
if [ "$KUNIT" = 1 ]; then
  grep -qE '# Totals: pass:[0-9]+ fail:0 ' <<<"$c" || fail "doctests failed"
  grep -qE '^ok [0-9]+ rust_doctests_kernel' <<<"$c" || fail "doctest suite not ok"
fi
if [ "$FAN" = 1 ]; then
  d=$(mktemp -d)
  grep '^FAN c ' <<<"$c" | sed 's/^FAN c //' > "$d/c.txt"
  grep '^FAN rust ' <<<"$c" | sed 's/^FAN rust //' > "$d/rs.txt"
  [ "$(wc -l < "$d/c.txt")" -ge 50 ] || fail "fan scenario too short"
  cat "$d/c.txt" "$d/c.txt" | diff -u - "$d/rs.txt" || fail "Rust as7760_fan differs from the C driver"
  grep -q 'fan scenarios done' <<<"$c" || fail "fan scenario incomplete"
  echo "fan: $(wc -l < "$d/c.txt") observations, Rust == C (twice)"
fi
echo "PASS: $(grep -m1 'ppc64abi: RESULT' <<<"$c" | sed 's/.*RESULT //') per ABI run, $(grep -c '^TEST: loaded' <<<"$c") module loads"

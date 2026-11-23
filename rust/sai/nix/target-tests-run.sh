#!/bin/sh
# Runs the sai-rs test bundle (target-tests.nix) from wherever it was unpacked. POSIX sh:
# the target has no bash guarantee. Uses the bundle's own loader and libc (sysroot/), so
# nothing here depends on the host's library paths. Exit status: 0 when every suite passed.
# Arguments are passed to every Rust test executable (e.g. --test-threads=1).
here=$(cd "$(dirname "$0")" && pwd) || exit 99
ld=$(ls "$here"/sysroot/ld*.so* | head -n 1)
libs="$here/sysroot"
tmp="$here/tmp"
mkdir -p "$tmp"

# Executables the tests start themselves (C harness, C MDIO driver, other prebuilt helpers
# in bin/) go through the same loader, under their own name in $SAI_RS_TEST_BIN.
for helper in "$here"/bin/*; do
    [ -f "$helper" ] || continue
    name=$(basename "$helper")
    # Where the host can start the helper itself (its loader is at the path the build recorded:
    # a NixOS switch built from the same nixpkgs), it is used directly, so a test that sets
    # argv[0] is obeyed. Else a wrapper through the bundle's loader; argv[0] is then the wrapper's.
    "$helper" < /dev/null > /dev/null 2>&1
    case $? in
        126 | 127)
            printf '#!/bin/sh\nexec "%s" --library-path "%s" --argv0 "$0" "%s" "$@"\n' "$ld" "$libs" "$helper" > "$tmp/$name"
            chmod +x "$tmp/$name"
            ;;
        *) ln -sf "$helper" "$tmp/$name" ;;
    esac
done
export SAI_RS_TEST_BIN="$tmp"
export SAI_RS_TEST_TMPDIR="$tmp"
export SAI_FFI_HARNESS="$tmp/harness"
export SAI_FFI_LIBSAI="$here/lib/libsai.so"
if [ -x "$tmp/mdio-driver" ]; then
    export MDIO_SCRIPT_C_DRIVER="$tmp/mdio-driver"
fi
if [ -f "$here/ext-macsec.tbl" ]; then   # a board's MACsec table for the end-to-end comparison
    export SAI_MACSEC_E2E_TABLE="$here/ext-macsec.tbl"
fi
# The tests use the virtual backend only. On a switch the SDK's kernel modules are loaded
# and the virtual backend refuses to start next to them unless this is set; it opens no
# device either way.
export SAI_RS_VIRTUAL_ON_HARDWARE=1
# Tests append one line each about what they compared or skipped (a passing test's own
# output is captured by the test harness and would not show).
export SAI_RS_TEST_SUMMARY="$tmp/summary.txt"
: > "$SAI_RS_TEST_SUMMARY"

# SAI_RS_TEST_ONLY: a shell pattern over the suite names (e.g. 'differential-*'), default all
only=${SAI_RS_TEST_ONLY:-*}
echo "== sai-rs target tests: $(cat "$here/TARGET") on $(uname -srm), $(date -u +%Y-%m-%dT%H:%M:%SZ)"
fail=0 total=0 passed_tests=0 log="$tmp/suite-output.txt"
for t in "$here"/tests/*; do
    name=$(basename "$t")
    case "$name" in $only) ;; *) continue ;; esac
    total=$((total + 1))
    # to a file, not a variable: a suite can print megabytes, and a variable that happens to
    # be exported in the caller's environment would no longer fit an exec
    "$ld" --library-path "$libs" "$t" "$@" > "$log" 2>&1
    rc=$?
    line=$(grep '^test result:' "$log" | tail -n 1)
    n=$(printf '%s\n' "$line" | sed -n 's/.* \([0-9][0-9]*\) passed.*/\1/p')
    passed_tests=$((passed_tests + ${n:-0}))
    if [ $rc -eq 0 ]; then
        echo "ok    $name: $line"
        grep -i 'SKIPPED\|comparisons\|HARNESS_OK' "$log" | sed 's/^/        /'
    else
        fail=$((fail + 1))
        echo "FAIL  $name (exit $rc)"
        sed 's/^/        /' "$log"
    fi
done
# The C harness against the library as shipped (release profile, stripped).
total=$((total + 1))
if "$tmp/harness" "$here/lib-release/libsai.so" > "$log" 2>&1 && grep -q HARNESS_OK "$log"; then
    echo "ok    C harness on the release libsai.so: $(tail -n 1 "$log")"
else
    fail=$((fail + 1))
    echo "FAIL  C harness on the release libsai.so"
    sed 's/^/        /' "$log"
fi
if [ -s "$SAI_RS_TEST_SUMMARY" ]; then
    echo "-- what the suites report about themselves"
    sed 's/^/   /' "$SAI_RS_TEST_SUMMARY"
fi
echo "== $((total - fail)) of $total suites passed, $passed_tests Rust tests passed, $fail failed"
[ $fail -eq 0 ]

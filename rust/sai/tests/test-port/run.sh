#!/usr/bin/env bash
# The UNMODIFIED C++ suite (cpp/test_sai.cpp + its .inc files and headers, the C++ frontend's tests, frozen
# since 2026-11-23 and moved here unchanged by H5-1: a C-ABI client, kept as a fixture) against the Rust
# libsai through the test port (test_port_rust.cpp here, over sai_rs_test_query), with a per-section
# progress table, compared with the golden of the C++ reference run (the C++ frontend and its test port
# were deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0). See README.md.
#
#   run.sh [--strict] [--self-test]
# Environment (all optional except SAI_INC):
#   SAI_INC            dir with sai.h (its ../experimental is added too)              required
#   LIBSAI             a prebuilt libsai.so with the test port (else: cargo build -p sai-ffi --features test-port)
#   SAI_RS_TEST_INC    dir with sai_rs_test.h      default: the repo's crates/sai-test-port/include
#   CXX, CXXFLAGS, CC  default c++, "-O2 -g" (the C++ Makefile's default), cc
#   BUILD_DIR          build + run dir             default: a new mktemp dir (the source tree stays read-only)
#   SUMMARY            the table is written here   default: $BUILD_DIR/summary.txt
#   TEST_TIMEOUT       seconds per test run        default 900
#   TRACE=0            run untraced (as when ptrace is refused): FAIL-line attribution only
#   HARNESS_STRICT=1   like --strict: exit 1 on any failure, crash or non-zero test exit
#   GOLDEN             the C++ reference's per-section results, captured once from the C++ side
#                      (default: golden/test_sai-cpp-reference.txt here; frozen). The Rust run is compared
#                      with it ("golden: N sections equal"); --strict: a difference exits 1.
#   --self-test        negative controls: a copy of the golden with one number changed, and one with a
#                      row dropped, must each be reported as different from the run; exit 1 if not.
# Exit: 0 when the harness built and ran the suite (whatever the suite's result), 1 with --strict
# on any failure, 2 when the harness itself could not build or run.
set -eu

strict=${HARNESS_STRICT:-0}
self_test=0
for a in "$@"; do
    case $a in
    --strict) strict=1 ;;
    --self-test) self_test=1 ;;
    -h | --help) sed -n '2,26p' "$0"; exit 0 ;;
    *) echo "run.sh: unknown argument $a" >&2; exit 2 ;;
    esac
done

die() { echo "run.sh: $*" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
ws=$(cd "$here/../.." && pwd)   # rust/sai
: "${SAI_INC:?run.sh: SAI_INC must name the dir with sai.h}"
# the suite's own sources: cpp/ here
tests=$here/cpp
[ -f "$tests/test_sai.cpp" ] || die "no test_sai.cpp in $tests"
[ -f "$here/test_port_rust.cpp" ] || die "no test_port_rust.cpp in $here"
golden=${GOLDEN:-$here/golden/test_sai-cpp-reference.txt}
[ -f "$golden" ] || die "no golden $golden"
rs_inc=${SAI_RS_TEST_INC:-$ws/crates/sai-test-port/include}
CXX=${CXX:-c++}
CC=${CC:-cc}
CXXFLAGS=${CXXFLAGS:--O2 -g}
BUILD_DIR=${BUILD_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sai-test-port.XXXXXX")}
mkdir -p "$BUILD_DIR"
BUILD_DIR=$(cd "$BUILD_DIR" && pwd)
SUMMARY=${SUMMARY:-$BUILD_DIR/summary.txt}
timeout_s=${TEST_TIMEOUT:-900}

# The C++ Makefile's flags for test_sai.o (sai-teralynx's, as sai-openbcm's were), plus -fno-inline: every test_* function keeps
# its own symbol for section_trace (the checks are the same code, just not inlined).
WARN="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
INC="-I$SAI_INC -I$SAI_INC/../experimental -I$tests"
B=$BUILD_DIR
log() { echo "run.sh: $*" >&2; }

# ---- a. the Rust libsai with the test port
if [ -n "${LIBSAI:-}" ]; then
    [ -f "$LIBSAI" ] || die "LIBSAI=$LIBSAI does not exist"
    libsai=$LIBSAI
else
    log "cargo build -p sai-ffi --features test-port"
    (cd "$ws" && cargo build -p sai-ffi --features test-port) >"$B/cargo.txt" 2>&1 || { tail -40 "$B/cargo.txt" >&2; die "cargo build failed (log: $B/cargo.txt)"; }
    libsai=${CARGO_TARGET_DIR:-$ws/target}/debug/libsai.so
    [ -f "$libsai" ] || die "cargo built no $libsai"
fi
libsai=$(cd "$(dirname "$libsai")" && pwd)/$(basename "$libsai")
# link through $B/lib: libsai.so for -lsai, and the library's SONAME (libsai.so.1) for the loader
libdir=$B/lib
rm -rf "$libdir"
mkdir -p "$libdir"
ln -s "$libsai" "$libdir/libsai.so"
soname=$(objdump -p "$libsai" 2>/dev/null | awk '$1 == "SONAME" { print $2 }')
[ -n "$soname" ] && [ "$soname" != libsai.so ] && ln -s "$libsai" "$libdir/$soname"

# ---- b. test_sai.o (unmodified source), c. the test port, the tracer, the link
log "compiling test_sai.o, test_port_rust.o, section_trace in $B"
port_inc=
[ -f "$rs_inc/sai_rs_test.h" ] && port_inc="-I$rs_inc"
# shellcheck disable=SC2086
$CXX -std=c++17 $WARN $CXXFLAGS -fno-inline $INC -c "$tests/test_sai.cpp" -o "$B/test_sai.o" || die "test_sai.o did not compile"
# shellcheck disable=SC2086
$CXX -std=c++17 $WARN $CXXFLAGS $INC $port_inc -c "$here/test_port_rust.cpp" -o "$B/test_port_rust.o" || die "test_port_rust.o did not compile"
# shellcheck disable=SC2086
$CXX -std=c++17 $CXXFLAGS -no-pie -o "$B/test_sai_rust" "$B/test_sai.o" "$B/test_port_rust.o" \
    -L"$libdir" -lsai -Wl,-rpath,"$libdir" -ldl -lpthread || die "test_sai_rust did not link"
tracer=
# shellcheck disable=SC2086
if $CXX -std=c++17 $WARN -O2 -o "$B/section_trace" "$here/section_trace.cpp" 2>"$B/section_trace.build.txt"; then
    tracer=$B/section_trace
else
    log "section_trace did not build (untraced runs): $(head -3 "$B/section_trace.build.txt")"
fi
awk -f "$here/plan.awk" "$tests/test_sai.cpp" "$tests"/test_*.inc >"$B/plan.tsv" || die "plan.awk failed"

# ---- d. run (traced when possible) and summarise
# run <label> <binary>: $B/run-<label>/{stdout,stderr,events}.txt, sections.tsv, table.txt; sets rc_<label>
run() {
    label=$1 bin=$2 dir=$B/run-$1
    rm -rf "$dir"
    mkdir -p "$dir"
    syms=$dir/syms.txt
    nm -C --defined-only "$bin" | awk '$2 ~ /^[tT]$/ && $3 ~ /^test_[A-Za-z0-9_]+\(\)$/ && NF == 3 { n = $3; sub(/\(\)$/, "", n); print $1, n }' >"$syms"
    a_checks=$(nm -C "$bin" | awk '$3 == "g_checks" { print $1 }')
    a_fails=$(nm -C "$bin" | awk '$3 == "g_fail" { print $1 }')
    traced=0 rc=0
    if [ "${TRACE:-1}" != 0 ] && [ -n "$tracer" ] && [ -s "$syms" ] && [ -n "$a_checks" ] && [ -n "$a_fails" ]; then
        (cd "$dir" && "$tracer" --timeout "$timeout_s" --syms "$syms" --checks "$a_checks" --fails "$a_fails" \
            --events "$dir/events.txt" -- "$bin" >"$dir/stdout.txt" 2>"$dir/stderr.txt") || rc=$?
        if [ "$rc" -eq 3 ] && [ ! -s "$dir/events.txt" ]; then
            log "$label: section_trace cannot trace here ($(grep '^section_trace:' "$dir/stderr.txt" | head -1)); untraced run"
            echo "section_trace could not trace: $(grep '^section_trace:' "$dir/stderr.txt" | head -1)" >"$dir/untraced.txt"
            rm -f "$dir/events.txt"
        else
            traced=1
        fi
    else
        echo "section_trace not used (TRACE=0, no tracer, or no test_* / g_checks / g_fail symbols)" >"$dir/untraced.txt"
    fi
    if [ "$traced" = 0 ]; then
        rc=0
        if command -v timeout >/dev/null 2>&1; then
            (cd "$dir" && timeout "$timeout_s" "$bin" >"$dir/stdout.txt" 2>"$dir/stderr.txt") || rc=$?
        else
            (cd "$dir" && "$bin" >"$dir/stdout.txt" 2>"$dir/stderr.txt") || rc=$?
        fi
    fi
    ev=
    [ "$traced" = 1 ] && ev=$dir/events.txt
    # the comparison columns: the golden's rows (the C++ reference run's sections.tsv, captured)
    grep -v '^#' "$golden" >"$B/golden-rows.tsv"
    refv=$B/golden-rows.tsv
    # the failing checks' sites (the tracer's watchpoint PCs) as file:line
    : >"$dir/sites.tsv"
    if [ "$traced" = 1 ] && grep -q '^fail' "$dir/events.txt" && command -v addr2line >/dev/null 2>&1; then
        awk -F'\t' '$1 == "fail" && !seen[$2]++ { print $2 }' "$dir/events.txt" >"$dir/pcs.txt"
        addr2line -e "$bin" <"$dir/pcs.txt" | sed -e 's/ (discriminator [0-9]*)//' -e 's|.*/||' | paste "$dir/pcs.txt" - >"$dir/sites.tsv" || :
    fi
    awk -f "$here/summary.awk" -v plan="$B/plan.tsv" -v events="$ev" -v err="$dir/stderr.txt" -v out="$dir/stdout.txt" \
        -v ref="$refv" -v tsv="$dir/sections.tsv" -v sites="$dir/sites.tsv" -v failures="$dir/failures.tsv" >"$dir/table.txt"
    [ -f "$dir/untraced.txt" ] && cat "$dir/untraced.txt" >>"$dir/table.txt"
    # where a fatal signal hit, as a source line when the object has debug info
    if [ "$traced" = 1 ] && grep -q '^signal' "$dir/events.txt" && command -v addr2line >/dev/null 2>&1; then
        w=$(awk -F'\t' '$1 == "signal" { print $4; exit }' "$dir/events.txt")
        obj=${w%+0x*} off=${w##*+}
        # the non-PIE test binary: addr2line wants the absolute address; a shared object: the offset
        [ "$obj" = "$bin" ] && off=$(awk -F'\t' '$1 == "signal" { print $3; exit }' "$dir/events.txt")
        [ -f "$obj" ] && echo "crash site: $(addr2line -f -C -i -e "$obj" "$off" 2>/dev/null | paste -sd' ' -)" >>"$dir/table.txt"
    fi
    echo "test exit status: $rc" >>"$dir/table.txt"
    eval "rc_$label=$rc"
}

log "running test_sai against $libsai"
run rust "$B/test_sai_rust"

{
    echo "== test_sai (unmodified) on the Rust libsai: $libsai"
    echo "   build dir $B; stderr: $B/run-rust/stderr.txt"
    cat "$B/run-rust/table.txt"
} >"$SUMMARY"
cat "$SUMMARY"

# ---- e. the golden: the C++ reference's per-section rows (section, checks, fails, state, first FAIL)
# golden_diff <golden> <tsv>: the golden's data rows (no '#' lines) against a run's sections.tsv; prints the
# difference, status 0 = equal. The comparison is whole-line, so a changed number, name, row or state differs.
golden_diff() { grep -v '^#' "$1" | diff -u --label golden --label "$3" - "$2"; }
golden_ok=1
if grep -q '[?]' "$B/run-rust/sections.tsv"; then
    echo "golden: not compared (untraced run: checks per section unknown)"
elif golden_diff "$golden" "$B/run-rust/sections.tsv" rust >"$B/golden-rust.diff"; then
    n=$(grep -vc -e '^#' -e '^TOTAL' "$golden")
    echo "golden: $n sections equal (rust run == $(basename "$golden"), $(awk -F'\t' '$1 == "TOTAL" { print $2 }' "$golden") checks)"
else
    echo "golden: rust run DIFFERENT from $golden:"
    cat "$B/golden-rust.diff"
    golden_ok=0
fi
if [ "$self_test" = 1 ]; then
    # negative controls on temp copies of the golden: a changed number, a dropped row
    st=0
    awk 'BEGIN { FS = OFS = "\t" } !/^#/ && !d && $1 != "TOTAL" { $2 = $2 + 1; d = 1 } { print }' "$golden" >"$B/golden-mut-number.txt"
    awk '!/^#/ && !d && !/^TOTAL/ { d = 1; next } { print }' "$golden" >"$B/golden-mut-row.txt"
    for m in number row; do
        if grep -q '[?]' "$B/run-rust/sections.tsv"; then echo "golden self-test: skipped (untraced run)"; break; fi
        if golden_diff "$B/golden-mut-$m.txt" "$B/run-rust/sections.tsv" rust >/dev/null; then
            echo "golden self-test: FAILED, a golden with a changed $m is not reported as different" >&2; st=1
        else
            echo "golden self-test: ok (a golden with a changed $m is reported as different from the rust run)"
        fi
    done
    [ "$st" = 0 ] || exit 1
fi

if [ "$strict" = 1 ]; then
    if [ "$golden_ok" != 1 ]; then
        echo "run.sh: --strict: the golden comparison differs (see above)" >&2
        exit 1
    fi
    fails=$(awk -F'\t' '$1 == "TOTAL" { print $3 }' "$B/run-rust/sections.tsv")
    state=$(awk -F'\t' '$1 == "TOTAL" { print $4 }' "$B/run-rust/sections.tsv")
    # shellcheck disable=SC2154
    if [ "$rc_rust" != 0 ] || [ "$state" != ok ] || [ "$fails" != 0 ]; then
        echo "run.sh: --strict: failures ($fails), state $state, test exit $rc_rust" >&2
        exit 1
    fi
fi
exit 0

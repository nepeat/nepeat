#!/usr/bin/env bash
# The UNMODIFIED C++ suite (sai-openbcm tests/test_sai.cpp + its .inc files) against the Rust
# libsai through the test port (tests/test_port_rust.cpp over sai_rs_test_query), with a
# per-section progress table. See README.md.
#
#   run.sh [--strict] [--no-cpp-ref]
# Environment (all optional except SAI_INC):
#   SAI_INC            dir with sai.h (its ../experimental is added too)              required
#   LIBSAI             a prebuilt libsai.so with the test port (else: cargo build -p sai-ffi --features test-port)
#   SAI_OPENBCM_TESTS  sai-openbcm's tests dir     default: the repo's ai_re/nix/packages/sai-openbcm/tests
#   SAI_RS_TEST_INC    dir with sai_rs_test.h      default: the repo's crates/sai-test-port/include
#   CXX, CXXFLAGS      default c++, "-O2 -g" (the sai-openbcm Makefile's default)
#   BUILD_DIR          build + run dir             default: a new mktemp dir (the source tree stays read-only)
#   SUMMARY            the table is written here   default: $BUILD_DIR/summary.txt
#   CPP_REF            build | none | <dir>        the C++ reference run for the comparison columns: build it from
#                      SAI_OPENBCM_TESTS/.. (default when that tree has src/), skip it, or take
#                      <dir>/test_port_cpp.o, recording_backend.o, libsai.a from a built sai-openbcm build/ dir
#   TEST_TIMEOUT       seconds per test run        default 900
#   TRACE=0            run untraced (as when ptrace is refused): FAIL-line attribution only
#   HARNESS_STRICT=1   like --strict: exit 1 on any failure, crash or non-zero test exit
# Exit: 0 when the harness built and ran the suite (whatever the suite's result), 1 with --strict
# on any failure, 2 when the harness itself could not build or run.
set -eu

strict=${HARNESS_STRICT:-0}
cpp_ref=${CPP_REF:-}
for a in "$@"; do
    case $a in
    --strict) strict=1 ;;
    --no-cpp-ref) cpp_ref=none ;;
    -h | --help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "run.sh: unknown argument $a" >&2; exit 2 ;;
    esac
done

die() { echo "run.sh: $*" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
ws=$(cd "$here/../.." && pwd)   # rust/sai
: "${SAI_INC:?run.sh: SAI_INC must name the dir with sai.h}"
tests=${SAI_OPENBCM_TESTS:-$ws/../../ai_re/nix/packages/sai-openbcm/tests}
[ -f "$tests/test_sai.cpp" ] || die "no test_sai.cpp in $tests (set SAI_OPENBCM_TESTS)"
[ -f "$tests/test_port_rust.cpp" ] || die "no test_port_rust.cpp in $tests"
tests=$(cd "$tests" && pwd)
rs_inc=${SAI_RS_TEST_INC:-$ws/crates/sai-test-port/include}
CXX=${CXX:-c++}
CXXFLAGS=${CXXFLAGS:--O2 -g}
BUILD_DIR=${BUILD_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sai-test-port.XXXXXX")}
mkdir -p "$BUILD_DIR"
BUILD_DIR=$(cd "$BUILD_DIR" && pwd)
SUMMARY=${SUMMARY:-$BUILD_DIR/summary.txt}
timeout_s=${TEST_TIMEOUT:-900}
if [ -z "$cpp_ref" ]; then
    if [ -f "$tests/../src/sai_api.cpp" ] && [ -f "$tests/../Makefile" ]; then cpp_ref=build; else cpp_ref=none; fi
fi

# The sai-openbcm Makefile's flags for test_sai.o, plus -fno-inline: every test_* function keeps
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
$CXX -std=c++17 $WARN $CXXFLAGS $INC $port_inc -c "$tests/test_port_rust.cpp" -o "$B/test_port_rust.o" || die "test_port_rust.o did not compile"
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

# ---- C++ reference (same test_sai.o, the C++ test port and library)
ref_bin=
if [ "$cpp_ref" = build ]; then
    log "building the C++ reference (CPP_REF=none skips it)"
    rm -rf "$B/cpp-ref"
    mkdir -p "$B/cpp-ref"
    cp -R "$tests/../src" "$tests/../Makefile" "$B/cpp-ref/"
    cp -R "$tests" "$B/cpp-ref/tests"
    chmod -R u+w "$B/cpp-ref"
    if make -C "$B/cpp-ref" -j"$(nproc 2>/dev/null || echo 4)" SAI_INC="$SAI_INC" CXX="$CXX" CXXFLAGS="$CXXFLAGS -I$SAI_INC/../experimental" \
        build/libsai.a build/test_port_cpp.o build/recording_backend.o >"$B/cpp-ref.build.txt" 2>&1; then
        cpp_ref=$B/cpp-ref/build
    else
        log "C++ reference did not build (log: $B/cpp-ref.build.txt); no comparison"
        cpp_ref=none
    fi
fi
if [ "$cpp_ref" != none ]; then
    for f in test_port_cpp.o recording_backend.o libsai.a; do [ -f "$cpp_ref/$f" ] || die "CPP_REF=$cpp_ref has no $f"; done
    # shellcheck disable=SC2086
    $CXX -std=c++17 $CXXFLAGS -no-pie -o "$B/test_sai_cpp" "$B/test_sai.o" "$cpp_ref/test_port_cpp.o" "$cpp_ref/recording_backend.o" \
        "$cpp_ref/libsai.a" -lpthread || die "test_sai_cpp did not link"
    ref_bin=$B/test_sai_cpp
fi

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
    refv=
    [ "$label" = rust ] && [ -n "$ref_bin" ] && [ -f "$B/run-cpp/sections.tsv" ] && refv=$B/run-cpp/sections.tsv
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

if [ -n "$ref_bin" ]; then
    log "running the C++ reference"
    run cpp "$ref_bin"
fi
log "running test_sai against $libsai"
run rust "$B/test_sai_rust"

{
    echo "== test_sai (unmodified) on the Rust libsai: $libsai"
    echo "   build dir $B; stderr: $B/run-rust/stderr.txt"
    cat "$B/run-rust/table.txt"
    if [ -n "$ref_bin" ]; then
        echo
        echo "== C++ reference (same test_sai.o, tests/test_port_cpp.cpp, C++ libsai)"
        cat "$B/run-cpp/table.txt"
    fi
} >"$SUMMARY"
cat "$SUMMARY"

if [ "$strict" = 1 ]; then
    fails=$(awk -F'\t' '$1 == "TOTAL" { print $3 }' "$B/run-rust/sections.tsv")
    state=$(awk -F'\t' '$1 == "TOTAL" { print $4 }' "$B/run-rust/sections.tsv")
    # shellcheck disable=SC2154
    if [ "$rc_rust" != 0 ] || [ "$state" != ok ] || [ "$fails" != 0 ]; then
        echo "run.sh: --strict: failures ($fails), state $state, test exit $rc_rust" >&2
        exit 1
    fi
fi
exit 0

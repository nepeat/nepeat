# test-port: the C++ suite on the Rust libsai

Runs the **unmodified** suite of the C++ frontend (`cpp/test_sai.cpp` + its `.inc` files and headers, frozen since
2026-11-23, moved here from `ai_re/nix/packages/sai-teralynx/frontend/tests` by H5-1 with their bytes unchanged; the
frontend's own copy stays there until the C++ package is deleted) against the Rust
`libsai.so` and prints progress per section. The suite reaches a library only through the SAI C ABI and
`test_port.h`; `test_port_rust.cpp` (here) implements that header over the library's test-only function
`sai_rs_test_query` (crate `sai-test-port`, `sai-ffi` feature `test-port`).

    SAI_INC=<dir with sai.h> ./run.sh [--strict] [--no-cpp-ref]

`run.sh -h` lists the environment (`LIBSAI` = a prebuilt library instead of `cargo build -p sai-ffi --features
test-port`, `SAI_OPENBCM_TESTS` (the frozen frontend's tests: the reference run only), `CXX`, `CXXFLAGS`, `BUILD_DIR`, `SUMMARY`, `CPP_REF`, `TEST_TIMEOUT`,
`HARNESS_STRICT`). It only reads the source tree; everything goes to `BUILD_DIR` (default: a new temp dir),
the suite runs with `BUILD_DIR/run-<rust|cpp>` as its working directory. Exit 0 whatever the suite's result
(progress view); `--strict`: 1 on any failure, crash or non-zero test exit; 2: the harness itself failed.

Build: `test_sai.o` with the C++ Makefile's flags (`-std=c++17`, its `WARN`, `CXXFLAGS`, SAI include
dirs + `experimental`, `-Itests`) **plus `-fno-inline`** so every `test_*` function keeps a symbol; linked
`-no-pie` with `test_port_rust.o` and the library (rpath). The C++ reference (`CPP_REF`, default: built from
`SAI_OPENBCM_TESTS/../src` on its fake backend and `SAI_OPENBCM_TESTS/test_port_cpp.cpp`) links the **same** `test_sai.o` with `test_port_cpp.o` and that
C++ `libsai.a`, so the comparison columns line up section by section.

## The golden

`golden/test_sai-cpp-reference.txt` holds the C++ reference run's per-section results (23 sections, 3014 / 3014),
captured once from the C++ side (header: sha256 of the C++ sources, the command; never regenerate it from the Rust output).
Every run compares the Rust run with it (`golden: N sections equal`, also with `CPP_REF=none`) and, when the C++
reference is built, checks live C++ == golden. `--strict` fails on a difference; `--self-test` runs the negative
controls (a golden with a changed number / a dropped row must differ). `GOLDEN=<file>` overrides the path.

## Sections and counting

A section is each `test_*` function `main()` calls, in run order, and the ones they call (`test_port_quirks`,
`test_ext_phy`, `test_macsec` inside `test_dump_and_teardown`, shown indented); `plan.awk` reads them, with their
line ranges, from the source. Counts are **exclusive** (a nested section's checks are not in its caller's).

- `section_trace.cpp` (x86_64 Linux, ptrace): breakpoints at the entry of every `test_*` symbol (`nm`) and at
  each return address; at each it reads the suite's file-static `g_checks` / `g_fail` from the tracee, and at
  `PTRACE_EVENT_EXIT`. A fatal signal is recorded with the section it hit and the crash site (`addr2line`).
  A hardware write watchpoint (x86 DR0) on `g_fail` gives every failing check's own `file:line`, also where the
  suite has fd 2 redirected (test_macsec.inc captures the library log into a temp file, FAIL lines included).
  It writes `@@tp enter|leave <section>` markers into the test's stderr, so every `FAIL file:line` and
  `test_port_rust:` line is attributed to the section that printed it. Totals are checked against `main()`'s
  own "N checks, M failures" line.
- No ptrace (sandbox, other arch: the tracer exits 3), or `TRACE=0`: the run is untraced; failures are attributed by source
  line range (FAILs from helper functions go to "outside test_* bodies"), checks per section are unknown, and
  the summary says so.

## The test port's stated differences (top of `test_port_rust.cpp`)

- Never aborts: an unanswered request prints `test_port_rust: <request>: <reason>` once and returns the absent
  answer; at exit `test_port_rust: summary <request> <calls>`. The summary table lists them. The C++ port
  aborts in those cases. The suite itself can still crash on absent answers (it indexes results).
- **Profile shim** (remove when the library reads `SAI_OPENBCM_BACKEND`): `sai_api_initialize` is interposed and
  the profile wrapped so that, unless the suite's table has them, `SAI_RS_BACKEND` = `virtual` for
  `SAI_OPENBCM_BACKEND=fake` (else that value unchanged) and `SAI_RS_VIRTUAL_COMPAT` = `cpp` (the C++ fake's port
  numbering, which the suite's hard-coded hw numbers assume). Each mapping is printed once.
  `TP_RUST_PROFILE_SHIM=0` passes the suite's table unchanged.

# The C++-vs-Rust parity harnesses of the Rust SAI frontend (gates G1 / G2), as one check:
#   1. test port: the UNMODIFIED C++ suite test_sai.cpp (+ .inc) of the C++ frontend frozen in
#      ai_re/nix/packages/sai-teralynx/frontend/tests, linked with tests/test-port/test_port_rust.cpp
#      against the Rust libsai.so built with sai-ffi's feature `test-port` (tests/test-port/run.sh);
#      per-section checks / failures in the summary, the frozen C++ on its fake backend as reference.
#   2. driver differential: crates/sai-driver's `differential` test: every
#      driver-scripts/*.ops through the frozen C++ frontend_driver and the Rust driver, every
#      difference covered by crates/sai-driver/tests/differential-allow.txt (live).
#   3. glue-call differential (gate G2): crates/sai-backend-openbcm's `glue_differential` test:
#      the Rust frontend on sai-backend-openbcm over sdk-glue's recording Rust fake against the
#      golden of the C++ frontend on openbcm_backend.cpp (deleted; captured at 50a6aa7: the crate's
#      tests/golden), every difference covered by tests/glue-differential-allow.txt.
#     pkgs.callPackage ./rust/sai/nix/parity-harness.nix { saiSrc = <sonic-sairedis source>; cppFrontend = <sai-teralynx/frontend>; }
# cppFrontend: ai_re/nix/packages/sai-teralynx/frontend (its src/ and tests/ are read). strict: the
# test-port suite must pass completely (default: it reports progress; the differentials are
# always strict: green only when every difference is listed). Native builds only (it runs
# the suites). $out/share/sai-rs-parity: the summaries.
# Source: this workspace (its own lock file) + tests/test-port.
{
  lib,
  rustPlatform,
  stdenv,
  saiSrc,
  cppFrontend,
  # the C SDK glue's header (sdk-glue-sys binds it; not the frozen frontend's copy)
  glueSrc ? ../../../ai_re/nix/packages/sai-openbcm/src,
  strict ? false,
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
  frontend = builtins.path {
    path = cppFrontend;
    name = "sai-cpp-frontend-parity-src";
    filter = path: type:
      if type == "directory" then baseNameOf path != "build"
      else baseNameOf path == "Makefile" || builtins.match ".*\\.(cpp|h|c|inc|awk|ops|map)" (baseNameOf path) != null;
  };
in
rustPlatform.buildRustPackage {
  pname = "sai-rs-parity";
  inherit (manifest.workspace.package) version;
  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions [
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (ws + "/clippy.toml")
      (ws + "/rustfmt.toml")
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "c" || f.hasExt "cpp" || f.hasExt "h" || f.hasExt "mdio" || f.hasExt "txt" || f.hasExt "ops" || f.name == "Cargo.toml") (ws + "/crates"))
      (ws + "/tests/test-port")
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";
  nativeBuildInputs = [ rustPlatform.bindgenHook ];
  # SAI_DRIVER_PREBUILD stays unset: native, the differential compiles the C++ driver itself

  env = {
    SAI_INC = "${saiSrc}/SAI/inc";
    SAI_SYS_ABI_CHECK = "require";
    SAI_SYS_ABI_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
    SDK_GLUE_INC = "${builtins.path { path = glueSrc; name = "sdk-glue-src"; filter = p: _: baseNameOf p == "sdk_glue.h"; }}";
    # the frozen C++ frontend (the variables keep the C++ package's name)
    SAI_OPENBCM_SRC = "${frontend}/src";
    SAI_OPENBCM_TESTS = "${frontend}/tests";
  };

  # the library with the test-only ABI (never the deployed one: see sai-ffi's feature test-port)
  cargoBuildFlags = [ "-p" "sai-ffi" "--features" "sai-ffi/test-port" "-p" "sai-driver" ];
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    mkdir -p $TMPDIR/parity
    set -o pipefail
    so=$(find target -name libsai.so -path '*/release/*' | head -n 1)
    test -n "$so"
    # 1. test_sai through the test port (exit 0 unless the harness broke, or strict and red)
    LIBSAI=$PWD/$so SUMMARY=$TMPDIR/parity/test-port.txt BUILD_DIR=$TMPDIR/test-port CPP_REF=build \
      ${lib.optionalString strict "HARNESS_STRICT=1"} bash tests/test-port/run.sh 2>&1 | tee $TMPDIR/parity/test-port-run.txt
    # The summaries in $out are reproducible (nix build --rebuild): the test binaries are built first
    # (cargo's "Compiling" lines come in no fixed order: they go to the build log, not to $out), the
    # runs are single-threaded (libtest prints results in completion order) and their timings are cut.
    mkdir -p $TMPDIR/parity-build
    steady() { grep -vE '^ *(Compiling|Finished|Fresh|Blocking|Running) ' | sed -E 's/(finished in )[0-9.]+s/\1(time)/'; }
    # 2. the driver differential (green only when every difference is in the allow-list)
    cargo test --offline --profile "$cargoCheckType" -p sai-driver --test differential --no-run 2>&1 \
      | tee $TMPDIR/parity-build/differential.txt
    cargo test --offline --profile "$cargoCheckType" -p sai-driver --test differential -- --nocapture --test-threads=1 2>&1 \
      | steady | tee $TMPDIR/parity/differential.txt
    # 3. the glue-call golden (same rule)
    cargo test --offline --profile "$cargoCheckType" -p sai-backend-openbcm --test glue_differential --no-run 2>&1 \
      | tee $TMPDIR/parity-build/glue-differential.txt
    cargo test --offline --profile "$cargoCheckType" -p sai-backend-openbcm --test glue_differential -- --nocapture --test-threads=1 2>&1 \
      | steady | tee $TMPDIR/parity/glue-differential.txt
    if grep -q SKIPPED $TMPDIR/parity/differential.txt $TMPDIR/parity/glue-differential.txt; then
      echo "a differential was skipped although its inputs are here"; exit 1
    fi
    runHook postCheck
  '';
  # the test-port libsai.so is a test artefact, never a library to deploy: not installed
  postInstall = ''
    rm -f $out/lib/libsai.so
    mkdir -p $out/share/sai-rs-parity
    cp $TMPDIR/parity/* $out/share/sai-rs-parity/
  '';

  meta = {
    description = "C++-vs-Rust parity harnesses of the Rust SAI: test_sai on the Rust libsai through the test port, the op-script driver differential (both on the frozen C++ frontend) and the glue-call golden";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}

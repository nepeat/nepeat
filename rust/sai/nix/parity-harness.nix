# The two C++-vs-Rust parity harnesses of the Rust SAI frontend (gate G1), as one check:
#   1. test port: the UNMODIFIED C++ suite sai-openbcm/tests/test_sai.cpp (+ .inc) linked with
#      tests/test_port_rust.cpp against the Rust libsai.so built with sai-ffi's feature
#      `test-port` (tests/test-port/run.sh); per-section checks / failures in the summary.
#   2. driver differential: crates/sai-driver's `differential` test: every
#      tests/driver-scripts/*.ops through the C++ frontend_driver and the Rust driver, every
#      difference covered by crates/sai-driver/tests/differential-allow.txt.
#   3. glue-call differential (gate G2): crates/sai-backend-openbcm's `glue_differential` test:
#      the same scripts through the C++ frontend on the unmodified openbcm_backend.cpp over a
#      recording stub of sdk_glue.h, and through the Rust frontend on sai-backend-openbcm over
#      sdk-glue's Rust fake; every ob_sdk_* call with its arguments compared, every difference
#      covered by crates/sai-backend-openbcm/tests/glue-differential-allow.txt.
#     pkgs.callPackage ./rust/sai/nix/parity-harness.nix { saiSrc = <sonic-sairedis source>; saiOpenbcm = <sai-openbcm package dir>; }
# saiOpenbcm: ai_re/nix/packages/sai-openbcm (its src/ and tests/ are read). strict: the
# test-port suite must pass completely (default: it reports progress; the differential test is
# always strict: green only when every difference is listed). Native builds only (it runs
# the suites). $out/share/sai-rs-parity: the summaries.
# Source: this workspace (its own lock file) + tests/test-port.
{
  lib,
  rustPlatform,
  stdenv,
  saiSrc,
  saiOpenbcm,
  strict ? false,
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
  openbcm = builtins.path {
    path = saiOpenbcm;
    name = "sai-openbcm-parity-src";
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
    SAI_OPENBCM_SRC = "${openbcm}/src";
    SAI_OPENBCM_TESTS = "${openbcm}/tests";
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
    # 2. the driver differential (green only when every difference is in the allow-list)
    cargo test --offline --profile "$cargoCheckType" -p sai-driver --test differential -- --nocapture 2>&1 \
      | tee $TMPDIR/parity/differential.txt
    # 3. the glue-call differential (same rule); a SKIP here would mean the sources were not found
    cargo test --offline --profile "$cargoCheckType" -p sai-backend-openbcm --test glue_differential -- --nocapture 2>&1 \
      | tee $TMPDIR/parity/glue-differential.txt
    if grep -q SKIPPED $TMPDIR/parity/differential.txt $TMPDIR/parity/glue-differential.txt; then
      echo "a differential was skipped although the C++ sources are here"; exit 1
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
    description = "C++-vs-Rust parity harnesses of the Rust SAI: test_sai on the Rust libsai through the test port, the op-script driver differential and the glue-call differential";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}

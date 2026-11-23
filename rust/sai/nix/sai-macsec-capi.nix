# The Rust MACsec object model (crates/sai-macsec) as a C library for the C++ libsai
# (ai_re/nix/packages/sai-openbcm, argument rustMacsec; make RUST_MACSEC=...):
#   $out/lib/libsai_macsec_capi.a   crates/sai-macsec-capi: the model behind sai_macsec_rs.h, and
#                                   phy-seq's C interface (phy_seq.h) in the same archive (a C++
#                                   link takes one Rust static library: two would each bring std)
#   $out/include/sai_macsec_rs.h, $out/include/phy_seq.h
#     pkgs.callPackage ./rust/sai/nix/sai-macsec-capi.nix { saiSrc = <sonic-sairedis source>; }
# Native builds run fmt, clippy -D warnings and the tests of sai-macsec, sai-macsec-phy and
# sai-macsec-capi. saiOpenbcmSrc (sai-openbcm's src): the differential tests against the C++
# model, and the C++ library with this model inside (driver.cpp on sai_macsec_rust.cpp + the
# library just built) held against the C++ one; tables: board tables for the end-to-end test
# (the first one: SAI_MACSEC_E2E_TABLE). A cross build runs nothing.
# Source: the crates it is made of (and the workspace's manifest and lock file).
{
  lib,
  rustPlatform,
  stdenv,
  clippy,
  rustfmt,
  saiSrc,
  saiOpenbcmSrc ? null,
  tables ? [ ],
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
  own = [ "sai-macsec" "sai-macsec-phy" "sai-macsec-capi" ];
  # every workspace crate the three reach through path dependencies (dev-dependencies included:
  # cargo loads their manifests too); a new dependency of sai-core must be added here
  deps = [ "mdio-report" "mdio-script" "mdio-tool" "phy-seq" "phy-seq-capi" "sai-core" "sai-diag" "sai-extphy" "sai-meta" "sai-sys" "sdk-glue" "sdk-glue-sys" "tap-netdev" ];
  crate = name: lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "h" || f.hasExt "cpp" || f.name == "Cargo.toml") (ws + "/crates/${name}");
  flags = lib.concatMap (c: [ "-p" c ]) own;
  native = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
in
rustPlatform.buildRustPackage {
  pname = "sai-macsec-capi";
  inherit (manifest.workspace.package) version;
  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions ([
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (ws + "/clippy.toml")
      (ws + "/rustfmt.toml")
    ] ++ map crate (own ++ deps));
  };
  cargoLock.lockFile = ws + "/Cargo.lock";

  nativeBuildInputs = [ rustPlatform.bindgenHook ];
  nativeCheckInputs = [ clippy rustfmt ];

  env = {
    SAI_INC = "${saiSrc}/SAI/inc";
    SAI_SYS_ABI_CHECK = "require";
    SAI_SYS_ABI_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
    # the C++ differential drivers are compiled by the tests (native) or not at all (cross)
    SAI_MACSEC_PREBUILD_DRIVER = "0";
  } // lib.optionalAttrs (saiOpenbcmSrc != null) { SAI_OPENBCM_SRC = "${saiOpenbcmSrc}"; }
    // lib.optionalAttrs (tables != [ ]) { SAI_MACSEC_E2E_TABLE = toString (builtins.head tables); };

  # as package.nix: the big-endian ppc64 ELFv2 triple the way clang spells it
  preBuild = lib.optionalString (lib.hasSuffix "gnuabielfv2" stdenv.hostPlatform.config) ''
    export BINDGEN_EXTRA_CLANG_ARGS="${"$"}{BINDGEN_EXTRA_CLANG_ARGS/--target=${stdenv.hostPlatform.config}/--target=${lib.removeSuffix "abielfv2" stdenv.hostPlatform.config} -mabi=elfv2}"
  '';

  cargoBuildFlags = [ "-p" "sai-macsec-capi" ];
  cargoTestFlags = flags;
  doCheck = native;
  preCheck = ''
    cargo fmt ${lib.concatStringsSep " " flags} --check
    cargo clippy --offline --no-deps ${lib.concatStringsSep " " flags} --all-targets --profile "$cargoCheckType" -- -D warnings
  '' + lib.optionalString (saiOpenbcmSrc != null) ''
    # the C++ library with this model inside, as a test driver (driver.cpp's "chip" variant)
    a=$(find target -name libsai_macsec_capi.a -path '*release*' | head -n 1)
    d=$TMPDIR/rs-driver; mkdir -p $d
    for u in sai_api sai_switch sai_port sai_l2 sai_hostif sai_l3 sai_lag sai_temp sai_caps sai_diag sai_acl sai_qos \
             sai_mirror sai_hash sai_extphy sai_extmacsec sai_macsec_rust fake_backend; do
      $CXX -std=c++17 -O1 -w -c -DOB_RUST_MACSEC -Icrates/sai-macsec-capi/include -I$SAI_INC -I$SAI_INC/../experimental \
        -I${saiOpenbcmSrc} ${saiOpenbcmSrc}/$u.cpp -o $d/$u.o &
    done
    $CC -std=gnu11 -O1 -w -c -I${saiOpenbcmSrc} ${saiOpenbcmSrc}/temp_files.c -o $d/temp_files.o
    wait
    $CXX -std=c++17 -O1 -Wall -DSAI_MACSEC_DRIVER_CHIP -I$SAI_INC -I$SAI_INC/../experimental -I${saiOpenbcmSrc} \
      crates/sai-macsec/tests/cpp/driver.cpp $d/*.o "$a" -ldl -lpthread -o $d/sai-macsec-rs-chip-driver
    export SAI_MACSEC_E2E_RUST_DRIVER=$d/sai-macsec-rs-chip-driver
  '';

  postInstall = ''
    install -Dm644 crates/sai-macsec-capi/include/sai_macsec_rs.h $out/include/sai_macsec_rs.h
    install -Dm644 crates/phy-seq-capi/include/phy_seq.h $out/include/phy_seq.h
    test -s $out/lib/libsai_macsec_capi.a
  '';

  meta = {
    description = "The Rust MACsec object model (and phy-seq) as a static C library for sai-openbcm";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}

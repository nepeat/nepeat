# sai-openbcm: an open-source libsai.so (OCP SAI 1.19) on the Broadcom
# OpenBCM SDK, with an in-memory "fake" backend for tests. See README.md.
#   callPackage ./sai-openbcm { sonic = ...; }                    fake backend only (any platform)
#   callPackage ./sai-openbcm { sonic = ...; libsdk = ...; }      + openbcm backend (ppc64be)
#   ... { rustPhySeq = <rust/sai/nix/phy-seq.nix for the same pkgs>; }   the external PHY / MACsec table
#       parsers and the sequence executor from the Rust crate phy-seq (Makefile, RUST_PHY_SEQ); default: C++
#   ... { rustMacsec = <rust/sai/nix/sai-macsec-capi.nix for the same pkgs>; }   the MACsec object model from the
#       Rust crate sai-macsec (Makefile, RUST_MACSEC: src/sai_macsec_rust.cpp for src/sai_macsec.cpp); default: C++.
#       Independent of rustPhySeq; with both, the one Rust library linked is rustMacsec's (it carries phy_seq.h's
#       functions too)
# Our code is Apache-2.0; with libsdk the result links the Broadcom SDK
# (Broadcom Switch Software License): not freely redistributable as a binary.
{ lib, stdenv, sonic, libsdk ? null, rustPhySeq ? null, rustMacsec ? null
, doCheck ? stdenv.buildPlatform.canExecute stdenv.hostPlatform && libsdk == null }:
let
  sai = sonic.sources.sairedis;   # SAI headers: the SAI submodule sairedis is pinned to
in stdenv.mkDerivation {
  pname = if libsdk == null then "sai-openbcm-fake" else "sai-openbcm";
  version = "0.1.0";
  src = builtins.path {
    path = ./.;
    name = "sai-openbcm-src";
    # Only what the library build reads: editing docs or the VM test does not rebuild it.
    filter = p: t: !(lib.hasSuffix ".md" p) && !(lib.hasSuffix ".py" p)
      && !(builtins.elem (baseNameOf p) [ "default.nix" "test-syncd.nix" "test-mdio-units.nix" "build" ]);
  };

  enableParallelBuilding = true;
  makeFlags = [
    "SAI_INC=${sai}/SAI/inc"
    "PREFIX=${placeholder "out"}"
    "CXX=${stdenv.cc.targetPrefix}c++"
    "CC=${stdenv.cc.targetPrefix}cc"
    "AR=${stdenv.cc.targetPrefix}ar"
  ] ++ lib.optional (libsdk != null) "OPENBCM=${libsdk}"
    ++ lib.optional (rustPhySeq != null) "RUST_PHY_SEQ=${rustPhySeq}"
    ++ lib.optional (rustMacsec != null) "RUST_MACSEC=${rustMacsec}";
  env.CXXFLAGS = "-O2 -g -I${sai}/SAI/experimental";
  env.CFLAGS = "-O2 -g";
  buildFlags = [ "build/libsai.so.1.0.0" "build/openbcm-diag" "build/openbcm-mdio-script" "build/openbcm-ext-phy" "build/openbcm-ext-macsec" ] ++ lib.optional doCheck "build/test_sai"
    ++ lib.optional (libsdk != null) "build/openbcm-asic-temp";

  inherit doCheck;
  checkTarget = "check";

  postInstall = ''
    mkdir -p $out/include/sai-openbcm $out/share/sai-openbcm
    cp src/backend.h src/sdk_glue.h $out/include/sai-openbcm/   # backend.h includes sdk_glue.h
    echo ${if libsdk == null then "fake" else "fake openbcm"} > $out/share/sai-openbcm/backends
  '' + lib.optionalString (rustPhySeq != null) ''
    echo rust > $out/share/sai-openbcm/phy-seq-engine
  '' + lib.optionalString (rustMacsec != null) ''
    echo rust > $out/share/sai-openbcm/macsec-model
  '';

  passthru = { inherit libsdk rustPhySeq rustMacsec; saiHeaders = "${sai}/SAI/inc"; };

  meta = {
    description = "SAI 1.19 implementation on the Broadcom OpenBCM SDK (thin SAI) with a fake backend";
    license = if libsdk == null then lib.licenses.asl20
      else [ lib.licenses.asl20 lib.licenses.unfreeRedistributable ];
    platforms = lib.platforms.linux;
  };
}

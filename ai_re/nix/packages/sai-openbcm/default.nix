# sai-openbcm: an open-source libsai.so (OCP SAI 1.19) on the Broadcom
# OpenBCM SDK, with an in-memory "fake" backend for tests. See README.md.
#   callPackage ./sai-openbcm { sonic = ...; }                    fake backend only (any platform)
#   callPackage ./sai-openbcm { sonic = ...; libsdk = ...; }      + openbcm backend (ppc64be)
# Our code is Apache-2.0; with libsdk the result links the Broadcom SDK
# (Broadcom Switch Software License): not freely redistributable as a binary.
{ lib, stdenv, sonic, libsdk ? null
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
      && !(builtins.elem (baseNameOf p) [ "default.nix" "test-syncd.nix" "build" ]);
  };

  enableParallelBuilding = true;
  makeFlags = [
    "SAI_INC=${sai}/SAI/inc"
    "PREFIX=${placeholder "out"}"
    "CXX=${stdenv.cc.targetPrefix}c++"
    "CC=${stdenv.cc.targetPrefix}cc"
    "AR=${stdenv.cc.targetPrefix}ar"
  ] ++ lib.optional (libsdk != null) "OPENBCM=${libsdk}";
  env.CXXFLAGS = "-O2 -g -I${sai}/SAI/experimental";
  env.CFLAGS = "-O2 -g";
  buildFlags = [ "build/libsai.so.1.0.0" "build/openbcm-diag" "build/openbcm-mdio-script" ] ++ lib.optional doCheck "build/test_sai"
    ++ lib.optional (libsdk != null) "build/openbcm-asic-temp";

  inherit doCheck;
  checkTarget = "check";

  postInstall = ''
    mkdir -p $out/include/sai-openbcm $out/share/sai-openbcm
    cp src/backend.h $out/include/sai-openbcm/
    echo ${if libsdk == null then "fake" else "fake openbcm"} > $out/share/sai-openbcm/backends
  '';

  passthru = { inherit libsdk; saiHeaders = "${sai}/SAI/inc"; };

  meta = {
    description = "SAI 1.19 implementation on the Broadcom OpenBCM SDK (thin SAI) with a fake backend";
    license = if libsdk == null then lib.licenses.asl20
      else [ lib.licenses.asl20 lib.licenses.unfreeRedistributable ];
    platforms = lib.platforms.linux;
  };
}

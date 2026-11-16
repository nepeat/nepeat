# sai-teralynx: libsai.so (OCP SAI, sairedis' pin) for the Teralynx 7 of the
# Celestica CS8210-32X. The SAI frontend is sai-openbcm's, unmodified; the
# backend (src/) drives the vendor IFCS 0.12.41 SDK, which is dlopen'ed at
# run time from the switch (SAI_TERALYNX_LIBIFCS): nothing vendor is linked
# or in the store. See README.md.
#   callPackage ./sai-teralynx { sonic = ...; }
{ lib, stdenv, sonic, doCheck ? stdenv.buildPlatform.canExecute stdenv.hostPlatform }:
let
  sai = sonic.sources.sairedis;   # SAI headers: the SAI submodule sairedis is pinned to
  frontend = builtins.path { path = ../sai-openbcm/src; name = "sai-openbcm-frontend-src"; };
  frontendTests = builtins.path { path = ../sai-openbcm/tests; name = "sai-openbcm-frontend-tests"; };
in stdenv.mkDerivation {
  pname = "sai-teralynx";
  version = "0.1.0";
  src = builtins.path {
    path = ./.;
    name = "sai-teralynx-src";
    # Only what the library build reads: docs, the board configs and nix files do not rebuild it.
    filter = p: t: !(lib.hasSuffix ".md" p)
      && !(builtins.elem (baseNameOf p) [ "default.nix" "build" "configs" ]);
  };

  enableParallelBuilding = true;
  makeFlags = [
    "SAI_INC=${sai}/SAI/inc"
    "FRONTEND=${frontend}"
    "PREFIX=${placeholder "out"}"
    "CXX=${stdenv.cc.targetPrefix}c++"
    "CC=${stdenv.cc.targetPrefix}cc"
    "AR=${stdenv.cc.targetPrefix}ar"
  ] ++ lib.optional doCheck "FRONTEND_TESTS=${frontendTests}";
  env.CXXFLAGS = "-O2 -g -I${sai}/SAI/experimental";
  env.CFLAGS = "-O2 -g";
  buildFlags = [ "build/libsai.so.1.0.0" "build/libsai-teralynx-shim.so" "build/openbcm-diag" "build/sai-teralynx-smoke" ] ++ lib.optional doCheck "tests";

  inherit doCheck;
  checkTarget = "check";

  passthru = { saiHeaders = "${sai}/SAI/inc"; inherit frontend; };

  meta = {
    description = "SAI implementation for the Innovium/Marvell Teralynx 7 on the vendor IFCS SDK (loaded at run time)";
    license = lib.licenses.asl20;   # links nothing vendor; libifcs.so is dlopen'ed on the switch
    platforms = lib.platforms.linux;
  };
}

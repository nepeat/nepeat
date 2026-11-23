# sai-teralynx: libsai.so (OCP SAI, sairedis' pin) for the Teralynx 7 of the
# Celestica CS8210-32X. The SAI frontend is the C++ one sai-openbcm had, frozen
# here in frontend/ (README.md "Frozen frontend"); the backend (src/) drives the vendor IFCS 0.12.41 SDK, which is dlopen'ed at
# run time from the switch (SAI_TERALYNX_LIBIFCS): nothing vendor is linked.
# Its IFCS declarations are generated at build time from a public ctypes file
# (ifcsCtypes below; README.md "IFCS declarations"). See README.md.
#   callPackage ./sai-teralynx { sonic = ...; }
{ lib, stdenv, buildPackages, fetchurl, sonic, doCheck ? stdenv.buildPlatform.canExecute stdenv.hostPlatform }:
let
  # Build-time vendor input (store only, never git): pyctypes/ifcs_ctypes.py
  # of Innovium's public ishell 2.0.1 deb (Innovium/SONiC debian/202111/T2.0.1,
  # amd64; the file is arch-neutral python). Its declarations of everything
  # src/ifcs_api*.h.in uses equal the box's 0.12.41 ones except
  # src/ifcs_overrides.txt and one @prefix struct (README.md).
  ishellDeb = fetchurl {
    url = "https://raw.githubusercontent.com/Innovium/SONiC/240ee432f6d6b3d36ca8c484d3f7051714279545/debian/202111/T2.0.1/ishell.deb";   # 2025-11-25
    hash = "sha256-iUync4A9AUm68UcyoTYDdAkgvOXwFbNDfj28R2H39ek=";
  };
  ifcsCtypes = buildPackages.runCommand "ifcs-ctypes-ishell-2.0.1.py" { nativeBuildInputs = [ buildPackages.dpkg ]; } ''
    dpkg-deb -x ${ishellDeb} x
    install -Dm644 x/innovium/pyctypes/ifcs_ctypes.py $out
    echo "5a4547fb452a2b4fe85c449ed7089033f81d133f07646168ce01eac2a2843069  $out" | sha256sum -c
  '';
  sai = sonic.sources.sairedis;   # SAI headers: the SAI submodule sairedis is pinned to
  # The frozen C++ frontend (frontend/src) and its tests (frontend/tests), each its own store
  # path: the store names are the ones they had as ../sai-openbcm/{src,tests}.
  frontend = builtins.path { path = ./frontend/src; name = "sai-openbcm-frontend-src"; };
  frontendTests = builtins.path { path = ./frontend/tests; name = "sai-openbcm-frontend-tests"; };
in stdenv.mkDerivation {
  pname = "sai-teralynx";
  version = "0.1.0";
  src = builtins.path {
    path = ./.;
    name = "sai-teralynx-src";
    # Only what the library build reads: docs, the board configs and nix files do not rebuild it.
    filter = p: t: !(lib.hasSuffix ".md" p)
      && !(builtins.elem (baseNameOf p) [ "default.nix" "build" "configs" "oracle" "frontend" "__pycache__" ]);
  };

  enableParallelBuilding = true;
  nativeBuildInputs = [ buildPackages.python3 ];   # tools/gen_ifcs_api.py
  makeFlags = [
    "SAI_INC=${sai}/SAI/inc"
    "FRONTEND=${frontend}"
    "IFCS_CTYPES=${ifcsCtypes}"
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

  passthru = { saiHeaders = "${sai}/SAI/inc"; inherit frontend ifcsCtypes ishellDeb; };

  meta = {
    description = "SAI implementation for the Innovium/Marvell Teralynx 7 on the vendor IFCS SDK (loaded at run time)";
    license = lib.licenses.asl20;   # links nothing vendor; libifcs.so is dlopen'ed on the switch
    platforms = lib.platforms.linux;
  };
}

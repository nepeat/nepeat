# The mock libifcs (tests/teralynx-mock/*.c, mock.h): the C stand-in for the vendor libifcs.so that
# the Teralynx tests run on (it records every IFCS call; test_ifcs, the parity harness, the smoke and
# syncd tests dlopen it as SAI_TERALYNX_LIBIFCS). ONE derivation for every user (the libsai test run
# and its target bundle, the parity fixtures, the syncd VM test); moved unchanged from
# ai_re/nix/packages/sai-teralynx/tests/mock (H5-1), where the C++ package keeps its copy until it goes.
# Compiled as that package's Makefile does (same flags), against the generated IFCS declarations.
#   pkgs.callPackage ./teralynx-mock.nix { ifcsHeaders = <./ifcs-headers.nix>; }  ->  $out/mock/libifcs.so
{
  lib,
  stdenv,
  ifcsHeaders,
}:
stdenv.mkDerivation {
  pname = "sai-teralynx-mock-libifcs";
  version = "1";
  src = lib.fileset.toSource {
    root = ../tests/teralynx-mock;
    fileset = ../tests/teralynx-mock;
  };
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    warn="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
    mkdir -p $out/mock
    $CC -std=gnu11 -fPIC -shared $warn -Werror=incompatible-pointer-types -O2 -g -I${ifcsHeaders}/include -I. \
      -Wl,--no-undefined -o $out/mock/libifcs.so *.c -lpthread
    runHook postBuild
  '';
  dontInstall = true;
  dontFixup = true;
  meta.description = "the C mock libifcs of the Teralynx tests";
}

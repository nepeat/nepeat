# qemu-ipd: qemu-user for x86_64 guests, patched to run Marvell's amd64 Teralynx
# SAI on the CS8210's arm64 CPU against the real ipd driver. Why and how:
# devices/cs8210-32x/sai-qemu.md. Patches on nixpkgs' qemu (11.1.2 at this pin):
#   0001 linux-user: /dev/ipd* ioctls passed to the host unchanged (needs
#        guest_base 0), device mmaps recorded, guest atomics on them fault
#   0002 tcg, QEMU_IPD_STRICT=1: barriers also single-threaded, DMB OSH on
#        aarch64, 16-byte SSE accesses as 8-byte pairs (no LDXP/STXP)
# Entry (x86_64-linux build host; the box binary is cross-built):
#   import ./qemu-ipd { pkgs = import <nixpkgs> { system = "x86_64-linux"; }; }
# ifcsHeaders: optional, `{ pkgs, saiSrc }: <the IFCS declarations of that package set>` (rust/sai/nix/ifcs-headers.nix, which
# path:./nix cannot reach: the root git flake / tests/nixtargets.py / ifcs-oracle/run-oracle.sh pass it); without it no oracle-ours.
{ pkgs, ifcsHeaders ? null }:
assert pkgs.stdenv.hostPlatform.system == "x86_64-linux";
let
  inherit (pkgs) lib;
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  qemuFor = p: (p.qemu-user.override { hostCpuTargets = [ "x86_64-linux-user" ]; }).overrideAttrs (o: {
    pname = "qemu-ipd";
    patches = (o.patches or [ ]) ++ [
      ./patches/0001-linux-user-ipd-passthrough.patch
      ./patches/0002-tcg-ipd-strict-device-memory.patch
    ];
    # a failed compile here once still produced an (empty) output
    postInstall = (o.postInstall or "") + "test -x $out/bin/qemu-x86_64";
  });

  # amd64 side (built natively here, copied to the box as plain store paths)
  isai = pkgs.callPackage ../isai-amd64 { libortools = ortools-stub; };
  ortools-stub = pkgs.callPackage ./guest/ortools-stub.nix { isaiUnpacked = isai.unpacked; };
  libsai = isai.libsai;

  devmem = pkgs.stdenv.mkDerivation {
    pname = "ipd-devmem";
    version = "1";
    src = lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [ ./guest/devmem.c ./tests/devmem_test.c ];
    };
    # no builtins / vectorising: the copy loops must stay loops of single accesses
    buildPhase = ''
      $CC -O2 -fno-builtin -fno-tree-vectorize -fno-tree-loop-distribute-patterns \
        -Wall -shared -fPIC -o libipd-devmem.so guest/devmem.c -ldl -lpthread
      $CC -O2 -o devmem_test tests/devmem_test.c -ldl
    '';
    doCheck = true;
    checkPhase = ''
      truncate -s 8192 fake
      IPD_DEVMEM_TEST_PATH=$PWD/fake LD_PRELOAD=$PWD/libipd-devmem.so ./devmem_test
    '';
    installPhase = "install -Dm755 libipd-devmem.so $out/lib/libipd-devmem.so";
  };

  # isai-smoke: the generic SAI client ../ifcs-oracle/tools/sai_smoke.cpp;
  # isai-version: loads libsai, prints its SAI version, no device access.
  # Both against isai's SAI 1.16.1 headers.
  smoke = pkgs.stdenv.mkDerivation {
    pname = "isai-smoke";
    inherit (libsai) version;
    dontUnpack = true;
    buildPhase = ''
      sai="-I${libsai}/include/sai -L${libsai}/lib -lsai -Wl,-rpath,${libsai}/lib"
      $CXX -O2 -std=c++17 -o isai-smoke ${../ifcs-oracle/tools/sai_smoke.cpp} $sai
      $CC -O2 -o isai-version ${./guest/isai_version.c} $sai
      $CXX -O2 -std=c++17 -o sai-bench ${./tools/sai_bench.cpp} $sai -ldl
    '';
    installPhase = "install -Dm755 -t $out/bin isai-smoke isai-version sai-bench";
  };

  # upstream syncd/sairedis (the pinned SONiC stack) on this libsai: SAI is
  # binary compatible from 1.12 on (sairedis checks >= 1.9), isai is 1.16.1
  syncd = (import ../sonic { inherit pkgs; }).sairedis-openbcm libsai;
  # isai 6.2.0's sai_attribute_t is 48 bytes (SAI 1.16), the pinned sairedis'
  # 64 (SAI 1.19.2): a vendor create_switch reads the attribute array with the
  # wrong stride. syncd for isai = sairedis of SONiC's 202505 branch (its SAI
  # submodule matches) linked to it, the rest of the stack as pinned.
  syncd-202505 = syncd.overrideAttrs (o: {
    version = "202505-14db8ad";
    src = pkgs.fetchFromGitHub {
      owner = "sonic-net";
      repo = "sonic-sairedis";
      rev = "14db8ad4452f70f375795b2d7d7bf797d2c012b5";   # 202505, 2026-02-12
      fetchSubmodules = true;
      hash = "sha256-TODqyiEA+d37b0/LYUJZfr6JCMuGXCow14/PNXaPWrw=";
    };
    # not the pinned build's big-endian mask fix (the AS7760, ppc); orchagent
    # (pinned, SAI 1.19) asks for capabilities of attributes SAI 1.16 lacks:
    # answer NOT_SUPPORTED instead of throwing (which shuts syncd down)
    patches = [ ./patches/sairedis-202505-capability-unknown-attr.patch ];
    # SAI 1.16's meta generator parses doxygen XML as doxygen 1.9.x wrote it
    # (SONiC 202505 builds on Debian bookworm, 1.9.4); nixpkgs' current
    # doxygen breaks it in either reader mode. Doxygen 1.9.5 from nixos-22.11
    # (a build tool only, from the binary cache), upstream's reader (-S: skip
    # the style checks).
    nativeBuildInputs = map (p: if (p.pname or "") == "doxygen" then doxygen-1_9 else p) o.nativeBuildInputs;
    postPatch = builtins.replaceStrings
      [ "substituteInPlace SAI/meta/Makefile --replace-fail 'perl -I. parse.pl' 'perl -I. -MXML::Simple parse.pl -s -S'" ]
      [ "substituteInPlace SAI/meta/Makefile --replace-fail 'perl -I. parse.pl' 'perl -I. parse.pl -S'" ]
      o.postPatch;
  });
  doxygen-1_9 = (import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/ea4c80b39be4c09702b0cb3b42eab59e2ba4f24b.tar.gz";   # nixos-22.11
    sha256 = "1xi53rlslcprybsvrmipm69ypd3g3hr7wkxvzc73ag8296yclyll";
  }) { system = "x86_64-linux"; }).doxygen;

  # the comparison: our SAI (arm64, native) with the same benchmark tool
  sai-teralynx = cross.callPackage ../sai-teralynx { sonic = import ../sonic { pkgs = cross; }; };
  bench-native = cross.stdenv.mkDerivation {
    pname = "sai-bench-teralynx";
    version = "1";
    dontUnpack = true;
    buildPhase = ''
      $CXX -O2 -std=c++17 -I${sai-teralynx.saiHeaders} -I${sai-teralynx.saiHeaders}/../experimental \
        -o sai-bench ${./tools/sai_bench.cpp} -L${sai-teralynx}/lib -lsai -ldl -Wl,-rpath,${sai-teralynx}/lib
    '';
    installPhase = "install -Dm755 -t $out/bin sai-bench";
  };

  # The oracle (../ifcs-oracle): the same SAI script on both SAIs with
  # their libifcs calls traced. isai: tracer generated from its ctypes file,
  # LD_PRELOAD'ed in the guest (ISAI_QEMU_PRELOAD); ours: the tracer for our
  # backend's table (SAI_TERALYNX_IFCS_TRACE) and the runner on our libsai.
  oracle-isai = pkgs.callPackage ../ifcs-oracle {
    saiInc = "${libsai}/include/sai";
    saiLib = libsai;
    abi = "isai-6.2.0";
    ctypes = "${libsai}/share/isai/pyctypes/ifcs_ctypes.py";
  };
  # (the IFCS declarations the library builds with: ifcs-gen's, from ifcsHeaders)
  oracle-ours = cross.callPackage ../ifcs-oracle {
    saiInc = sai-teralynx.saiHeaders;
    saiLib = sai-teralynx;
    abi = "ifcs-0.12.41";
    ifcsHeaders = ifcsHeaders { pkgs = cross; saiSrc = (import ../sonic { pkgs = cross; }).sources.sairedis; };
  };

  self = {
    qemu-ipd = qemuFor cross;          # for the CS8210 (aarch64 host)
    qemu-ipd-native = qemuFor pkgs;    # same patches, x86_64 host: offline tests
  } // lib.optionalAttrs (ifcsHeaders != null) {
    inherit oracle-ours;
  } // {
    inherit isai libsai ortools-stub devmem smoke syncd syncd-202505 sai-teralynx bench-native oracle-isai;

    # arm64 runner: isai-qemu PROGRAM [ARGS...] (see sai-qemu.md, phase 2)
    runner = cross.writeShellScriptBin "isai-qemu" ''
      # Run an amd64 program (isai-smoke, syncd) here under qemu-ipd:
      # guest_base 0 (ipd ioctls carry pointers), no AVX in the guest CPU,
      # device-safe code generation, device-safe memcpy for the guest only.
      # ISAI_QEMU_PRELOAD: more guest libraries to preload (the oracle's tracer).
      : "${"$"}{QEMU_IPD_STRICT:=1}"
      export QEMU_IPD_STRICT
      exec ${self.qemu-ipd}/bin/qemu-x86_64 -B 0 -cpu Nehalem \
        -E LD_PRELOAD=${devmem}/lib/libipd-devmem.so${"$"}{ISAI_QEMU_PRELOAD:+:${"$"}ISAI_QEMU_PRELOAD} \
        "$@"
    '';
    # arm64 bin/syncd that runs the amd64 upstream syncd (202505, SAI 1.16 ABI)
    # under qemu-ipd (services.sonic.sai.package, see sonic-isai.nix)
    syncd-runner = cross.writeShellScriptBin "syncd" ''
      exec ${self.runner}/bin/isai-qemu ${syncd-202505}/bin/syncd "$@"
    '';
    # everything amd64 the box needs, as one closure to nix-copy
    guest = pkgs.linkFarm "isai-qemu-guest" { inherit smoke devmem libsai syncd; };

    tests = pkgs.callPackage ./tests {
      qemu-ipd = self.qemu-ipd-native;
      qemu-ipd-aarch64 = self.qemu-ipd;
      inherit cross;
    };
  };
in self

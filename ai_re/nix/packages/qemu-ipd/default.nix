# qemu-ipd: qemu-user for x86_64 guests, patched to run Marvell's amd64 Teralynx
# SAI on the CS8210's arm64 CPU against the real ipd driver. Why and how:
# devices/cs8210-32x/sai-qemu.md. Patches on nixpkgs' qemu (11.1.2 at this pin):
#   0001 linux-user: /dev/ipd* ioctls passed to the host unchanged (needs
#        guest_base 0), device mmaps recorded, guest atomics on them fault
#   0002 tcg, QEMU_IPD_STRICT=1: barriers also single-threaded, DMB OSH on
#        aarch64, 16-byte SSE accesses as 8-byte pairs (no LDXP/STXP)
# Entry (x86_64-linux build host; the box binary is cross-built):
#   import ./qemu-ipd { pkgs = import <nixpkgs> { system = "x86_64-linux"; }; }
{ pkgs }:
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

  # isai-smoke: the generic SAI client sai-teralynx/tools/sai_smoke.cpp;
  # isai-version: loads libsai, prints its SAI version, no device access.
  # Both against isai's SAI 1.16.1 headers.
  smoke = pkgs.stdenv.mkDerivation {
    pname = "isai-smoke";
    inherit (libsai) version;
    dontUnpack = true;
    buildPhase = ''
      sai="-I${libsai}/include/sai -L${libsai}/lib -lsai -Wl,-rpath,${libsai}/lib"
      $CXX -O2 -std=c++17 -o isai-smoke ${../sai-teralynx/tools/sai_smoke.cpp} $sai
      $CC -O2 -o isai-version ${./guest/isai_version.c} $sai
    '';
    installPhase = "install -Dm755 -t $out/bin isai-smoke isai-version";
  };

  # upstream syncd/sairedis (the pinned SONiC stack) on this libsai: SAI is
  # binary compatible from 1.12 on (sairedis checks >= 1.9), isai is 1.16.1
  syncd = (import ../sonic { inherit pkgs; }).sairedis-openbcm libsai;

  self = {
    qemu-ipd = qemuFor cross;          # for the CS8210 (aarch64 host)
    qemu-ipd-native = qemuFor pkgs;    # same patches, x86_64 host: offline tests
    inherit isai libsai ortools-stub devmem smoke syncd;

    # arm64 runner: isai-qemu PROGRAM [ARGS...] (see sai-qemu.md, phase 2)
    runner = cross.writeShellScriptBin "isai-qemu" ''
      # Run an amd64 program (isai-smoke, syncd) here under qemu-ipd:
      # guest_base 0 (ipd ioctls carry pointers), no AVX in the guest CPU,
      # device-safe code generation, device-safe memcpy for the guest only.
      : "${"$"}{QEMU_IPD_STRICT:=1}"
      export QEMU_IPD_STRICT
      exec ${self.qemu-ipd}/bin/qemu-x86_64 -B 0 -cpu Nehalem \
        -E LD_PRELOAD=${devmem}/lib/libipd-devmem.so \
        "$@"
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

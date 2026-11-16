# Marvell's published amd64 Teralynx SAI, isai 6.2.0 (SAI 1.16.1), from
# github.com/Marvell-switching/sonic-marvell-teralynx-binaries debian/202505/T6.2.0:
# fetch + unpack only. Vendor binaries live in the Nix store, never in git.
# Why this release (its libifcs attaches only to ipd 0.16.2.0.0.0, the driver
# the CS8210 runs): devices/cs8210-32x/sai-qemu.md.
#   callPackage ./isai-amd64 { libortools = <stub, see ../qemu-ipd/guest>; }
# Outputs: debs (fetchurl), unpacked (dpkg-deb -x of each, untouched) and
# libsai: libsai.so + libmkaecp.so with RUNPATHs to their dependencies (no
# other change), the SAI headers and the ishell configs. libifcs.so (590 MB,
# only the vendor CLI uses it) is left out of libsai.
{ lib, stdenv, stdenvNoCC, fetchurl, dpkg, autoPatchelfHook
, jansson, libyaml, python311, libortools }:
let
  rev = "ff24d20223c922c05e67bf9a108a5efb911b1091";   # 2025-11-07, adds T6.2.0
  deb = name: hash: fetchurl {
    url = "https://raw.githubusercontent.com/Marvell-switching/sonic-marvell-teralynx-binaries/${rev}/debian/202505/T6.2.0/${name}.deb";
    inherit hash;
  };
  debs = {
    isai = deb "isai" "sha256-gkkXVjjJC2NA+8IQUoHvdx7l/GPKVlenSCA1DOm9uQ8=";
    saihdr = deb "saihdr" "sha256-0CjAGnQFvM3LHlf99643iyD2oiyWT3zInRlT2PY4WdU=";
    ishell = deb "ishell" "sha256-XUdVlGlN/rHmYP5atfEVPoHnZ3WNwyIyymLnI1kNsQw=";
    ipd = deb "ipd" "sha256-VgU5JQ/X9q2LzIuIyXU0PmNkE99Cg9xqTnIa8R4XI04=";
  };
  version = "6.2.0";

  unpacked = stdenvNoCC.mkDerivation {
    pname = "isai-amd64-unpacked";
    inherit version;
    nativeBuildInputs = [ dpkg ];
    dontUnpack = true;
    dontFixup = true;
    installPhase = lib.concatStrings (lib.mapAttrsToList (n: d: ''
      mkdir -p $out/${n}
      dpkg-deb -x ${d} $out/${n}
      dpkg-deb -e ${d} $out/${n}/DEBIAN
    '') debs);
  };
in {
  inherit debs unpacked;

  libsai = stdenv.mkDerivation {
    pname = "isai";
    inherit version;
    src = unpacked;
    nativeBuildInputs = [ autoPatchelfHook ];
    buildInputs = [ jansson libyaml python311 stdenv.cc.cc.lib libortools ];
    dontConfigure = true;
    dontBuild = true;
    dontStrip = true;           # vendor objects: only RUNPATH changes
    installPhase = ''
      mkdir -p $out/lib $out/include $out/share/isai
      cp -P isai/usr/lib/libsai.so* isai/usr/lib/libifcssai.so \
            isai/usr/lib/libmkaecp.so* $out/lib/
      cp -r saihdr/usr/include/sai $out/include/sai
      cp -r ishell/innovium/configs ishell/innovium/pyctypes $out/share/isai/
    '';
    # Every symbol libsai and libmkaecp need resolves (RTLD_NOW); this runs
    # their constructors in the sandbox, nothing else.
    doInstallCheck = true;
    installCheckPhase = ''
      cat > dl.c <<'EOF'
      #include <dlfcn.h>
      #include <stdio.h>
      int main(int c, char **v) {
        for (int i = 1; i < c; i++)
          if (!dlopen(v[i], RTLD_NOW | RTLD_GLOBAL)) { fprintf(stderr, "%s\n", dlerror()); return 1; }
        puts("dlopen RTLD_NOW ok"); return 0;
      }
      EOF
      $CC dl.c -o dl -ldl
      ./dl $out/lib/libmkaecp.so.1.0.0 $out/lib/libsai.so.1.0
    '';
    passthru = { inherit unpacked debs; libsdk = null; };
    meta = {
      description = "Marvell Teralynx SAI ${version} (amd64 binary, sonic-marvell-teralynx-binaries)";
      license = lib.licenses.unfree;
      platforms = [ "x86_64-linux" ];
    };
  };
}

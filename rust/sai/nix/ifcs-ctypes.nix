# The pinned vendor ctypes input of the IFCS declarations (store only, never git): pyctypes/ifcs_ctypes.py
# of Innovium's public ishell 2.0.1 deb (Innovium/SONiC debian/202111/T2.0.1, amd64; the file is
# arch-neutral python). Its declarations of everything ifcs-gen's templates use equal the box's
# 0.12.41 ones except the overrides (crates/ifcs-gen/templates/ifcs_overrides.txt) and one @prefix struct.
# Moved here unchanged from the C++ sai-teralynx package's default.nix (H5-1): the same derivations, the
# same store paths (nix/ifcs-ctypes.nix in rust/sai is now the one every user takes it from).
#   pkgs.callPackage ./ifcs-ctypes.nix { }  ->  { ishellDeb, ifcsCtypes }
{ fetchurl, buildPackages }:
rec {
  ishellDeb = fetchurl {
    url = "https://raw.githubusercontent.com/Innovium/SONiC/240ee432f6d6b3d36ca8c484d3f7051714279545/debian/202111/T2.0.1/ishell.deb";   # 2025-11-25
    hash = "sha256-iUync4A9AUm68UcyoTYDdAkgvOXwFbNDfj28R2H39ek=";
  };
  ifcsCtypes = buildPackages.runCommand "ifcs-ctypes-ishell-2.0.1.py" { nativeBuildInputs = [ buildPackages.dpkg ]; } ''
    dpkg-deb -x ${ishellDeb} x
    install -Dm644 x/innovium/pyctypes/ifcs_ctypes.py $out
    echo "5a4547fb452a2b4fe85c449ed7089033f81d133f07646168ce01eac2a2843069  $out" | sha256sum -c
  '';
}

# Board reset through the DMC-12 CPLD on reboot (out-of-tree, ~70 lines).
# See dmc12_cpld_reset.c for why a reboot notifier and not a userspace hook.
{ lib, stdenv, kernel }:
stdenv.mkDerivation {
  pname = "dmc12-cpld-reset";
  inherit (kernel) version;
  src = builtins.path { path = ./.; name = "dmc12-cpld-reset-src"; filter = p: t: !(lib.hasSuffix ".nix" p) && !(t == "directory" && baseNameOf p == "rust"); };
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  nativeBuildInputs = kernel.moduleBuildDependencies;
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
  ];
  buildPhase = ''
    runHook preBuild
    make "''${makeFlags[@]}" M=$(pwd) modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
    runHook postInstall
  '';
  meta = { description = "DMC-12 CPLD board reset on reboot"; license = lib.licenses.gpl2Only; platforms = [ "aarch64-linux" ]; };
}

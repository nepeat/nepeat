# fastfetch with no optional libraries: nixpkgs always builds the unwrapped
# package against chafa/imagemagick/rpm/efl/libdrm/..., which neither
# cross-builds for odd targets (libhwy wants VSX, nss and valgrind fail on
# ppc64 BE) nor fits a small image. They are all optional at configure time.
{ lib, fastfetch-unwrapped, yyjson, hwdata }:
fastfetch-unwrapped.overrideAttrs (old: {
  pname = "fastfetch-tiny";
  buildInputs = [ yyjson ];
  cmakeFlags = [
    (lib.cmakeOptionType "filepath" "CMAKE_INSTALL_SYSCONFDIR" "${placeholder "out"}/etc")
    (lib.cmakeBool "ENABLE_DIRECTX_HEADERS" false)
    (lib.cmakeBool "ENABLE_SYSTEM_YYJSON" true)
    (lib.cmakeBool "ENABLE_IMAGEMAGICK6" false)
    (lib.cmakeBool "ENABLE_EMBEDDED_PCIIDS" true)
  ];
  # nixpkgs also embeds libdrm's amdgpu.ids here, i.e. builds libdrm for the target.
  preConfigure = ''
    buildDir="''${cmakeBuildDir:-build}"
    mkdir -p "$buildDir"
    cp ${hwdata}/share/hwdata/pci.ids "$buildDir/pci.ids"
  '';
  doInstallCheck = false; # versionCheckHook cannot run a cross binary
})

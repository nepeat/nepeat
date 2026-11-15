{ pkgs, kernel }:
# AS7760-32X fan CPLD hwmon driver (out-of-tree module for the given kernel)
# and its userspace control/monitor tool.
{
  driver = pkgs.stdenv.mkDerivation {
    pname = "as7760-fan";
    version = "0.1";
    src = builtins.path { path = ./src; name = "as7760-fan-source"; };
    nativeBuildInputs = kernel.moduleBuildDependencies;
    hardeningDisable = [ "all" ];
    __structuredAttrs = true;
    strictDeps = true;
    makeFlags = kernel.commonMakeFlags ++ [
      "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    ];
    buildPhase = ''make "''${makeFlags[@]}" M="$PWD" modules'';
    installPhase = ''
      # Use kbuild installation so nixpkgs also generates the per-module
      # certificate required by its module aggregator.
      make "''${makeFlags[@]}" M="$PWD" INSTALL_MOD_PATH="$out" \
        INSTALL_MOD_DIR=extra DEPMOD=true modules_install
    '';
  };
  tool = pkgs.writeShellApplication {
    name = "as7760-fan";
    runtimeInputs = [ pkgs.python3 ];
    text = ''exec python3 ${pkgs.writeText "as7760-fan-control.py" (builtins.readFile ./control.py)} "$@"'';
  };
}

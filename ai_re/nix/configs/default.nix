# Complete systems built from ../modules, plus the images derived from them.
#   import ./configs { inherit nixpkgs; }  ->  { nixosConfigurations, packages }
# nixpkgs: a nixpkgs flake/input (needs .lib.nixosSystem and to be importable).
{ nixpkgs, buildSystem ? "x86_64-linux" }:
let
  native = import nixpkgs { system = buildSystem; };
  mkHost = platform: modules: nixpkgs.lib.nixosSystem {
    modules = modules ++ [ { nixpkgs.buildPlatform = buildSystem; nixpkgs.hostPlatform = platform; } ];
  };

  # --- Accton AS7760-32X ---------------------------------------------------
  as7760Platform = import ./as7760/platform.nix;
  # Installed system on the SD card (nixpkgs sd-image layout; deploy in place).
  as7760 = mkHost as7760Platform [ ./as7760/configuration.nix ../modules/boards/as7760/sd-install.nix ];
  # Same system, root in RAM (squashfs store in the initramfs) for TFTP boots.
  as7760Netboot = mkHost as7760Platform [
    ./as7760/configuration.nix ../modules/profiles/netboot-root.nix ../modules/boards/as7760/sd-persist.nix
  ];
  img = ../packages/as7760-images;
  kernel = as7760.config.boot.kernelPackages.kernel;
  fanDriver = builtins.head as7760.config.boot.extraModulePackages;
  kernelImage = import ../packages/linux-qoriq-t2080/uimage.nix { pkgs = native; targetPkgs = as7760.pkgs; inherit kernel; };
  netboot = import (img + "/netboot-files.nix") { pkgs = native; config = as7760Netboot.config; inherit kernelImage; };
in {
  nixosConfigurations = { inherit as7760; as7760-netboot = as7760Netboot; };
  packages = {
    as7760-system = as7760.config.system.build.toplevel;
    as7760-kernel = kernel;
    as7760-kernel-config = kernel.configfile;
    as7760-kernel-image = kernelImage;
    as7760-fan-driver = fanDriver;
    # Installed system image (first install; later: nixos-rebuild in place).
    as7760-sd-image = as7760.config.system.build.sdImage;
    as7760-boot-files = import (img + "/boot-files.nix") { pkgs = native; targetPkgs = as7760.pkgs; config = as7760.config; inherit kernelImage; };
    as7760-netboot = netboot;
    # RAM-root alternative: SD as a read-only boot medium (sd-persist.nix).
    as7760-sd-image-ram = import (img + "/sd-image-ram.nix") { pkgs = native; bundle = netboot; };
    as7760-ram-probe = import (img + "/ram-probe.nix") { pkgs = native; targetPkgs = as7760.pkgs; inherit kernelImage kernel fanDriver; };
    # Budget on the real transitive runtime closure; separate so a failure is inspectable.
    as7760-size-check = native.runCommand "as7760-runtime-size-check" {
      nativeBuildInputs = [ native.python3 ];
      closure = native.closureInfo { rootPaths = [ as7760.config.system.build.toplevel ]; };
    } ''
      python3 ${img + "/check-size.py"} "$closure/store-paths" "$out"
    '';
  };
}

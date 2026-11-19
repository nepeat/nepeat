# Complete systems built from ../modules, plus the images derived from them.
#   import ./configs { inherit nixpkgs; }  ->  { nixosConfigurations, packages }
# nixpkgs: a nixpkgs flake/input (needs .lib.nixosSystem and to be importable).
# rust: the monorepo's rust flake (fand); null keeps the older fan monitor.
{ nixpkgs, buildSystem ? "x86_64-linux", rust ? null }:
let
  native = import nixpkgs { system = buildSystem; };
  mkHost = platform: modules: nixpkgs.lib.nixosSystem {
    specialArgs.aiReRust = rust;
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
  openbcm = import ../packages/openbcm { pkgs = as7760.pkgs; };
  kernelImage = import ../packages/linux-qoriq-t2080/uimage.nix { pkgs = native; targetPkgs = as7760.pkgs; inherit kernel; };
  netboot = import (img + "/netboot-files.nix") { pkgs = native; config = as7760Netboot.config; inherit kernelImage; };

  # --- Edgecore DMC-12-48R-AS0 (Alpine V2) ---------------------------------
  # Installed on the SSD: a full, rebuildable Nix system (nixos-rebuild --target-host).
  dmc12 = mkHost "aarch64-linux" [ ./dmc12/configuration.nix ../modules/profiles/rebuildable.nix ../modules/boards/dmc12/disk.nix ];
  # Same, booted by the chainloaded upstream U-Boot (raw sda1, extlinux on sda2).
  dmc12Chainload = mkHost "aarch64-linux" [
    ./dmc12/configuration.nix ../modules/profiles/rebuildable.nix ../modules/boards/dmc12/disk.nix
    { hardware.dmc12.bootLoader = "chainload"; }
  ];
  # Same system, root in RAM, for TFTP boots and as the reimaging environment. Not rebuildable:
  # its store is never registered, so a nix GC there would delete the running system.
  dmc12Netboot = mkHost "aarch64-linux" [
    ./dmc12/configuration.nix ../modules/profiles/netboot-root.nix { boot.loader.grub.enable = false; }
  ];
  dmc12NetbootFiles = import ../packages/dmc12-images/netboot-files.nix { pkgs = native; config = dmc12Netboot.config; };

  # --- Edgecore AS5114-48X (Armada 7040) -----------------------------------
  # Installed on the SATA SSD (sd-image layout), booted by the factory U-Boot's sysboot.
  as5114 = mkHost "aarch64-linux" [ ./as5114/configuration.nix ];
in {
  nixosConfigurations = { inherit as7760 dmc12 as5114; as7760-netboot = as7760Netboot; dmc12-netboot = dmc12Netboot; };
  packages = {
    as5114-system = as5114.config.system.build.toplevel;
    as5114-sd-image = as5114.config.system.build.sdImage;
    dmc12-system = dmc12.config.system.build.toplevel;
    dmc12-kernel = dmc12.config.boot.kernelPackages.kernel;
    dmc12-dtbs = dmc12.config.system.build.alpineDtbs;
    dmc12-disk-image = dmc12.config.system.build.diskImage;
    dmc12-disk-image-chainload = dmc12Chainload.config.system.build.diskImage;
    dmc12-netboot = dmc12NetbootFiles;
    as7760-system = as7760.config.system.build.toplevel;
    as7760-kernel = kernel;
    as7760-kernel-config = kernel.configfile;
    as7760-kernel-image = kernelImage;
    as7760-fan-driver = fanDriver;
    # Broadcom OpenBCM SDK for this board's kernel/userspace (see packages/openbcm).
    as7760-openbcm-kmods = openbcm.kmodsFor kernel;
    as7760-bcm-user = openbcm.bcm-user { };
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

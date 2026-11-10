# cs8210 -- Celestica CS8210-32X switch running NixOS on its Alpine v3 CPU card.
#
# Bring-up stage: the goal is boot + console, then management Ethernet, PCIe
# (switch ASIC + FPGAs), TPM. Nothing here drives the front-panel ports.
#
# Install is a raw image, not an installer: build
#   nix build .#nixosConfigurations.cs8210.config.system.build.sdImage
# and dd it onto the M.2 SSD (/dev/sda) from ONIE. Partition 1 is the unused
# "firmware" FAT stub the sd-image module always creates, partition 2 is the
# ext4 root, grown to the whole disk on first boot. See module/cs8210.
{
  lib,
  modulesPath,
  config,
  ...
}:

{
  system.stateVersion = "26.05";

  imports = [
    (modulesPath + "/installer/sd-card/sd-image.nix")
    ../../module/cs8210
  ];

  # Kernel + al_eth are cross-built from the x86 desktop; the only aarch64
  # builder otherwise is binfmt emulation.
  hardware.cs8210.kernelBuildSystem = "x86_64-linux";

  image.baseName = "nixos-cs8210";

  sdImage = {
    compressImage = false;
    firmwareSize = 16;
    populateFirmwareCommands = "";
    populateRootCommands = ''
      mkdir -p ./files/boot
      CS8210_BOOT_TARGET=./files/boot CS8210_PROFILES_DIR=/nonexistent \
        ${config.system.build.installBootLoader} ${config.system.build.toplevel}
    '';
  };

  # DHCP on the management port only. The front-panel netdevs (fpp1..fpp32,
  # inno_*) appear when the switch SDK starts and must not be auto-configured.
  networking.useDHCP = false;
  networking.interfaces.enp0s1.useDHCP = true;

  # Bring-up convenience: the serial console is the only way in until al_eth
  # works, and every account here is SSH-key-only. Physical access to the
  # console port already means U-Boot access, so this gives away nothing new.
  services.getty.autologinUser = "root";

  # Nothing on this box wants these from the shared base.
  programs.nix-index.enable = lib.mkForce false;
  programs.nix-index-database.comma.enable = lib.mkForce false;
  documentation.nixos.enable = false;

  # lldpd bundles an old copy of linux/types.h that shadows the system one and
  # has no __u128. arm64's asm/sigcontext.h in current kernel headers needs
  # it, so lldpd 1.0.22 fails to compile for aarch64 (x86 is unaffected, and
  # there is no cached aarch64 build on nixpkgs master). Add the typedefs the
  # real header has. Drop this once nixpkgs/lldpd carry a fix.
  nixpkgs.overlays = [
    (final: prev: {
      lldpd = prev.lldpd.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./lldpd-bundled-types-u128.patch ];
      });
    })
  ];

  # Root logins: the keys published at https://github.com/nepeat.keys (as
  # fetched 2026-11-09) plus root on sea420-desktop, the build/deploy host.
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
  ];
}

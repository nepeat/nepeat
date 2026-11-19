# Edgecore AS5114-48X: basic NixOS port, installed on the SATA SSD.
# Image: nix build path:./nix#as5114-sd-image, written to /dev/sda from ONIE
# (devices/edge-core-as5114-48x/nixos-install.md). Boot: the factory U-Boot's
# nos_bootcmd runs `sysboot` on p1's /extlinux/extlinux.conf; ONIE stays the fallback.
# Then deploy in place: nixos-rebuild switch --flake path:./nix#as5114 --target-host root@<ip>
#   p1 FAT "NIXOS_BOOT" 512 MiB = /boot (extlinux.conf + nixos/ kernels, initrds, DTBs)
#   p2 ext4 "NIXOS_SD" = /, grown to the whole SSD on first boot.
{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [
    (modulesPath + "/installer/sd-card/sd-image.nix")
    ../../modules/profiles/appliance.nix
    ../../modules/profiles/rebuildable.nix
    ../../modules/boards/as5114
  ];
  system.stateVersion = "26.11";
  networking.hostName = "as5114";
  time.timeZone = "America/Los_Angeles";

  # Stock mainline from nixpkgs: it has PRESTERA_PCI, MVPP2, NVMEM_LAYOUT_ONIE_TLV
  # (as on the TN48M). A trimmed kernel is a follow-up.
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # sd-image pulls in all-hardware; this board needs none of it.
  hardware.enableAllHardware = lib.mkForce false;
  image.baseName = "nixos-as5114";
  sdImage = {
    compressImage = false;
    firmwareSize = 512;
    firmwarePartitionName = "NIXOS_BOOT";
    populateFirmwareCommands = ''
      ${config.boot.loader.generic-extlinux-compatible.populateCmd} -c ${config.system.build.toplevel} -d ./firmware
    '';
    populateRootCommands = "";
    postBuildCommands = "sfdisk --activate $img 1";
  };
  fileSystems."/boot/firmware".enable = false;
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/NIXOS_BOOT";
    fsType = "vfat";
    options = [ "fmask=0077" "dmask=0077" ];
  };
  boot.loader.generic-extlinux-compatible.configurationLimit = 5;

  # Mgmt only (mvpp2, MAC = TLV base via the U-Boot fixup / nvmem). Prestera
  # front-panel netdevs match nothing, so they stay admin-down.
  networking.useDHCP = false;
  networking.useNetworkd = true;
  networking.usePredictableInterfaceNames = false;
  systemd.network = {
    enable = true;
    wait-online.enable = false;
    networks."10-mgmt" = {
      matchConfig.Driver = "mvpp2";
      networkConfig = { DHCP = "yes"; IPv6AcceptRA = true; };
      dhcpV4Config.ClientIdentifier = "mac";
      linkConfig.RequiredForOnline = "no";
    };
  };
  networking.firewall.allowedTCPPorts = [ 22 ];
  networking.nftables.enable = true;

  # Serial console is physical access (it already means U-Boot access).
  services.getty.autologinUser = "root";

  # Same root keys as the DMC-12 / AS7760 (https://github.com/nepeat.keys + the build host).
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
  ];
}

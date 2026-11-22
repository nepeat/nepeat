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
    ../lab-access.nix
    ../../modules/profiles/appliance.nix
    ../../modules/profiles/rebuildable.nix
    ../../modules/boards/as5114
  ];
  system.stateVersion = "26.11";
  networking.hostName = "as5114";

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
  # The rest of the network and the root keys: ../lab-access.nix.
  lab.managementInterface = null;
  systemd.network.networks."10-mgmt" = {
    matchConfig.Driver = "mvpp2";
    networkConfig = { DHCP = "yes"; IPv6AcceptRA = true; };
    dhcpV4Config.ClientIdentifier = "mac";
    linkConfig.RequiredForOnline = "no";
  };
}

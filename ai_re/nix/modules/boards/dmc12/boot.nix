# How the DMC-12 boots NixOS from its SSD: hardware.dmc12.bootLoader and the three loaders.
# Imported by disk.nix (our disk image); hosts with their own disk layout (disko) import it
# alone, as nixosModules.board-dmc12-boot.
{
  imports = [ ./bootloader.nix ./uboot-menu ./boot-chainload.nix ./boot-standard.nix ];
}

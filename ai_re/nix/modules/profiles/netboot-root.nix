{ config, lib, pkgs, ... }:
# Non-destructive full-NixOS boot: the whole system closure travels inside the
# initramfs as a zstd squashfs, overlaid with tmpfs. Nothing on the board's
# SD/NOR is mounted or written. Modelled on nixpkgs installer/netboot/netboot.nix,
# minus its nix-daemon registration (nix.enable = false here) and kexec bits.
{
  fileSystems."/" = { fsType = "tmpfs"; options = [ "mode=0755" "size=4G" ]; };
  fileSystems."/nix/.ro-store" = {
    fsType = "squashfs";
    device = "../nix-store.squashfs";
    options = [ "loop" ];
    neededForBoot = true;
  };
  fileSystems."/nix/.rw-store" = {
    fsType = "tmpfs";
    options = [ "mode=0755" ];
    neededForBoot = true;
  };
  fileSystems."/nix/store" = {
    overlay = {
      lowerdir = [ "/nix/.ro-store" ];
      upperdir = "/nix/.rw-store/store";
      workdir = "/nix/.rw-store/work";
    };
    neededForBoot = true;
  };

  system.build.squashfsStore = pkgs.callPackage (pkgs.path + "/nixos/lib/make-squashfs.nix") {
    storeContents = [ config.system.build.toplevel ];
    comp = "zstd -Xcompression-level 19";
  };
  # Stage-1 gzip initrd first, then an uncompressed cpio holding the already
  # compressed squashfs (no point gzipping zstd data twice).
  system.build.squashfsCpio = pkgs.makeInitrdNG {
    name = "as7760-squashfs-cpio";
    compressor = "cat";
    contents = [ { source = config.system.build.squashfsStore; target = "/nix-store.squashfs"; } ];
  };
  # Hardware result 2026-11-14: a raw cpio straight after the gzip member hit
  # "Initramfs unpacking failed: invalid magic". The kernel only recognises an
  # uncompressed cpio at a 4-byte aligned offset (zero padding is skipped), so
  # pad the gzip stage to 512 bytes before appending.
  system.build.netbootRamdisk = pkgs.runCommand "as7760-netboot-initrd" { } ''
    mkdir $out
    cat ${config.system.build.initialRamdisk}/initrd > $out/initrd
    truncate -s %512 $out/initrd
    cat ${config.system.build.squashfsCpio}/initrd >> $out/initrd
  '';
  # Bring-up box with serial root autologin anyway: let stage-1 failures land
  # in a usable emergency shell instead of "root account is locked".
  boot.initrd.systemd.emergencyAccess = true;
}

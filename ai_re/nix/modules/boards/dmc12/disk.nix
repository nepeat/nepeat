# DMC-12 installed on its M.2 SATA SSD, booted by the factory U-Boot 2015.07.
#   sda1  ext4 /boot, 512 MiB, made with the factory's feature set (no 64bit or
#         metadata_csum: U-Boot 2015.07 ext4load can't read those). Holds
#         boot.scr + u-boot.menu + nixos/* from ./uboot-menu.
#   sda2  ext4 /, grown to fill the disk on first boot.
# MBR, not GPT: a small image on a 32 GB disk would leave GPT's backup header
# in the middle. U-Boot needs one env change to run /boot.scr (see
# devices/dmc-12-48r-as0/reimage.md); the factory chain stays as the fallback.
{ config, lib, pkgs, ... }:
let
  bootSizeMiB = 512;
  toplevel = config.system.build.toplevel;
  rootfs = pkgs.callPackage (pkgs.path + "/nixos/lib/make-ext4-fs.nix") {
    storePaths = [ toplevel ];
    volumeLabel = "DMC12ROOT";
    compressImage = false;
    populateImageCommands = ''
      mkdir -p ./files/boot ./files/etc
    '';
  };
  bb = pkgs.buildPackages;
in
{
  imports = [ ./uboot-menu ];

  fileSystems."/" = {
    device = "/dev/disk/by-label/DMC12ROOT";
    fsType = "ext4";
    autoResize = true;
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/DMC12BOOT";
    fsType = "ext4";
    options = [ "noatime" ];
  };
  boot.growPartition = true;

  system.build.diskImage = pkgs.runCommand "dmc12-disk-image" {
    nativeBuildInputs = [ bb.e2fsprogs bb.util-linux bb.zstd bb.coreutils ];
  } ''
    set -euo pipefail
    mkdir -p boot $out
    # Stage /boot exactly as activation would (default entry only: no profiles).
    ${config.system.build.dmc12BootBuilderNative} ${toplevel} -d "$PWD/boot"

    truncate -s ${toString bootSizeMiB}M boot.img
    mkfs.ext4 -q -F -L DMC12BOOT -b 4096 -I 256 \
      -O ^64bit,^metadata_csum,^metadata_csum_seed,^orphan_file,uninit_bg \
      -E root_owner=0:0 -d boot boot.img
    tune2fs -l boot.img | grep -E '^Filesystem features' > $out/boot-features.txt
    if grep -E -q '64bit|metadata_csum|orphan_file' $out/boot-features.txt; then
      echo "boot fs has features U-Boot 2015.07 cannot read"; exit 1
    fi

    root=${rootfs}
    rootBytes=$(stat -c %s $root)
    bootStart=2048                                  # 1 MiB
    bootSectors=$(( ${toString bootSizeMiB} * 2048 ))
    rootStart=$(( bootStart + bootSectors ))
    rootSectors=$(( (rootBytes + 511) / 512 ))
    total=$(( (rootStart + rootSectors + 2048) * 512 ))
    truncate -s $total disk.img
    sfdisk --quiet disk.img <<EOF
    label: dos
    start=$bootStart, size=$bootSectors, type=83, bootable
    start=$rootStart, size=$rootSectors, type=83
    EOF
    dd if=boot.img of=disk.img bs=512 seek=$bootStart conv=notrunc,sparse status=none
    dd if=$root of=disk.img bs=512 seek=$rootStart conv=notrunc,sparse status=none

    sha256sum disk.img | cut -d' ' -f1 > $out/disk.img.sha256
    stat -c %s disk.img > $out/disk.img.bytes
    zstd -q -T4 -19 --long=27 disk.img -o $out/disk.img.zst
    sha256sum $out/disk.img.zst | cut -d' ' -f1 > $out/disk.img.zst.sha256
    sfdisk -d disk.img > $out/partitions.txt
    echo ${toplevel} > $out/toplevel
  '';
}

# DMC-12 installed on its M.2 SATA SSD. MBR, not GPT: a small image on a
# 32 GB disk would leave GPT's backup header in the middle. "uboot-menu" and
# "chainload" need one factory U-Boot env change (devices/dmc-12-48r-as0/reimage.md);
# "standard" needs none. Layout by hardware.dmc12.bootLoader:
#
# "standard" (our U-Boot in SPI, standard boot, ./boot-standard.nix):
#   sda1  ext4 /boot, 512 MiB, default features, generic-extlinux-compatible.
#   sda2  ext4 /, grown on first boot.
# "uboot-menu" (factory U-Boot boots NixOS itself, ./uboot-menu):
#   sda1  ext4 /boot, 512 MiB, made with the factory's feature set (no 64bit or
#         metadata_csum: U-Boot 2015.07 ext4load can't read those). Holds
#         boot.scr + u-boot.menu + nixos/*.
#   sda2  ext4 /, grown to fill the disk on first boot.
# "chainload" (factory U-Boot bootm's upstream U-Boot, ./boot-chainload.nix):
#   sda1  8 MiB raw (type 0xda): upstream U-Boot uImage in slots A and B,
#         read by the factory loader with `scsi read` (no ext4 there).
#   sda2  ext4 /boot, 512 MiB, default features, extlinux.
#   sda3  ext4 /, grown on first boot.
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
  ubootMenuImage = pkgs.runCommand "dmc12-disk-image" {
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

  # Standard boot: /boot staged by nixpkgs' own extlinux populate command.
  standardImage = pkgs.runCommand "dmc12-disk-image" {
    nativeBuildInputs = [ bb.e2fsprogs bb.util-linux bb.zstd bb.coreutils ];
  } ''
    set -euo pipefail
    mkdir -p boot $out
    ${config.boot.loader.generic-extlinux-compatible.populateCmd} -c ${toplevel} -d "$PWD/boot"
    truncate -s ${toString bootSizeMiB}M boot.img
    mkfs.ext4 -q -F -L DMC12BOOT -E root_owner=0:0 -d boot boot.img
    tune2fs -l boot.img | grep -E '^Filesystem features' > $out/boot-features.txt

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

    echo standard > $out/loader.txt                 # reimage.py: no factory env, SPI U-Boot
    sha256sum disk.img | cut -d' ' -f1 > $out/disk.img.sha256
    stat -c %s disk.img > $out/disk.img.bytes
    zstd -q -T4 -19 --long=27 disk.img -o $out/disk.img.zst
    sha256sum $out/disk.img.zst | cut -d' ' -f1 > $out/disk.img.zst.sha256
    sfdisk -d disk.img > $out/partitions.txt
    echo ${toplevel} > $out/toplevel
  '';

  # Same staging and root image; raw U-Boot partition in front of /boot.
  chain = config.system.build.dmc12Chainload or null;
  chainloadImage = pkgs.runCommand "dmc12-disk-image" {
    nativeBuildInputs = [ bb.e2fsprogs bb.util-linux bb.zstd bb.coreutils ];
  } (let l = chain.layout; in ''
    set -euo pipefail
    mkdir -p boot $out
    ${config.system.build.dmc12BootBuilderNative} ${toplevel} -d "$PWD/boot"
    truncate -s ${toString bootSizeMiB}M boot.img
    mkfs.ext4 -q -F -L DMC12BOOT -E root_owner=0:0 -d boot boot.img
    tune2fs -l boot.img | grep -E '^Filesystem features' > $out/boot-features.txt

    root=${rootfs}
    rootBytes=$(stat -c %s $root)
    bootStart=${toString l.bootStart}
    bootSectors=$(( ${toString bootSizeMiB} * 2048 ))
    rootStart=$(( bootStart + bootSectors ))
    rootSectors=$(( (rootBytes + 511) / 512 ))
    total=$(( (rootStart + rootSectors + 2048) * 512 ))
    truncate -s $total disk.img
    sfdisk --quiet disk.img <<EOF
    label: dos
    start=${toString l.partStart}, size=${toString l.partSectors}, type=da
    start=$bootStart, size=$bootSectors, type=83, bootable
    start=$rootStart, size=$rootSectors, type=83
    EOF
    u=${chain.uimage}
    [ $(stat -c %s $u) -le $(( ${toString l.slotSectors} * 512 )) ]
    dd if=$u of=disk.img bs=512 seek=${toString l.slotA} conv=notrunc status=none
    dd if=$u of=disk.img bs=512 seek=${toString l.slotB} conv=notrunc status=none
    dd if=boot.img of=disk.img bs=512 seek=$bootStart conv=notrunc,sparse status=none
    dd if=$root of=disk.img bs=512 seek=$rootStart conv=notrunc,sparse status=none

    # The factory env this disk needs (reimage.py): exact stored values.
    cat > $out/factory-env.txt <<'ENV'
    nixos_boot=${chain.factoryEnv.nixos_boot}
    bootcmd=${chain.factoryEnv.bootcmd}
    ENV
    sha256sum $u | cut -d' ' -f1 > $out/u-boot.uimage.sha256
    sha256sum disk.img | cut -d' ' -f1 > $out/disk.img.sha256
    stat -c %s disk.img > $out/disk.img.bytes
    zstd -q -T4 -19 --long=27 disk.img -o $out/disk.img.zst
    sha256sum $out/disk.img.zst | cut -d' ' -f1 > $out/disk.img.zst.sha256
    sfdisk -d disk.img > $out/partitions.txt
    echo ${toplevel} > $out/toplevel
  '');
in
{
  imports = [ ./boot.nix ];

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

  # First boot of an image: register the store paths the image ships so nix and
  # nixos-rebuild work, and create the system profile (as nixpkgs' sd-image does).
  # If registration ever failed, the store DB is empty: a GC would then delete the system.
  systemd.services.nix-gc.unitConfig.ConditionPathExists = lib.mkIf config.nix.enable "!/nix-path-registration";
  systemd.services.nix-daemon.unitConfig.ConditionPathExists = lib.mkIf config.nix.enable "!/nix-path-registration";
  systemd.services.dmc12-register-store = lib.mkIf config.nix.enable {
    description = "Register the disk image's Nix store paths";
    unitConfig = { DefaultDependencies = false; ConditionPathExists = "/nix-path-registration"; };
    wantedBy = [ "sysinit.target" ];
    before = [ "sysinit.target" "shutdown.target" "nix-daemon.socket" "nix-daemon.service" ];
    after = [ "local-fs.target" ];
    conflicts = [ "shutdown.target" ];
    restartIfChanged = false;
    serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
    script = ''
      ${lib.getExe' config.nix.package.out "nix-store"} --load-db < /nix-path-registration
      touch /etc/NIXOS
      ${lib.getExe' config.nix.package.out "nix-env"} -p /nix/var/nix/profiles/system --set /run/current-system
      rm -f /nix-path-registration
    '';
  };

  system.build.diskImage = {
    standard = standardImage;
    chainload = chainloadImage;
    uboot-menu = ubootMenuImage;
  }.${config.hardware.dmc12.bootLoader};
}

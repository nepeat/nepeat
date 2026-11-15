{ pkgs, bundle }:
# 1-GiB SD image for the RAM-root system: MBR + one FAT32 partition holding the
# netboot bundle (uImage, uInitrd with the squashfs store, DTB) and the U-Boot
# command list. Linux never writes it except once for the SSH host key
# (sd-persist.nix). The rest of the card stays unallocated.
pkgs.runCommand "as7760-sd-image" {
  nativeBuildInputs = with pkgs; [ util-linux dosfstools mtools ];
} ''
  mkdir -p $out
  truncate -s 1025M $out/as7760-sd.img
  sfdisk $out/as7760-sd.img <<'EOF'
  label: dos
  unit: sectors
  start=2048, size=2097152, type=c, bootable
  EOF
  truncate -s 1024M boot.fat
  mkfs.vfat -F 32 -n NIXOS_BOOT boot.fat
  mcopy -i boot.fat ${bundle}/uImage ${bundle}/uInitrd ${bundle}/as7760.dtb \
    ${bundle}/bootargs.txt ${bundle}/sd-boot-commands.txt ${bundle}/boot.scr ${bundle}/manifest.json ::/
  mmd -i boot.fat ::/persist
  fsck.fat -n boot.fat
  dd if=boot.fat of=$out/as7760-sd.img bs=1M seek=1 conv=notrunc status=none
  sfdisk --json $out/as7760-sd.img > $out/partitions.json
  (cd $out; sha256sum as7760-sd.img > SHA256SUMS)
''

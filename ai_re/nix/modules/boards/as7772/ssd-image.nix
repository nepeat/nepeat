# The installed-image layout of ../as7760/sd-install.nix, written to the SSD.
# make-ext4-fs sizes the root with no spare inodes, and with a systemd initrd
# the first activation (creating /etc, /usr, ...) runs before the first-boot
# resize: it failed with "No space left on device" (0 free inodes, 2026-11-21).
# Grow the root partition in the image by 1 GiB so the first boot has room;
# the first-boot resize still grows it to the whole disk afterwards.
{ lib, ... }:
{
  sdImage.postBuildCommands = lib.mkAfter ''
    truncate -s +1G $img
    echo ", +" | sfdisk --no-reread -N 2 $img
    eval $(partx $img -o START,SECTORS --nr 2 --pairs)
    dd if=$img of=root.img bs=512 skip=$START count=$SECTORS status=none
    e2fsck -fy root.img
    resize2fs root.img
    [ "$(dumpe2fs -h root.img 2>/dev/null | sed -n 's/^Free inodes: *//p')" -gt 20000 ]
    dd if=root.img of=$img bs=512 seek=$START conv=notrunc status=none
    rm root.img
    sfdisk -l $img
  '';
}

# Disk layout for a machine whose root lives on an iSCSI LUN: one ESP and an
# XFS root, nothing else. Pairs with nixos/module/iscsi-ibft.nix, which
# attaches the LUN in stage 1 from iBFT.
#
# XFS rather than ZFS or btrfs on purpose: the volume is already a zvol on the
# storage host, so a second copy-on-write layer buys nothing and costs write
# amplification over the network.
{ lib, ... }:

{
  disko.devices.disk.iscsi0 = {
    # Whatever this points at gets repartitioned. Fine where the LUN is the
    # only SCSI device; a machine with local SCSI/SATA storage should override
    # it with the unambiguous by-path link for the session, e.g.
    #   /dev/disk/by-path/ip-10.0.0.1:3260-iscsi-iqn.…-lun-0
    # `lsblk -o NAME,TRAN` shows which one is TRAN=iscsi.
    device = lib.mkDefault "/dev/sda";
    type = "disk";
    content = {
      type = "gpt";
      partitions = {
        ESP = {
          size = "512M";
          type = "EF00";
          content = {
            type = "filesystem";
            format = "vfat";
            mountpoint = "/boot";
            extraArgs = [
              "-n"
              "BOOT"
            ];
            # _netdev everywhere: systemd must not order these before the
            # network is up, nor try to unmount them after it has gone.
            mountOptions = [
              "umask=0077"
              "_netdev"
            ];
          };
        };
        root = {
          size = "100%";
          content = {
            type = "filesystem";
            format = "xfs";
            mountpoint = "/";
            extraArgs = [
              "-L"
              "nixos"
            ];
            mountOptions = [ "_netdev" ];
          };
        };
      };
    };
  };
}

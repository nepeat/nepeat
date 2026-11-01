# Ubuntu's 2.4.4-1ubuntu4 backports, retained byte-for-byte from:
# https://git.launchpad.net/ubuntu/+source/zfs-linux/tree/debian/patches?h=import/2.4.4-1ubuntu4
# The inode probe must retain HAVE_IOPS_CREATE_IDMAP on 2.4.4. The kthread
# fix must use init's fs_struct, which follows switch_root after initrd.
{ lib, zfs }:

assert zfs.version == "2.4.4";
assert lib.versions.majorMinor zfs.kernel.version == "7.3";

zfs.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    ./zfs-linux-7.3/linux-7.3-iops-create-no-flag.patch
    ./zfs-linux-7.3/linux-7.3-bvec-iter-bi-offset.patch
    ./zfs-linux-7.3/linux-cpu-feature-enabled.patch
    ./zfs-linux-7.3/linux-7.3-kthread-init-fs.patch
  ];

  # Upstream's 2.4.4 META still stops at 7.2. Permit this explicitly patched
  # 7.3 module without claiming that upstream supports the combination.
  configureFlags = (old.configureFlags or [ ]) ++ [ "--enable-linux-experimental" ];
  meta = old.meta // { broken = false; };
})

{ pkgs }:

let
  kernelPackages = pkgs.linuxPackages_7_3;
  zfsModule = pkgs.callPackage ../package/zfs-linux-7.3.nix {
    zfs = kernelPackages.zfs_unstable;
  };
in
pkgs.testers.runNixOSTest {
  name = "zfs-linux-7-3";

  nodes.machine = {
    system.stateVersion = "26.11";
    networking.hostId = "deadbeef";
    boot = {
      inherit kernelPackages;
      supportedFilesystems = [ "zfs" ];
      # Exercise the kthread fix across initrd -> stage 2, even though the
      # VM's operating-system root itself uses the test framework's disk.
      initrd.kernelModules = [ "zfs" ];
      zfs = {
        package = pkgs.zfs_unstable;
        modulePackage = zfsModule;
        devNodes = "/dev";
        forceImportRoot = false;
      };
    };
    virtualisation = {
      emptyDiskImages = [ 1024 ];
      memorySize = 2048;
      cores = 2;
    };
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("uname -r | grep '^7[.]3[.]'", "test -d /sys/module/zfs")

    with subtest("pool, inode operations and verified snapshot replication"):
        machine.succeed(
            "zpool create -o cachefile=none -O mountpoint=/pool testpool /dev/vdb",
            "zfs create testpool/data",
            "dd if=/dev/urandom of=/pool/data/payload bs=1M count=32 conv=fsync",
            "mkdir /pool/data/dir && mv /pool/data/payload /pool/data/dir/payload",
            "ln /pool/data/dir/payload /pool/data/payload",
            "ln -s dir/payload /pool/data/link && cmp /pool/data/payload /pool/data/link",
            "zfs snapshot testpool/data@verified",
            "zfs send testpool/data@verified | zfs receive testpool/copy",
            "cmp /pool/data/payload /pool/copy/payload",
        )
        expected = machine.succeed("sha256sum /pool/data/payload").split()[0]

    with subtest("encrypted dataset unload and reload"):
        machine.succeed(
            "printf 'test-passphrase' > /root/zfs-test-key && chmod 600 /root/zfs-test-key",
            "zfs create -o encryption=aes-256-gcm -o keyformat=passphrase "
            "-o keylocation=file:///root/zfs-test-key testpool/encrypted",
            "cp /pool/data/payload /pool/encrypted/payload && sync",
            "zfs unmount testpool/encrypted && zfs unload-key testpool/encrypted",
            "zfs load-key testpool/encrypted && zfs mount testpool/encrypted",
            "cmp /pool/data/payload /pool/encrypted/payload",
            "zpool scrub testpool && zpool wait -t scrub testpool",
            "zpool status -x | grep -F 'all pools are healthy'",
            "zpool export testpool",
        )

    with subtest("import and data integrity after a cold boot"):
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.succeed(
            "zpool import -d /dev testpool",
            "zfs load-key testpool/encrypted && zfs mount testpool/encrypted",
            "cmp /pool/data/payload /pool/copy/payload",
            "cmp /pool/data/payload /pool/encrypted/payload",
            "zpool status -x | grep -F 'all pools are healthy'",
        )
        assert expected == machine.succeed("sha256sum /pool/data/payload").split()[0]
        machine.fail("dmesg | grep -E 'BUG:|Oops:|WARNING: CPU:|scheduling while atomic'")
  '';
}

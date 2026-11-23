# trial-boot's Linux half in a NixOS VM (x86_64, KVM): trial-boot setup/stage/
# cancel/status against a second disk standing in for the boot disk, and the
# verifier's pass and fail paths (fail = reboot) with stub watchdog/loader hooks.
#   import ./test-linux.nix { inherit pkgs; }
{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "trial-boot-linux";
  nodes.peer = { };
  nodes.box = { ... }: {
    imports = [ ./default.nix ];
    virtualisation.emptyDiskImages = [ 64 ];   # /dev/vdb: the "boot disk"
    system.trialBoot = {
      enable = true;
      flag = { device = "/dev/vdb"; lba = 2047; };
      loader = {
        confPartition = 2;
        check = "[ -e /var/lib/tb-loader-set ]";
        setup = "touch /var/lib/tb-loader-set";
      };
      watchdog = {
        arm = "echo \"arm $1\" >> /var/log/tb-wd.log";
        kick = "echo \"kick $1\" >> /var/log/tb-wd.log";
        disarm = "echo disarm >> /var/log/tb-wd.log";
      };
      verify = { interface = "eth1"; hosts = [ "peer" ]; gateway = false; window = 20; interval = 2; };
    };
  };
  testScript = { nodes, ... }: let verify = "${nodes.box.system.build.trialBoot.verify}/bin/trial-boot-verify"; in ''
    start_all()
    box.wait_for_unit("multi-user.target")
    peer.wait_for_unit("multi-user.target")
    sector = "dd if=/dev/vdb bs=512 skip=2047 count=1 status=none"
    box.succeed("printf 'label: dos\\nstart=2048, size=8192, type=83\\n' | sfdisk -q /dev/vdb")
    box.succeed("mkdir -p /boot")
    box.fail("trial-boot stage /run/current-system")          # not set up yet
    box.succeed("trial-boot setup | grep 'loader setting made'")
    box.succeed("cmp /boot/boot.scr ${nodes.box.system.build.trialBoot.bootScr}/boot.scr")
    box.succeed("trial-boot setup | grep 'is current'")
    box.succeed("trial-boot stage /run/current-system --append 'module_blacklist=foo'")
    box.succeed(f"{sector} | head -c 8 | grep -qx TRIALBT1")
    box.succeed("grep -q 'APPEND init=/nix/store/.*/init .*trialboot=[0-9a-f]* panic=10 module_blacklist=foo' /var/lib/trial-boot/extlinux.conf")
    box.succeed("grep -q '^DEFAULT trial' /var/lib/trial-boot/extlinux.conf")
    box.succeed("test -L /nix/var/nix/gcroots/trial-boot")
    box.fail("trial-boot stage /run/current-system")          # one pending at a time
    box.succeed("trial-boot status | grep 'request sector /dev/vdb:2047: pending'")
    box.succeed("trial-boot cancel | grep 'request cancelled'")
    box.succeed(f"test -z \"$({sector} | od -An -v -tx1 | tr -d ' 0\\n')\"")
    # a sector that is not ours is never touched
    box.succeed("printf FOREIGN | dd of=/dev/vdb bs=512 seek=2047 conv=notrunc status=none")
    box.fail("trial-boot stage /run/current-system")
    box.fail("trial-boot cancel")
    box.succeed(f"{sector} | head -c 7 | grep -qx FOREIGN")
    box.succeed("dd if=/dev/zero of=/dev/vdb bs=512 seek=2047 count=1 conv=notrunc status=none")
    box.fail("trial-boot promote")                            # not a trial boot

    # verifier, pass: eth1 up, peer answers -> kick, then disarm, verified, no reboot
    box.succeed("${verify}")
    box.succeed("grep -q verified /run/trial-boot/state")
    box.succeed("grep -q 'verified at' /var/lib/trial-boot/history")
    box.succeed("head -n1 /var/log/tb-wd.log | grep -qx 'kick 180' && tail -n1 /var/log/tb-wd.log | grep -qx disarm")

    # verifier, fail: no carrier on eth1 -> records why, arms the watchdog, reboots
    box.succeed("ip link set eth1 down")
    box.execute("systemd-run --unit tb-fail ${verify}")
    box.wait_for_shutdown()
    box.start()
    box.wait_for_unit("multi-user.target")
    box.succeed("grep -q 'FAILED: after 20 s: eth1 has no carrier; rebooting to the default' /var/lib/trial-boot/history")
    box.succeed("tail -n1 /var/log/tb-wd.log | grep -qx 'arm 120'")
    box.succeed("trial-boot status | grep 'this boot: not a trial'")
  '';
}

{ config, lib, pkgs, ... }:
# SD card as a near-read-only boot medium for the RAM-root system.
#
# U-Boot fatloads the kernel/initrd from the FAT partition; Linux then runs
# entirely from RAM (netboot-root.nix). The FAT is mounted read-only only to
# keep a stable SSH host key across boots. The single write ever made is the
# first-boot key generation, done inside a brief rw remount. Booting over TFTP
# uses the same key whenever the card is present.
let
  keyDir = "/boot/persist";
  key = "${keyDir}/ssh_host_ed25519_key";
in {
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/NIXOS_BOOT";
    fsType = "vfat";
    options = [ "ro" "nofail" "noatime" "fmask=0077" "dmask=0077" "x-systemd.device-timeout=15s" ];
  };

  services.openssh.hostKeys = [ { path = key; type = "ed25519"; } ];
  systemd.services.as7760-persist-hostkey = {
    description = "Create the persistent SSH host key on the SD boot partition (once)";
    wantedBy = [ "multi-user.target" ];
    before = [ "sshd.service" "sshd-keygen.service" ];
    requiredBy = [ "sshd.service" ];
    after = [ "boot.mount" ];
    unitConfig.RequiresMountsFor = [ "/boot" ];
    path = [ pkgs.util-linux pkgs.openssh pkgs.coreutils ];
    serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
    script = ''
      [ -s ${key} ] && exit 0
      if mountpoint -q /boot; then
        mount -o remount,rw /boot
        trap 'sync; mount -o remount,ro /boot' EXIT
      fi
      mkdir -p ${keyDir}
      ssh-keygen -q -t ed25519 -N "" -C root@as7760 -f ${key}
    '';
  };

  # Fixed identity instead of a fresh transient ID on every RAM boot (the
  # initrd activation writes it before switch-root; no kernel param needed,
  # which also keeps bootargs under old U-Boot's console line limit).
  environment.etc.machine-id = { text = "4868c5965ac99c54e77e6ad9853de64a\n"; mode = "0444"; };

  # Keep the card idle: logs in RAM (1 GiB cap), no trim/swap/store writes.
  services.journald.settings.Journal = {
    Storage = lib.mkForce "volatile";
    RuntimeMaxUse = lib.mkForce "1G";
    RuntimeKeepFree = "2G";
  };
  services.fstrim.enable = false;
}

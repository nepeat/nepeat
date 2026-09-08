# Root filesystem on an iSCSI LUN, with every parameter taken from iBFT.
#
# The bootloader (iPXE) attaches the LUN and publishes an iSCSI Boot Firmware
# Table into ACPI. Stage 1 reads that table -- initiator IQN, target, portal and
# IP configuration -- instead of baking any of it into the image, so one closure
# boots on whichever host the boot manager has assigned a volume to.
#
# Deliberately NOT nixpkgs' `boot.iscsi-initiator`: that module requires an
# explicit `discoverPortal` and `target` at build time, which is precisely what
# makes an image host-specific. It is the right choice for a machine pinned to
# one target; it cannot express "whatever the firmware says".
#
# It also asserts `!boot.initrd.systemd.enable`, which would force the scripted
# initrd -- deprecated, and scheduled for removal in 26.11. That assertion is
# about *that module's* implementation (it drives iscsid from preLVMCommands),
# not a kernel or systemd limitation: a stage-1 unit running `iscsistart` works
# fine, and is what this module does. So we get iBFT support and a supported
# initrd at the same time.
#
# This module only attaches the LUN. Where root lives on it is the disk layout
# module's job -- see nixos/module/iscsi-xfs.nix.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.iscsiRoot;
in

{
  options.hardware.iscsiRoot = {
    enable = lib.mkEnableOption "root filesystem on an iSCSI LUN described by iBFT";

    loginRetries = lib.mkOption {
      type = lib.types.ints.positive;
      default = 10;
      description = ''
        How many times stage 1 retries the iBFT login before giving up. The
        target is not always ready to accept a new session immediately: the
        bootloader has just released its own, and LIO can still be tearing that
        down when the kernel gets here. The loop also covers a NIC udev has not
        finished with yet, since `iscsistart -N` simply fails until it exists.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = config.boot.initrd.systemd.enable;
        message = ''
          hardware.iscsiRoot drives iscsistart from a stage-1 systemd unit and
          needs boot.initrd.systemd.enable = true. The scripted initrd is
          deprecated upstream; do not switch back to it.
        '';
      }
    ];

    boot.initrd.systemd.enable = true;

    # Same reasoning as nixpkgs' boot.iscsi-initiator: the root device is on the
    # network, so the interface must not go down between stage 1 and stage 2.
    # Scripted networking does not order interface teardown against unmounting;
    # networkd does.
    networking.useNetworkd = true;
    networking.useDHCP = false;

    boot.initrd.kernelModules = [
      "iscsi_tcp"
      # Exposes /sys/firmware/ibft, which is where all of this comes from.
      "iscsi_ibft"
    ];

    boot.initrd.availableKernelModules = [
      "sd_mod"
      "virtio_net"
      "virtio_pci"
      "e1000e"
      "igb"
      "igc"
      "r8169"
    ];

    # coreutils is already in the stage-1 /bin, so the unit below can use
    # cat/mkdir/sleep; only iscsistart has to be added.
    boot.initrd.systemd.extraBin.iscsistart = "${pkgs.openiscsi}/bin/iscsistart";
    boot.initrd.systemd.storePaths = [ "${pkgs.openiscsi}/bin/iscsistart" ];

    boot.initrd.systemd.services.iscsi-ibft = {
      description = "Attach the iSCSI root LUN described by iBFT";

      # initrd-root-device.target is systemd's anchor for "the root device is
      # now available". This unit is what makes it available, so it belongs
      # before that target; the sysroot mount then waits on it for free.
      wantedBy = [ "initrd-root-device.target" ];
      before = [ "initrd-root-device.target" ];
      after = [
        "systemd-modules-load.service"
        "systemd-udevd.service"
      ];
      unitConfig.DefaultDependencies = false;

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };

      script = ''
        if [ ! -e /sys/firmware/ibft/initiator/initiator-name ]; then
          echo "iscsi-ibft: no iBFT present -- the bootloader did not attach a SAN device."
          echo "iscsi-ibft: nothing to do; the root device will have to come from elsewhere."
          exit 0
        fi

        mkdir -p /etc/iscsi /run/lock/iscsi
        ibft_name=$(cat /sys/firmware/ibft/initiator/initiator-name)
        # iscsistart still expects the initiator name in the usual place even
        # though it is about to read everything else from firmware. It must
        # match what the target's ACL was created for, or login is refused.
        echo "InitiatorName=$ibft_name" > /etc/iscsi/initiatorname.iscsi
        echo "iscsi-ibft: initiator $ibft_name"

        attempt=1
        while [ $attempt -le ${toString cfg.loginRetries} ]; do
          # -N brings the NIC up exactly as the bootloader recorded it, so we
          # inherit iPXE's addressing rather than racing a second DHCP lease
          # against the one the firmware already took. It fails harmlessly if
          # udev has not finished with the NIC yet; the retry covers that.
          iscsistart -N \
            || echo "iscsi-ibft: no usable iBFT network config yet (attempt $attempt)"

          if iscsistart -b; then
            echo "iscsi-ibft: logged in on attempt $attempt"
            exit 0
          fi

          echo "iscsi-ibft: login attempt $attempt failed, retrying"
          attempt=$((attempt + 1))
          sleep 1
        done

        echo "iscsi-ibft: giving up after ${toString cfg.loginRetries} attempts" >&2
        exit 1
      '';
    };

    # KeepConfiguration=yes is the load-bearing part: without it networkd drops
    # the address stage 1 configured while it works out what to do, and the root
    # filesystem disappears mid-boot.
    #
    # Known wart: this leaves the machine with TWO addresses on the same subnet
    # -- the iBFT one that the iSCSI session actually uses, plus a DHCP lease
    # networkd takes afterwards (verified on the UM350: 10.36.75.107 from iBFT,
    # .109 from DHCP at metric 1024). Harmless, since the iBFT address is
    # primary and the session is bound to it, but untidy. Set DHCP = "no" to
    # rely purely on the firmware's addressing -- at the cost of having no
    # fallback if a machine's iBFT carries no network config.
    systemd.network.enable = true;
    systemd.network.networks."10-iscsi-uplink" = lib.mkDefault {
      matchConfig.Type = "ether";
      networkConfig = {
        DHCP = "yes";
        KeepConfiguration = "yes";
      };
      linkConfig.RequiredForOnline = "routable";
    };

    environment.systemPackages = with pkgs; [
      xfsprogs
      openiscsi
    ];
  };
}

# Broadcom OpenBCM SDK on a switch board: BDE + KNET kernel modules loaded at
# boot, their /dev nodes, one-SDK-process-per-unit locking for bcm.user, and
# the ASIC die temperature reader (openbcm-asic-temp, attach only) that fan
# control and monitoring use. Chip support is whatever Make.local selects in
# packages/openbcm (Tomahawk on the AS7760). Needs the SDK packages allowed as
# unfree: openbcm-bcm-user, openbcm-libsdk, sai-openbcm.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.openbcm;
  openbcm = import ../../packages/openbcm { inherit pkgs; };
  sonic = import ../../packages/sonic { inherit pkgs; };
  kmods = openbcm.kmodsFor config.boot.kernelPackages.kernel;
  sai = pkgs.callPackage ../../packages/sai-openbcm { inherit sonic; libsdk = openbcm.libsdk { }; };
  bcmUser = openbcm.bcm-user { };
  lock = "/run/openbcm-unit0.lock";
  # Same lock as sdk_glue.c, held for the life of the shell.
  bcmUserLocked = pkgs.writeShellScriptBin "bcm.user" ''
    exec 9>>${lock}
    if ! ${pkgs.util-linux}/bin/flock -n 9; then
      echo "bcm.user: another SDK process owns the switch (${lock}); stop it first, e.g. systemctl stop openbcm-asic-temp" >&2
      exit 3
    fi
    exec ${bcmUser}/bin/bcm.user "$@"
  '';
in {
  options.hardware.openbcm = {
    enable = lib.mkEnableOption "the Broadcom OpenBCM SDK kernel modules (BDE, KNET) and tools";
    kernelBdeOptions = lib.mkOption {
      type = lib.types.str;
      default = "himem=0 maxpayload=128 dmasize=16M";
      description = "linux-kernel-bde module options (the AS7760 factory values).";
    };
    # PHYs in front of the ASIC that the SDK has no driver for (retimers,
    # MACsec PHYs with a vendor-only library): a recorded MDIO script is run
    # through the chip's MDIO master once per boot, before anything else
    # uses the SDK. Format and tool: packages/sai-openbcm (mdio_script.h).
    mdioScript = {
      enable = lib.mkEnableOption "running MDIO scripts for external PHYs at boot (openbcm-mdio-script.service)";
      files = lib.mkOption {
        type = lib.types.listOf (lib.types.strMatching "/.*");
        example = [ "/var/lib/evora-phy/init.mdio" ];
        description = ''
          Scripts, run in order. Absolute paths on the target, read at run
          time: private data (vendor firmware words) that is installed by
          hand and never enters the Nix store. A missing or malformed file
          fails the unit before any PHY is written.
        '';
      };
      sdkConfig = lib.mkOption {
        type = lib.types.path;
        description = "SDK config.bcm for init soc + init misc (it sets the external MDIO clock and hold delay).";
      };
    };
    temperature = {
      enable = lib.mkEnableOption "the ASIC die temperature reader (openbcm-asic-temp)";
      interval = lib.mkOption { type = lib.types.ints.between 1 60; default = 5; description = "Seconds between reads."; };
      file = lib.mkOption {
        type = lib.types.str;
        default = "/run/asic/temperature";
        readOnly = true;
        description = "Hottest current monitor in C; removed while there is no reading.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    boot.extraModulePackages = [ kmods ];
    boot.kernelModules = [ "linux-kernel-bde" "linux-user-bde" "linux-bcm-knet" ];
    boot.extraModprobeConfig = "options linux_kernel_bde ${cfg.kernelBdeOptions}";
    environment.systemPackages = [ bcmUserLocked sai ];

    # The modules register fixed majors but create no device nodes.
    systemd.services.openbcm-devices = {
      description = "Broadcom SDK device nodes";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-modules-load.service" ];
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      path = [ pkgs.coreutils pkgs.gawk ];
      script = ''
        for d in linux-kernel-bde linux-user-bde linux-bcm-knet; do
          major=$(awk -v d=$d '$2 == d { print $1 }' /proc/devices)
          if [ -z "$major" ]; then echo "$d: module not loaded" >&2; exit 1; fi
          [ -c /dev/$d ] || mknod -m 0600 /dev/$d c "$major" 0
        done
      '';
    };

    # Once per boot (RemainAfterExit; not re-run by nixos-rebuild or a SONiC
    # restart: the PHYs keep their state while the switch chip is re-inited).
    # It holds the SDK lock while it runs, so the temperature reader and syncd
    # are ordered after it. They do not require it: a failed PHY bring-up
    # leaves the ports down and this unit failed, not the fans without the
    # ASIC temperature. Re-run by hand: systemctl restart openbcm-mdio-script
    # with syncd / openbcm-asic-temp stopped.
    systemd.services.openbcm-mdio-script = lib.mkIf cfg.mdioScript.enable {
      description = "External PHY bring-up over the switch chip's MDIO bus";
      wantedBy = [ "multi-user.target" ];
      requires = [ "openbcm-devices.service" ];
      after = [ "openbcm-devices.service" ];
      before = [ "openbcm-asic-temp.service" "sonic-syncd.service" ];
      restartIfChanged = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${sai}/bin/openbcm-mdio-script --config ${cfg.mdioScript.sdkConfig} ${lib.escapeShellArgs cfg.mdioScript.files}";
        TimeoutStartSec = "15min";
        NoNewPrivileges = true;
        ProtectHome = true;
        PrivateTmp = true;
      };
    };

    systemd.services.openbcm-asic-temp = lib.mkIf cfg.temperature.enable {
      description = "Switch ASIC die temperature reader (Broadcom SDK, attach only)";
      wantedBy = [ "multi-user.target" ];
      requires = [ "openbcm-devices.service" ];
      after = [ "openbcm-devices.service" ];
      # fand waits for the first reading instead of starting in failsafe.
      before = lib.optional (config.services.fand.enable or false) "fand.service";
      serviceConfig = {
        # "started" = the first reading is on disk (or 30 s passed).
        ExecStartPost = "${pkgs.bash}/bin/bash -c 'for i in $(seq 60); do [ -s ${cfg.temperature.file} ] && exit 0; sleep 0.5; done; echo no reading after 30 s >&2'";
        ExecStart = lib.concatStringsSep " " [
          "${sai}/bin/openbcm-asic-temp --interval ${toString cfg.temperature.interval}"
          "--out ${cfg.temperature.file}"
          (lib.optionalString (config.monitoring.nodeExporter.enable or false) "--prom /run/prometheus-textfile/asic.prom")
        ];
        RuntimeDirectory = "asic";
        Restart = "always";
        RestartSec = "5s";
        # Exit 5 = it powered the ASIC core down (--shutdown-at). Any SDK access
        # after that can crash the host (2026-11-15: the box panicked and
        # rebooted right after the test), so stay down: no reading means fand
        # holds 100% until a power cycle.
        RestartPreventExitStatus = "5";
        # The SDK maps the BDE devices and its DMA pool; it runs as root.
        NoNewPrivileges = true;
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
  };
}

# VRFs for services.sonic: SONiC's vrfmgrd (CONFIG_DB VRF -> Linux VRF
# devices + APPL_DB VRF_TABLE, which orchagent's VRFOrch turns into SAI
# virtual routers) and the kernel's vrf module. intfmgrd (default.nix) then
# enslaves bound interfaces ("config interface vrf bind ...") and intfsorch
# creates their router interfaces in that virtual router. Kernel:
# NET_VRF (+ IP_MULTIPLE_TABLES, IPV6_MULTIPLE_TABLES, NET_L3_MASTER_DEV).
# At start vrfmgrd deletes VRF devices it did not create and moves the
# "local" ip rule from pref 0 to 1001 (after the l3mdev rule), as on SONiC.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.sonic;
  unit = n: "sonic-${n}.service";
in {
  options.services.sonic.vrf.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Run vrfmgrd and load the kernel's vrf module (SONiC VRFs: config vrf add, config interface vrf bind).";
  };

  config = lib.mkIf (cfg.enable && cfg.vrf.enable) {
    boot.kernelModules = [ "vrf" ];
    # Like the other managers: stops with orchagent, comes back with it.
    systemd.services.sonic-orchagent.upholds = [ (unit "vrfmgrd") ];
    systemd.services.sonic-vrfmgrd = {
      description = "SONiC vrfmgrd (CONFIG_DB VRF -> kernel VRF devices, APPL_DB VRF_TABLE)";
      wantedBy = [ "sonic.target" ];
      partOf = [ "sonic.target" ];
      bindsTo = [ (unit "orchagent") ];
      after = [ (unit "orchagent") "sonic-config-loader.service" ];
      requires = [ "sonic-config-loader.service" ];
      # ip is a store path in the binary; its shell pipelines need grep.
      path = with pkgs; [ iproute2 coreutils gnugrep ];
      environment = { platform = cfg.asicType; ASIC_VENDOR = cfg.asicType; };
      unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
      serviceConfig = {
        ExecStart = "${cfg.packages.swss}/bin/vrfmgrd";
        Restart = "always";
        RestartSec = 3;
      };
    };
  };
}

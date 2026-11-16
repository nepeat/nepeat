# sFlow for services.sonic (docker-sflow minus the container): sflowmgrd
# (CONFIG_DB SFLOW / SFLOW_SESSION -> APPL_DB, which sfloworch turns into SAI
# SAMPLEPACKET objects on the ports), hsflowd (mod_sonic: collectors, agent and
# polling from CONFIG_DB; mod_psample: the samples from the kernel's psample
# group 1, which the SAI's GENETLINK host interface "psample" feeds) and
# port_index_mapper (STATE_DB PORT_INDEX_TABLE). CONFIG_DB FEATURE|sflow is
# seeded enabled: coppmgrd installs the SAMPLEPACKET trap (copp_cfg.json
# "sflow", group queue2_group1 with genetlink_name psample) only for an
# enabled feature. sflowmgrd starts / stops hsflowd with "service hsflowd
# restart|stop" (SONiC's init script); a "service" shim on its PATH maps that
# to sonic-hsflowd.service. The SAI / kernel side (genetlink hostif ->
# psample) is the platform's: on the CS8210 sai-teralynx tl_sflow + ipd psample=1.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkEnableOption mkIf mkDefault types concatStringsSep;
  cfg = config.services.sonic;
  scfg = cfg.sflow;
  pk = cfg.packages.sflow;
  service = pkgs.writeShellScriptBin "service" ''
    # sflowmgrd: "service hsflowd restart|stop"
    case "$1 $2" in
      "hsflowd restart") exec systemctl restart sonic-hsflowd.service ;;
      "hsflowd stop") exec systemctl stop sonic-hsflowd.service ;;
      "hsflowd start") exec systemctl start sonic-hsflowd.service ;;
    esac
    echo "service: only hsflowd start|stop|restart here" >&2
    exit 1
  '';
  # SONiC's hsflowd.conf, plus: no sysmonitor here sets STATE_DB
  # SYSTEM_READY|SYSTEM_STATE, which mod_sonic otherwise waits for (300 s).
  defaultConf = pkgs.writeText "hsflowd.conf" ''
    sflow {
      sonic { waitReady=10 }
      refreshAdaptors=60
    }
  '';
  unitBase = {
    partOf = [ "sonic.target" ];
    unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
  };
in {
  options.services.sonic.sflow = {
    enable = mkEnableOption "sFlow (sflowmgrd, hsflowd with mod_sonic + mod_psample, port_index_mapper)";
    hsflowdConfig = mkOption {
      type = types.path;
      default = defaultConf;
      defaultText = lib.literalExpression "SONiC's hsflowd.conf with sonic { waitReady=10 }";
      description = "hsflowd.conf (collectors, agent, sampling and polling come from CONFIG_DB).";
    };
    hsflowdArgs = mkOption { type = types.listOf types.str; default = [ ]; example = [ "-dd" ]; description = "More hsflowd arguments (debug: -dd ...)."; };
  };

  config = mkIf (cfg.enable && scfg.enable) {
    environment.systemPackages = [ pk.sflowtool ];
    services.sonic.settings.FEATURE.sflow = lib.mapAttrs (_: mkDefault) {
      state = "enabled";
      has_global_scope = "True";
      has_per_asic_scope = "False";
      auto_restart = "enabled";
    };
    services.sonic.tableModes.FEATURE = mkDefault "seed";

    systemd.services = {
      # orchagent restarts it with the other managers (Upholds)
      sonic-orchagent.upholds = [ "sonic-sflowmgrd.service" ];
      sonic-sflowmgrd = unitBase // {
        description = "SONiC sflowmgrd (CONFIG_DB SFLOW -> APPL_DB SFLOW_TABLE / SFLOW_SESSION_TABLE)";
        wantedBy = [ "sonic.target" ];
        bindsTo = [ "sonic-orchagent.service" ];
        after = [ "sonic-orchagent.service" "sonic-config-loader.service" ];
        requires = [ "sonic-config-loader.service" ];
        path = [ service pkgs.systemd ];
        serviceConfig = { ExecStart = "${cfg.packages.swss}/bin/sflowmgrd"; Restart = "always"; RestartSec = 3; };
      };
      # started and stopped by sflowmgrd (SFLOW|global admin_state)
      sonic-hsflowd = unitBase // {
        description = "hsflowd (sFlow agent: psample samples + counters to the CONFIG_DB collectors)";
        requires = [ "redis-sonic.service" ];
        after = [ "redis-sonic.service" "sonic-config-loader.service" ];
        serviceConfig = {
          ExecStart = concatStringsSep " " ([ "${pk.hsflowd}/bin/hsflowd" "-d" "-P" "-f" (toString scfg.hsflowdConfig)
            "-l" "${pk.hsflowd}/lib/hsflowd/modules" "-p" "/run/hsflowd/hsflowd.pid" "-o" "/run/hsflowd/hsflowd.auto" ] ++ scfg.hsflowdArgs);
          RuntimeDirectory = "hsflowd";
          Restart = "on-failure";
          RestartSec = 3;
        };
      };
      sonic-port-index-mapper = unitBase // {
        description = "SONiC port_index_mapper (STATE_DB PORT_INDEX_TABLE for hsflowd)";
        wantedBy = [ "sonic.target" ];
        requires = [ "redis-sonic.service" ];
        after = [ "redis-sonic.service" "sonic-config-loader.service" ];
        # it does not exit on SIGTERM (stop timed out on the CS8210)
        serviceConfig = { ExecStart = "${pk.port-index-mapper}/bin/port_index_mapper"; Restart = "always"; RestartSec = 3; TimeoutStopSec = 5; };
      };
    };
  };
}

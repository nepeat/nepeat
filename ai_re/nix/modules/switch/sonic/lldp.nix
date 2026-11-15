# LLDP for services.sonic: lldpd on the front-panel netdevs, SONiC's lldpmgrd
# (port ID = front-panel alias, description from CONFIG_DB PORT, hostname)
# and lldp_syncd (neighbors -> APPL_DB LLDP_ENTRY_TABLE), "show lldp
# neighbors|table". As docker-lldp, minus the container: lldpd's config is
# rendered from CONFIG_DB by sonic-cfggen at start (its "pause" line dropped:
# lldpd runs from the start, lldpmgrd's later "lldpcli resume" is a no-op).
# CONFIG_DB FEATURE|lldp is seeded enabled: coppmgrd installs the LLDP trap
# only for an enabled feature, and lldpshow looks for it.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkEnableOption mkIf mkDefault types concatStringsSep optionalString;
  cfg = config.services.sonic;
  lcfg = cfg.lldp;
  plat = cfg.packages.platform;
  lldpd = plat.lldpd;
  spy = cfg.packages.python;
  configure = pkgs.writeShellScript "sonic-lldpd-configure" ''
    set -eu
    for _ in $(seq 60); do [ -S /run/lldpd.socket ] && break; sleep 0.5; done
    ${spy.config-engine}/bin/sonic-cfggen -d -y /etc/sonic/sonic_version.yml \
      -t ${plat.lldpTemplates}/lldpd.conf.j2 -t ${plat.lldpTemplates}/lldpdSysDescr.conf.j2 \
      | grep -v '^pause$' > /run/sonic-lldp/lldpd.conf
    exec ${lldpd}/bin/lldpcli -c /run/sonic-lldp/lldpd.conf
  '';
  unitBase = {
    wantedBy = [ "sonic.target" ];
    partOf = [ "sonic.target" ];
    unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
  };
in {
  options.services.sonic.lldp = {
    enable = mkEnableOption "LLDP (lldpd, lldpmgrd, lldp_syncd) and show lldp";
    interfaces = mkOption {
      type = types.listOf types.str;
      default = [ "Ethernet[0-9]*" ];
      example = [ "Ethernet[0-9]*" "eth0" ];
      description = "lldpd -I: interfaces (patterns) lldpd runs on.";
    };
    chassisIdInterface = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "eth0";
      description = "lldpd -C: interface whose MAC is the chassis ID (SONiC: eth0). null: lldpd's choice (a front-panel netdev, i.e. the switch MAC).";
    };
    extraArgs = mkOption { type = types.listOf types.str; default = [ ]; description = "More lldpd arguments."; };
  };

  config = mkIf (cfg.enable && lcfg.enable) {
    users.users._lldpd = { description = "lldpd privilege separation"; group = "_lldpd"; home = "/run/lldpd"; isSystemUser = true; };
    users.groups._lldpd = { };
    environment.systemPackages = [ lldpd plat.lldpshow ];

    services.sonic.settings.FEATURE.lldp = lib.mapAttrs (_: mkDefault) {
      state = "enabled";
      has_global_scope = "True";
      has_per_asic_scope = "False";
      auto_restart = "enabled";
    };
    services.sonic.tableModes.FEATURE = mkDefault "seed";

    systemd.services = {
      sonic-lldpd = unitBase // {
        description = "SONiC LLDP daemon (lldpd)";
        requires = [ "redis-sonic.service" "sonic-config-loader.service" ];
        after = [ "redis-sonic.service" "sonic-config-loader.service" "network.target" ];
        serviceConfig = {
          ExecStart = concatStringsSep " " ([ "${lldpd}/bin/lldpd" "-d" "-I" (concatStringsSep "," lcfg.interfaces) ]
            ++ lib.optionals (lcfg.chassisIdInterface != null) [ "-C" lcfg.chassisIdInterface ] ++ lcfg.extraArgs);
          # Best effort: lldpd runs with its defaults if CONFIG_DB cannot be rendered.
          ExecStartPost = "-${configure}";
          RuntimeDirectory = "sonic-lldp";
          Restart = "always";
          RestartSec = 3;
        };
      };
      sonic-lldpmgrd = unitBase // {
        description = "SONiC lldpmgrd (per-port LLDP settings from CONFIG/APPL_DB)";
        bindsTo = [ "sonic-lldpd.service" ];
        after = [ "sonic-lldpd.service" ];
        path = [ lldpd ];
        environment.PLATFORM = cfg.platform;
        serviceConfig = { ExecStart = "${plat.lldpmgrd}/bin/lldpmgrd"; Restart = "always"; RestartSec = 3; };
      };
      sonic-lldp-syncd = unitBase // {
        description = "SONiC lldp_syncd (LLDP neighbors -> APPL_DB)";
        bindsTo = [ "sonic-lldpd.service" ];
        after = [ "sonic-lldpd.service" ];
        environment.PLATFORM = cfg.platform;
        serviceConfig = { ExecStart = "${plat.lldp-syncd}/bin/lldp_syncd"; Restart = "always"; RestartSec = 3; };
      };
    };
  };
}

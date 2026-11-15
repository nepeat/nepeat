# SONiC's routing stack (the bgp container) as NixOS services, part of
# services.sonic: FRR's zebra/mgmtd/bgpd/staticd, zebra's FPM to SONiC's
# fpmsyncd (routes -> APPL_DB ROUTE_TABLE -> orchagent -> ASIC), bgpcfgd
# (CONFIG_DB BGP_NEIGHBOR/STATIC_ROUTE/... -> FRR via vtysh) and bgpmon, in
# SONiC's default "separated" mode: per-daemon configs rendered from
# CONFIG_DB at every start, then bgpcfgd applies changes live. FRR config is
# never a Nix or hand-edited file (config-ownership.md: BGP is unmanaged
# unless declared in services.sonic.settings). vtysh is on PATH, so the CLI's
# "config interface ip remove" and "show ip route" work.
# Packages: ../../../packages/sonic/routing. README.md next to this file.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkIf types optional optionals concatStringsSep listToAttrs nameValuePair;
  cfg = config.services.sonic;
  fcfg = cfg.frr;
  routing = cfg.packages.routing;
  spy = cfg.packages.python;
  frr = fcfg.package;
  tdir = routing.templateDir;
  unit = n: "sonic-${n}.service";
  orch = unit "orchagent";

  # As docker-fpm-frr's docker_init.sh ("separated", bgpcfgd mode): render the
  # daemon configs from CONFIG_DB + constants.yml. Other modes are not built.
  renderConfig = pkgs.writeShellScript "sonic-frr-config" ''
    set -euo pipefail
    get() { sonic-cfggen -d -v "DEVICE_METADATA.localhost.$1" 2>/dev/null || true; }
    mode=$(get docker_routing_config_mode); mgmt=$(get frr_mgmt_framework_config)
    if [ -n "$mode" ] && [ "$mode" != separated ]; then
      echo "docker_routing_config_mode=$mode is not supported here; using separated (bgpcfgd)" >&2
    fi
    if [ "$mgmt" = true ]; then
      echo "frr_mgmt_framework_config=true (frrcfgd) is not supported here; using bgpcfgd" >&2
    fi
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    sonic-cfggen -d -y /etc/sonic/constants.yml -T ${tdir} \
      -t ${tdir}/bgpd/gen_bgpd.conf.j2,"$tmp/bgpd.conf" \
      -t ${tdir}/zebra/zebra.conf.j2,"$tmp/zebra.conf" \
      -t ${tdir}/staticd/gen_staticd.conf.j2,"$tmp/staticd.conf"
    for f in bgpd zebra staticd; do
      install -o frr -g frr -m 0640 "$tmp/$f.conf" /etc/frr/$f.conf
    done
    rm -f /etc/frr/frr.conf
    echo "FRR config rendered from CONFIG_DB (separated mode)"
  '';
  # zsocket.sh: zebra answers on its vty before bgpd/staticd start.
  waitDaemon = d: pkgs.writeShellScript "sonic-frr-wait-${d}" ''
    for i in $(seq 100); do
      ${frr}/bin/vtysh -d ${d} -c "show version" >/dev/null 2>&1 && exit 0
      sleep 0.1
    done
    echo "${d} is not answering on its vty socket" >&2
  '';

  # Like the swss daemons: BindsTo orchagent, upheld by it (SONiC restarts the
  # bgp container with swss); plus BindsTo the FRR daemon each one needs.
  svc = { description, exec, needs ? [ ], after ? [ ], extra ? { } }: lib.recursiveUpdate {
    inherit description;
    wantedBy = [ "sonic.target" ];
    partOf = [ "sonic.target" ];
    bindsTo = [ orch ] ++ needs;
    after = [ orch ] ++ needs ++ after;
    path = [ frr pkgs.iproute2 pkgs.coreutils ];
    unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
    serviceConfig = { ExecStart = exec; Restart = "always"; RestartSec = 3; };
  } extra;
  frrd = d: args: needs: svc {
    description = "SONiC FRR ${d}";
    exec = concatStringsSep " " ([ "${frr}/libexec/frr/${d}" ] ++ args);
    inherit needs;
    extra.serviceConfig.ExecStartPost = [ "${waitDaemon d}" ];
  };
  units = {
    frr-config = svc {
      description = "SONiC FRR config from CONFIG_DB";
      exec = "${renderConfig}";
      extra = {
        path = [ spy.config-engine ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; Restart = "on-failure"; };
      };
    };
    frr-mgmtd = frrd "mgmtd" [ "-P" "0" ] [ (unit "frr-config") ];
    frr-zebra = frrd "zebra" fcfg.zebraArgs [ (unit "frr-mgmtd") ];
    frr-staticd = frrd "staticd" [ "-P" "0" ] [ (unit "frr-zebra") ];
    frr-bgpd = frrd "bgpd" [ "-P" "0" ] [ (unit "frr-zebra") ];
    # FPM server (TCP 127.0.0.1:2620); zebra's dplane_fpm_nl connects and retries.
    fpmsyncd = svc {
      description = "SONiC fpmsyncd (zebra FPM -> APPL_DB ROUTE_TABLE)";
      exec = "${cfg.packages.swss}/bin/fpmsyncd";
      after = [ (unit "frr-bgpd") ];
      extra.environment = { platform = cfg.asicType; };
    };
    bgpcfgd = svc {
      description = "SONiC bgpcfgd (CONFIG_DB -> FRR)";
      exec = "${routing.bgpcfgd}/bin/bgpcfgd";
      needs = [ (unit "frr-bgpd") (unit "frr-staticd") ];
    };
  } // lib.optionalAttrs fcfg.bgpmon {
    bgpmon = svc {
      description = "SONiC bgpmon (BGP neighbor state -> STATE_DB)";
      exec = "${routing.bgpcfgd}/bin/bgpmon";
      needs = [ (unit "frr-bgpd") ];
    };
  };
in {
  options.services.sonic.frr = {
    enable = mkOption {
      type = types.bool;
      default = true;
      description = ''
        SONiC's routing stack (the bgp container): FRR zebra/mgmtd/bgpd/staticd,
        zebra's FPM to fpmsyncd, bgpcfgd (CONFIG_DB -> FRR, SONiC's default
        "separated" mode) and vtysh on PATH. On by default, as SONiC's bgp feature.
      '';
    };
    package = mkOption {
      type = types.package;
      default = routing.frr;
      defaultText = lib.literalExpression "sonic.routing.frr";
      description = "FRR with SONiC's FPM patches (bin/vtysh, libexec/frr/<daemon>, dplane_fpm_nl).";
    };
    zebraArgs = mkOption {
      type = types.listOf types.str;
      default = [ "-P" "0" "-s" "90000000" "-M" "dplane_fpm_nl" "--asic-offload=notify_on_offload" ];
      description = "zebra arguments (SONiC's, with FRR's dplane_fpm_nl in place of dplane_fpm_sonic).";
    };
    bgpmon = mkOption { type = types.bool; default = true; description = "Run bgpmon (BGP neighbor state into STATE_DB NEIGH_STATE_TABLE)."; };
  };

  config = mkIf (cfg.enable && fcfg.enable) {
    users.users.frr = { isSystemUser = true; group = "frr"; extraGroups = [ "frrvty" ]; description = "FRR daemons"; };
    users.groups.frr = { };
    users.groups.frrvty = { };

    environment.etc = {
      # Separated mode (docker_init.sh); the daemon files are rendered at start.
      "frr/vtysh.conf".text = "no service integrated-vtysh-config\n";
      "sonic/constants.yml".source = lib.mkDefault routing.constants;
    };
    systemd.tmpfiles.rules = [
      "d /etc/frr 0755 frr frr -"
      "d /run/frr 0755 frr frr -"
      "d /var/lib/frr 0750 frr frr -"
    ];
    environment.systemPackages = [ frr ];

    # bgpcfgd configures no neighbor until these exist (BGPPeerMgrBase deps;
    # SONiC has them from init_cfg.json / minigraph). Seeded: config + config
    # save win. Loopback0 (also a dependency) is the user's (config loopback add).
    services.sonic.settings = {
      BGP_DEVICE_GLOBAL.STATE = lib.mapAttrs (_: lib.mkDefault) {
        tsa_enabled = "false"; wcmp_enabled = "false"; idf_isolation_state = "unisolated";
      };
      DEVICE_METADATA.localhost.type = lib.mkDefault "LeafRouter";
    };
    services.sonic.keyModes.BGP_DEVICE_GLOBAL.STATE = lib.mkDefault "seed";
    services.sonic.fieldModes.DEVICE_METADATA.localhost.type = lib.mkDefault "seed";

    systemd.services = listToAttrs (lib.mapAttrsToList (n: v: nameValuePair "sonic-${n}" v) units) // {
      sonic-orchagent.upholds = map unit (lib.attrNames units);
    };
  };
}

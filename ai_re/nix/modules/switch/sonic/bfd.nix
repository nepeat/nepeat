# Software BFD as part of services.sonic (the Tomahawk's BFD offload needs
# closed microcontroller firmware; sai-openbcm has no SAI BFD): FRR bfdd,
# BGP neighbors with BFD (BGP_NEIGHBOR bfd=true), static routes with BFD
# (STATIC_ROUTE bfd=true: staticroutebfd -> APPL_DB BFD_SESSION_TABLE ->
# orchagent's BfdOrch, which finds no offload and hands the session to
# STATE_DB BFD_SOFTWARE_SESSION_TABLE -> bgpcfgd's BfdMgr -> bfdd) and
# bfdsyncd (ours: bfdd session state -> STATE_DB BFD_SESSION_TABLE, which
# staticroutebfd and "show bfd summary" read). VRF-aware throughout (bfdd
# runs sessions in any VRF zebra knows). README.md next to this file.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkIf mkDefault types concatStringsSep;
  cfg = config.services.sonic;
  bcfg = cfg.bfd;
  fcfg = cfg.frr;
  routing = cfg.packages.routing;
  frr = fcfg.package;
  # As frr.nix's (same chain layout, its own chain).
  iptablesAcceptExcept = chain: except: matches: ''
    ip46tables -F ${chain} 2>/dev/null || true
    ip46tables -X ${chain} 2>/dev/null || true
    ip46tables -N ${chain}
    ${lib.concatMapStrings (i: "ip46tables -A ${chain} -i ${i} -j RETURN\n") except}
    ip46tables -A ${chain} -j nixos-fw-accept
    ${lib.concatMapStrings (m: "ip46tables -A nixos-fw ${m} -j ${chain}\n") matches}
  '';
  unit = n: "sonic-${n}.service";
  orch = unit "orchagent";
  bfdd = unit "frr-bfdd";

  waitBfdd = pkgs.writeShellScript "sonic-frr-wait-bfdd" ''
    for i in $(seq 100); do
      ${frr}/bin/vtysh -d bfdd -c "show version" >/dev/null 2>&1 && exit 0
      sleep 0.1
    done
    echo "bfdd is not answering on its vty socket" >&2
  '';
  # As frr.nix: BindsTo orchagent (upheld by it) and the daemons each one needs.
  svc = { description, exec, needs ? [ ], after ? [ ], extra ? { } }: lib.recursiveUpdate {
    inherit description;
    wantedBy = [ "sonic.target" ];
    partOf = [ "sonic.target" ];
    bindsTo = [ orch ] ++ needs;
    after = [ orch ] ++ needs ++ after;
    path = [ frr pkgs.coreutils ];
    unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
    serviceConfig = { ExecStart = exec; Restart = "always"; RestartSec = 3; };
  } extra;
  units = {
    # SONiC removes bfdd.conf in separated mode; sessions come from bgpd
    # (neighbor X bfd) and BfdMgr (vtysh), never from a file.
    frr-bfdd = svc {
      description = "SONiC FRR bfdd (software BFD)";
      exec = "${frr}/libexec/frr/bfdd -P 0";
      needs = [ (unit "frr-zebra") ];
      extra.serviceConfig = {
        ExecStartPre = [ "${pkgs.coreutils}/bin/install -o frr -g frr -m 0640 /dev/null /etc/frr/bfdd.conf" ];
        ExecStartPost = [ "${waitBfdd}" ];
      };
    };
    bfdsyncd = svc {
      description = "SONiC bfdsyncd (bfdd session state -> STATE_DB BFD_SESSION_TABLE)";
      exec = "${routing.bfdsyncd}/bin/bfdsyncd";
      needs = [ bfdd ];
      extra.environment.BFDSYNCD_INTERVAL_MS = toString bcfg.pollInterval;
    };
    # No ordering against bgpcfgd/bgpd (it only talks to redis): it takes ~1 s
    # to stop (select timeout), and a unit ordered after it would wait for that
    # on every stop. (Before 2026-11-27 that was also enough for "config reload"
    # to start the stack before it was down; sonic-nix reload now waits.)
    staticroutebfd = svc {
      description = "SONiC staticroutebfd (STATIC_ROUTE bfd=true -> BFD sessions, APPL_DB STATIC_ROUTE_TABLE)";
      exec = "${routing.bgpcfgd}/bin/staticroutebfd";
      extra.environment = with bcfg.staticRoute; {
        STATICROUTEBFD_TX_INTERVAL = toString txInterval;
        STATICROUTEBFD_RX_INTERVAL = toString rxInterval;
        STATICROUTEBFD_MULTIPLIER = toString multiplier;
        STATICROUTEBFD_MULTIHOP = lib.boolToString multihop;
      };
    };
  };
in {
  options.services.sonic.bfd = {
    enable = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Software BFD with the routing stack (services.sonic.frr): FRR bfdd,
        staticroutebfd, bgpcfgd's BfdMgr (seeds SYSTEM_DEFAULTS|software_bfd
        status=enabled) and bfdsyncd. For SAIs without BFD offload only
        (sai-openbcm; the vs SAI is told to report none): with offload,
        orchagent programs ASIC sessions and writes BFD_SESSION_TABLE itself.
      '';
    };
    staticRoute = {
      txInterval = mkOption { type = types.ints.between 10 60000; default = 300; description = "staticroutebfd sessions: transmit interval (ms). SONiC: 50, meant for the ASIC."; };
      rxInterval = mkOption { type = types.ints.between 10 60000; default = 300; description = "staticroutebfd sessions: receive interval (ms). SONiC: 50."; };
      multiplier = mkOption { type = types.ints.between 2 255; default = 3; description = "staticroutebfd sessions: detect multiplier."; };
      multihop = mkOption { type = types.bool; default = false; description = "staticroutebfd sessions: multihop (SONiC: false)."; };
    };
    pollInterval = mkOption {
      type = types.ints.between 20 5000;
      default = 200;
      description = ''
        bfdsyncd: how often (ms) bfdd's session states are read into STATE_DB.
        Adds up to this much to a static route's reaction to a BFD change
        (BGP gets BFD events from bfdd directly).
      '';
    };
    openFirewall = mkOption {
      type = types.bool;
      default = fcfg.openFirewall;
      defaultText = lib.literalExpression "services.sonic.frr.openFirewall";
      description = ''
        Accept BFD (UDP 3784 single hop, 4784 multihop, 3785 echo) on every
        interface except services.sonic.frr.managementInterfaces, as frr.nix
        does for BGP.
      '';
    };
  };

  config = mkIf (cfg.enable && fcfg.enable && bcfg.enable) {
    networking.firewall.extraInputRules = mkIf (bcfg.openFirewall && config.networking.nftables.enable) ''
      iifname != { ${concatStringsSep ", " (map (i: "\"${i}\"") fcfg.managementInterfaces)} } udp dport { 3784, 3785, 4784 } accept comment "BFD on switch interfaces"
    '';
    networking.firewall.extraCommands = mkIf (bcfg.openFirewall && !config.networking.nftables.enable)
      (iptablesAcceptExcept "sonic-bfd-in" fcfg.managementInterfaces (map (p: "-p udp --dport ${toString p}") [ 3784 3785 4784 ]));

    # bgpcfgd runs BfdMgr (STATE_DB BFD_SOFTWARE_SESSION_TABLE -> bfdd) only
    # with this; seeded, so "config" + "config save" can turn it off.
    services.sonic.settings.SYSTEM_DEFAULTS.software_bfd.status = mkDefault "enabled";
    services.sonic.keyModes.SYSTEM_DEFAULTS.software_bfd = mkDefault "seed";
    # The vs SAI claims full BFD offload unless told otherwise; then BfdOrch
    # would create (never-up) ASIC sessions instead of software ones.
    services.sonic.sai.profile = mkIf (cfg.sai.kind == "vs") { SAI_VS_BFD_OFFLOAD_SUPPORTED = mkDefault "false"; };

    systemd.services = lib.listToAttrs (lib.mapAttrsToList (n: v: lib.nameValuePair "sonic-${n}" v) units) // {
      # BfdMgr looks for bfdd with pgrep and starts its own when none runs.
      sonic-bgpcfgd = { bindsTo = [ bfdd ]; after = [ bfdd ]; path = [ pkgs.procps ]; };
      sonic-orchagent.upholds = map unit (lib.attrNames units);
    };
  };
}

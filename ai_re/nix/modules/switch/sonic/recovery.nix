# Crash recovery for services.sonic, as SONiC's supervisors do it
# (README.md "units"; hardware evidence: devices/sonic-nixos/
# sonic-as7760-counters-recovery-20261115.txt):
#  - orchagent failure (crash, kill, abort) restarts syncd and with it
#    everything below (SONiC restarts swss together with syncd); a restart of
#    orchagent alone left the KNET hostif netdevs gone. syncd failure already
#    restarts orchagent (BindsTo + Upholds in default.nix).
#  - Every syncd start is a cold start, as swss.sh: APPL/ASIC/COUNTERS DBs
#    flushed, stale STATE_DB tables removed (all writers restart with syncd).
#  - teamd death: sonic-teamd-watch respawns the dead teamd of a PortChannel
#    with the same arguments, taking over the existing team netdev (-o, as
#    teammgrd's warm start), so its IP, VLANs and members stay and the other
#    PortChannels are not touched. If that fails, teammgrd is restarted
#    (all PortChannels re-created: SONiC's teamd container restart).
{ config, lib, pkgs, ... }:
let
  cfg = config.services.sonic;
  sd = config.systemd.package;
  # ExecStopPost of orchagent. A stop that systemd asked for (stop/restart,
  # or syncd going down through BindsTo) ends with SERVICE_RESULT=success.
  orchagentFailed = pkgs.writeShellScript "sonic-orchagent-failed" ''
    [ "''${SERVICE_RESULT:-success}" = success ] && exit 0
    [ -e /run/sonic-nix/reloading ] && exit 0
    state=$(${sd}/bin/systemctl show -p ActiveState --value sonic-syncd.service)
    case "$state" in deactivating|activating|inactive|failed) exit 0 ;; esac
    echo "orchagent failed ($SERVICE_RESULT ''${EXIT_STATUS:-}): restarting syncd and the units below it"
    exec ${sd}/bin/systemctl restart --no-block sonic-syncd.service
  '';
  # swss.sh start (cold): syncd starts from empty ASIC/APPL/COUNTERS DBs and
  # without stale STATE_DB entries; everything that fills them restarts with
  # syncd. Without this a restarted syncd compared its new view with the old
  # ASIC_DB and orchagent aborted on the APPLY VIEW failure.
  flushDbs = pkgs.writeShellScript "sonic-syncd-flush-dbs" ''
    set -e
    for d in APPL_DB ASIC_DB COUNTERS_DB FLEX_COUNTER_DB GB_ASIC_DB GB_COUNTERS_DB RESTAPI_DB APPL_STATE_DB; do
      sonic-db-cli $d FLUSHDB >/dev/null
    done
    sonic-db-cli STATE_DB EVAL "local t = {${stateTables}}; for i = 1, #t do for _, k in ipairs(redis.call('KEYS', t[i])) do redis.call('DEL', k) end end" 0 >/dev/null
    echo "flushed APPL/ASIC/COUNTERS/FLEX_COUNTER/APPL_STATE DBs and stale STATE_DB tables"
  '';
  stateTables = lib.concatMapStringsSep ", " (t: "'${t}*'") [
    "PORT_TABLE" "MGMT_PORT_TABLE" "VLAN_TABLE" "VLAN_MEMBER_TABLE" "LAG_TABLE" "LAG_MEMBER_TABLE"
    "INTERFACE_TABLE" "MIRROR_SESSION" "VRF_TABLE" "FDB_TABLE" "FG_ROUTE_TABLE" "BUFFER_POOL"
    "BUFFER_PROFILE" "MUX_CABLE_TABLE" "ADVERTISE_NETWORK_TABLE" "VXLAN_TUNNEL_TABLE" "VNET_ROUTE"
    "MACSEC_PORT_TABLE" "MACSEC_INGRESS_SA_TABLE" "MACSEC_EGRESS_SA_TABLE" "MACSEC_INGRESS_SC_TABLE"
    "MACSEC_EGRESS_SC_TABLE" "VRF_OBJECT_TABLE" "VNET_MONITOR_TABLE" "BFD_SESSION_TABLE"
    "SYSTEM_NEIGH_TABLE" "FABRIC_PORT_TABLE" "TUNNEL_DECAP_TABLE" "TUNNEL_DECAP_TERM_TABLE"
    "HIGH_FREQUENCY_TELEMETRY_SESSION_TABLE" "PROCESS_HEALTH" "COPP_TRAP_TABLE"
  ];
  teamdWatch = pkgs.writeShellScript "sonic-teamd-watch" ''
    set -u
    st=/run/sonic-teamd-watch; mkdir -p "$st"
    declare -A fails
    alive() { [ -n "$1" ] && [ -r /proc/$1/cmdline ] && tr '\0' ' ' < /proc/$1/cmdline | grep -q "teamd .*-t $2 "; }
    while sleep 2; do
      for pidf in /run/teamd/*.pid; do
        [ -e "$pidf" ] || continue
        lag=$(basename "$pidf" .pid)
        pid=$(cat "$pidf" 2>/dev/null || true)
        if alive "$pid" "$lag"; then
          cmp -s /proc/$pid/cmdline "$st/$lag.cmdline" || cp /proc/$pid/cmdline "$st/$lag.cmdline"
          fails[$lag]=0
          continue
        fi
        # teamd removes its pid file on a normal exit (PortChannel deleted):
        # a pid file with no teamd behind it is a crash.
        [ -s "$st/$lag.cmdline" ] && ip link show "$lag" >/dev/null 2>&1 || { rm -f "$pidf"; continue; }
        n=$(( ''${fails[$lag]:-0} + 1 )); fails[$lag]=$n
        if [ "$n" -gt 3 ]; then
          echo "teamd for $lag does not stay up: restarting teammgrd (all PortChannels)"
          rm -f "$st"/*.cmdline; fails=()
          systemctl restart sonic-teammgrd.service
          break
        fi
        mapfile -d "" args < "$st/$lag.cmdline"
        new=()
        for a in "''${args[@]}"; do case "$a" in -r|-w) ;; *) new+=("$a") ;; esac; done
        echo "teamd for $lag died (pid $pid): respawning, taking over the team device (try $n)"
        rm -f "$pidf" /run/teamd/$lag.sock
        "''${new[0]}" -o "''${new[@]:1}" || echo "teamd respawn for $lag failed" >&2
        # Into teammgrd's cgroup: stopped with it, like the teamds it started.
        cg=$(systemctl show -p ControlGroup --value sonic-teammgrd.service)
        newpid=$(cat "$pidf" 2>/dev/null || true)
        [ -n "$cg" ] && [ -n "$newpid" ] && echo "$newpid" > "/sys/fs/cgroup$cg/cgroup.procs" 2>/dev/null || true
        # tlm_teamd (STATE_DB LAG state, "show interfaces portchannel") holds
        # a teamdctl connection to the dead teamd: reconnect it.
        systemctl restart --no-block sonic-tlm_teamd.service
      done
    done
  '';
in {
  config = lib.mkIf cfg.enable {
    systemd.services.sonic-orchagent.serviceConfig.ExecStopPost = [ "${orchagentFailed}" ];
    # nixos-rebuild switch restarting a changed orchagent unit must restart
    # syncd too: orchagent alone against a running syncd leaves no KNET
    # netdevs (AS7760, 2026-11-15, BFD deploy). syncd's unit carries
    # orchagent's as a restart trigger, so it changes whenever orchagent's
    # does and activation restarts both. (PartOf=orchagent did this too but
    # turned a killed syncd's auto-restart into a stop: vm-frr caught it.)
    # A manual restart: `systemctl restart sonic-syncd` (orchagent follows).
    systemd.services.sonic-syncd.restartTriggers = [ config.systemd.units."sonic-orchagent.service".unit ];
    systemd.services.sonic-syncd.serviceConfig.ExecStartPre = [ "${pkgs.writeShellScript "sonic-syncd-pre" ''
      PATH=${cfg.packages.swss-common}/bin:$PATH exec ${flushDbs}
    ''}" ];
    systemd.services.sonic-teammgrd.upholds = [ "sonic-teamd-watch.service" ];
    systemd.services.sonic-teamd-watch = {
      description = "Respawn crashed teamd instances (SONiC PortChannels)";
      wantedBy = [ "sonic.target" ];
      partOf = [ "sonic.target" ];
      bindsTo = [ "sonic-teammgrd.service" ];
      after = [ "sonic-teammgrd.service" ];
      path = [ cfg.packages.libteam pkgs.iproute2 pkgs.coreutils pkgs.diffutils pkgs.gnugrep sd ];
      serviceConfig = { ExecStart = "${teamdWatch}"; Restart = "always"; RestartSec = 3; };
    };
  };
}

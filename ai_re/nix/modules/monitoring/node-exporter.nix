# Prometheus node metrics for appliances (switch / embedded control planes):
# node_exporter on :9100 with a control-plane collector set, a textfile
# directory, and "textfile producers" (command + interval) run by systemd
# timers that publish .prom files atomically and never leave stale data.
#
# Go's ppc64 port needs POWER8 (ISA 2.07): on older PowerPC (e6500, e5500,
# 32-bit) node_exporter cross-builds but dies with SIGILL, so there the
# implementation defaults to node-exporter-lite (same port, flags, metric
# names for the collectors it has; stdlib Python). See ./README.md.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption types;
  cfg = config.monitoring.nodeExporter;
  hp = pkgs.stdenv.hostPlatform;
  goRuns = !hp.isPower
    || (hp.is64bit && (hp.isLittleEndian
      || builtins.elem (hp.gcc.cpu or "") [ "power8" "power9" "power10" "power11" ]));
  dir = cfg.textfile.directory;
  lite = pkgs.callPackage ../../packages/node-exporter-lite { };
  mkProducer = pkgs.callPackage ./textfile-producer.nix { };
  flags = map (c: "--collector.${c}") cfg.enabledCollectors
    ++ map (c: "--no-collector.${c}") cfg.disabledCollectors
    ++ [ "--collector.textfile.directory=${dir}" ] ++ cfg.extraFlags;
  producers = cfg.textfile.producers;
  staleAfter = p: if p.staleAfter != null then p.staleAfter else 3 * p.interval + p.timeout;
  pruneCases = lib.concatStrings (lib.mapAttrsToList
    (n: p: "    ${n}.prom) prune \"$f\" ${toString (staleAfter p)} ;;\n") producers);
  pruneOther = if cfg.textfile.unmanagedMaxAge == null then ":"
    else ''prune "$f" ${toString cfg.textfile.unmanagedMaxAge}'';
  cleanup = pkgs.writeShellScript "prometheus-textfile-cleanup" ''
    set -u
    PATH=${pkgs.coreutils}/bin:$PATH
    now=$(date +%s)
    prune() {
      m=$(stat -c %Y -- "$1" 2>/dev/null) || return 0
      if [ $((now - m)) -gt "$2" ]; then rm -f -- "$1"; echo "removed stale $1" >&2; fi
    }
    for f in ${dir}/*.prom; do
      [ -e "$f" ] || continue
      case "''${f##*/}" in
    ${pruneCases}    *) ${pruneOther} ;;
      esac
    done
  '';
in {
  options.monitoring.nodeExporter = {
    enable = lib.mkEnableOption "Prometheus node metrics (node_exporter or node-exporter-lite) with textfile producers";
    implementation = mkOption {
      type = types.enum [ "upstream" "lite" ];
      default = if goRuns then "upstream" else "lite";
      defaultText = lib.literalMD "`upstream` where Go binaries run, `lite` on pre-POWER8 PowerPC";
      description = "`upstream`: nixpkgs' node_exporter (services.prometheus.exporters.node). `lite`: ../../packages/node-exporter-lite.";
    };
    port = mkOption { type = types.port; default = 9100; };
    listenAddress = mkOption {
      type = types.str;
      default = "[::]";
      description = "Address to listen on; `[::]` is all IPv4 and IPv6 addresses.";
    };
    openFirewall = mkOption { type = types.bool; default = true; description = "Open the port in the firewall."; };
    enabledCollectors = mkOption {
      type = types.listOf types.str;
      default = [ "systemd" ];
      description = "Collectors enabled on top of node_exporter's defaults (hwmon, thermal_zone, netdev, netclass, cpu, loadavg, meminfo, filesystem, diskstats, edac, pressure, textfile, time, uname, ...).";
    };
    disabledCollectors = mkOption {
      type = types.listOf types.str;
      # Nothing to report on a switch control plane, or (dmi, rapl) not on non-x86.
      default = [ "bcache" "btrfs" "dmi" "fibrechannel" "infiniband" "ipvs" "mdadm" "nfs" "nfsd"
                  "nvme" "rapl" "selinux" "tapestats" "xfs" "zfs" ];
      description = "Default collectors to turn off.";
    };
    extraFlags = mkOption { type = types.listOf types.str; default = [ ]; };
    textfile = {
      directory = mkOption {
        type = types.str;
        default = "/run/prometheus-textfile";
        description = "Textfile collector directory (tmpfs: nothing survives a reboot).";
      };
      unmanagedMaxAge = mkOption {
        type = types.nullOr types.ints.positive;
        default = 3600;
        description = "Delete *.prom files that no producer owns once they are this many seconds old (e.g. a removed producer's last file); null keeps them.";
      };
      producers = mkOption {
        default = { };
        description = "Commands whose stdout (Prometheus text format, no timestamps) becomes <directory>/<name>.prom.";
        type = types.attrsOf (types.submodule {
          options = {
            command = mkOption { type = types.str; description = "Command line (not a shell snippet) printing metrics on stdout."; };
            interval = mkOption { type = types.ints.positive; default = 30; description = "Seconds between runs."; };
            timeout = mkOption { type = types.ints.positive; default = 20; description = "Seconds before the run is killed and its file removed."; };
            staleAfter = mkOption {
              type = types.nullOr types.ints.positive;
              default = null;
              description = "Delete the file when it has not been refreshed for this many seconds (default 3 x interval + timeout).";
            };
            user = mkOption { type = types.str; default = "root"; };
            path = mkOption { type = types.listOf types.package; default = [ ]; description = "Extra packages on PATH."; };
            serviceConfig = mkOption { type = types.attrs; default = { }; description = "Extra systemd [Service] settings."; };
          };
        });
      };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      assertions = [ {
        assertion = lib.all (n: builtins.match "[A-Za-z0-9_-]+" n != null) (lib.attrNames producers);
        message = "monitoring.nodeExporter.textfile.producers: names must match [A-Za-z0-9_-]+";
      } {
        assertion = !(producers ? cleanup);
        message = "monitoring.nodeExporter.textfile.producers.cleanup: the name is taken by the stale-file cleaner (prometheus-textfile-cleanup)";
      } ];
      networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
      systemd.tmpfiles.rules = [ "d ${dir} 0755 root root -" ];
      systemd.services = lib.mapAttrs' (name: p: lib.nameValuePair "prometheus-textfile-${name}" {
        description = "Prometheus textfile producer ${name}";
        after = [ "systemd-tmpfiles-setup.service" ];
        path = p.path;
        serviceConfig = {
          Type = "oneshot";
          User = p.user;
          ExecStart = mkProducer { inherit name; inherit (p) command timeout; directory = dir; };
          TimeoutStartSec = p.timeout + 15;
          RuntimeDirectory = "prometheus-textfile-${name}";   # scratch/cache, kept between runs
          RuntimeDirectoryPreserve = true;
          ProtectSystem = "strict";
          ReadWritePaths = [ dir ];
          ProtectHome = true;
          PrivateTmp = true;
          NoNewPrivileges = true;
          Nice = 10;
        } // p.serviceConfig;
      }) producers // {
        prometheus-textfile-cleanup = {
          description = "Remove stale Prometheus textfile collector files";
          serviceConfig = { Type = "oneshot"; ExecStart = cleanup; };
        };
      };
      systemd.timers = lib.mapAttrs' (name: p: lib.nameValuePair "prometheus-textfile-${name}" {
        wantedBy = [ "timers.target" ];
        timerConfig = {
          OnBootSec = "15s";
          OnUnitActiveSec = "${toString p.interval}s";
          AccuracySec = "1s";
        };
      }) producers // {
        prometheus-textfile-cleanup = {
          wantedBy = [ "timers.target" ];
          timerConfig = { OnBootSec = "1min"; OnUnitActiveSec = "1min"; AccuracySec = "5s"; };
        };
      };
    }
    (lib.mkIf (cfg.implementation == "upstream") {
      services.prometheus.exporters.node = {
        enable = true;
        inherit (cfg) port listenAddress disabledCollectors;
        # mkDefault: a host that sets its own list wins instead of concatenating with ours
        # (node_exporter refuses a repeated --collector.X flag).
        enabledCollectors = lib.mkDefault cfg.enabledCollectors;
        openFirewall = lib.mkDefault false;   # handled above for both implementations (a host may also open it here)
        extraFlags = [ "--collector.textfile.directory=${dir}" ] ++ cfg.extraFlags;
      };
    })
    (lib.mkIf (cfg.implementation == "lite") {
      systemd.services.prometheus-node-exporter = {
        description = "Prometheus node metrics (node-exporter-lite)";
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" ];
        path = [ config.systemd.package ];   # systemctl, for the systemd collector
        serviceConfig = {
          ExecStart = lib.escapeShellArgs ([ "${lite}/bin/node-exporter-lite"
            "--web.listen-address=${cfg.listenAddress}:${toString cfg.port}" ] ++ flags);
          DynamicUser = true;
          Restart = "always";
          RestartSec = "5s";
          MemoryMax = "128M";
          Nice = 5;
          ProtectSystem = "strict";
          ProtectHome = true;
          PrivateTmp = true;
          PrivateDevices = true;
          NoNewPrivileges = true;
          ProtectKernelTunables = true;
          ProtectKernelModules = true;
          ProtectControlGroups = true;
          RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" "AF_NETLINK" ];
          CapabilityBoundingSet = "";
        };
      };
    })
  ]);
}

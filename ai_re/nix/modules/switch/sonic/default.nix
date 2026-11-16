# SONiC control plane as NixOS services: redis (CONFIG/APPL/ASIC/STATE DBs),
# CONFIG_DB owned per config-ownership.md (Nix base + "config save" file,
# enforce/seed/unmanaged), syncd (virtual or vendor SAI), orchagent and the
# managers/syncers for ports, VLANs and PortChannels, the show/config CLI.
# README.md next to this file; packages in ../../../packages/sonic.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkEnableOption mkIf types mapAttrs mapAttrs' nameValuePair
    concatStringsSep concatMapStringsSep splitString optional optionals optionalAttrs
    listToAttrs elem hasAttr attrNames isList isBool filterAttrs;
  cfg = config.services.sonic;
  sonic = cfg.packages;
  spy = sonic.python;
  # Build-time tools (YANG validation, template rendering) run on the build host.
  sonicBuild = import ../../../packages/sonic { pkgs = pkgs.buildPackages; };
  str = toString;

  P = import ./ports.nix { inherit lib; } { inherit (cfg) cages breakout defaultSpeed validSpeeds allowUnusedLanes; };
  portByName = listToAttrs (map (p: nameValuePair p.name p) P.ports);
  translateKey = k: concatStringsSep "|" (map (part: P.aliasToName.${part} or part) (splitString "|" k));
  translateKeys = mapAttrs (_: entries: mapAttrs' (k: v: nameValuePair (translateKey k) v) entries);
  value = v: if isList v then map str v else if isBool v then (if v then "true" else "false") else str v;

  generated = {
    # What NixOS knows about the device, mirrored (enforced) for SONiC tools.
    DEVICE_METADATA.localhost = {
      inherit (cfg) platform hwsku;
      mac = cfg.macAddress;
    } // optionalAttrs (config.networking.hostName != "") {
      hostname = config.networking.hostName;
    } // optionalAttrs (config.time.timeZone != null) {
      timezone = config.time.timeZone;
    };
    PORT = listToAttrs (map (p: nameValuePair p.name {
      lanes = concatMapStringsSep "," str p.lanes;
      inherit (p) alias;
      index = str p.cage;
      speed = str p.speed;
      admin_status = "down";        # undeclared ports stay down (seed)
      mtu = "9100";
    }) P.ports);
  };
  settings = mapAttrs (_: mapAttrs (_: mapAttrs (_: value))) (translateKeys cfg.settings);
  base = lib.recursiveUpdate generated settings;

  fieldModes = translateKeys cfg.fieldModes;
  keyModes = translateKeys cfg.keyModes;
  hardwareMode = t: k: f:
    if t == "PORT" && hasAttr k portByName then
      (if elem f [ "lanes" "alias" "index" ] || (f == "speed" && portByName.${k}.fromBreakout) then "enforce" else null)
    else if t == "DEVICE_METADATA" && k == "localhost" && elem f [ "platform" "hwsku" "mac" "hostname" "timezone" ] then "enforce"
    else null;
  softDefault = t: k: if t == "PORT" && hasAttr k portByName then "seed" else cfg.defaultMode;
  modeOf = t: k: f:
    fieldModes.${t}.${k}.${f} or keyModes.${t}.${k} or
    (let h = hardwareMode t k f; in if h != null then h else cfg.tableModes.${t} or (softDefault t k));
  modes = mapAttrs (t: mapAttrs (k: mapAttrs (f: _: modeOf t k f))) base;

  baseJson = pkgs.writeText "sonic-config-db-nix.json" (builtins.toJSON base);
  yangDir = "${spy.yang-models}/share/sonic/yang-models";
  # The Nix side, YANG-validated at build time (fails the build when invalid).
  validatedBase = pkgs.runCommand "sonic-config-db-nix-validated.json" {
    nativeBuildInputs = [ (sonicBuild.python.python3.withPackages (_: [ sonicBuild.python.yang-mgmt ])) ];
  } ''
    python3 -I ${./validate.py} ${sonicBuild.python.yang-models}/share/sonic/yang-models ${baseJson} \
      ${concatStringsSep " " cfg.tablesWithoutYang}
    cp ${baseJson} $out
  '';
  modesJson = pkgs.writeText "sonic-config-db-nix-modes.json" (builtins.toJSON modes);
  metaJson = pkgs.writeText "sonic-nix-meta.json" (builtins.toJSON {
    inherit (cfg) hostTables hostFields;
    ownedKeyTables = [ "PORT" ];
    userFile = "/var/lib/sonic/config_db.json";
    runDir = "/run/sonic-nix";
    inherit yangDir;
  });

  # swss-common's DB map minus the chassis instance (not run here); the
  # DPU_* databases need ids up to 18, so redis gets cfg.redisDatabases.
  dbConfig = pkgs.runCommand "database_config.json" { nativeBuildInputs = [ pkgs.buildPackages.jq ]; } ''
    jq '.INSTANCES |= (del(.redis_chassis) | .redis.unix_socket_path = "/var/run/redis/redis.sock"
          | .redis.hostname = "127.0.0.1" | .redis.port = 6379)
        | .DATABASES |= with_entries(select(.value.instance != "redis_chassis"))' \
      ${sonic.swss-common}/share/swsscommon/sonic-db/database_config.json > $out
    max=$(jq '[.DATABASES[].id] | max' $out)
    if [ "$max" -ge ${str cfg.redisDatabases} ]; then
      echo "database_config.json uses db id $max, redis has ${str cfg.redisDatabases} databases" >&2; exit 1
    fi
  '';

  vs = cfg.sai.kind == "vs";
  # Vendor SDK config from the ports (a board function; Tomahawk etc. live there).
  sdkConfig = if cfg.sai.sdkConfig.generator == null then null else cfg.sai.sdkConfig.generator {
    ports = map (p: { inherit (p) name alias lanes speed; }) P.ports;
    inherit (cfg.sai.sdkConfig) settings;
  };
  vsIfname = p: cfg.sai.vs.interfaces.${p.alias} or "sw${lib.removePrefix "Ethernet" p.name}";
  laneMap = pkgs.writeText "lanemap.ini" (concatMapStringsSep "\n" (p: "${vsIfname p}:${concatMapStringsSep "," str p.lanes}") P.ports + "\n");
  saiProfileAttrs = optionalAttrs vs {
    SAI_VS_SWITCH_TYPE = "SAI_VS_SWITCH_TYPE_BCM56850";
    SAI_VS_HOSTIF_USE_TAP_DEVICE = "true";
    SAI_VS_INTERFACE_LANE_MAP_FILE = "${laneMap}";
    SAI_VS_USE_CONFIGURED_SPEED_AS_OPER_SPEED = "true";
  } // optionalAttrs (cfg.sai.initConfigFile != null) {
    SAI_INIT_CONFIG_FILE = cfg.sai.initConfigFile;
  } // cfg.sai.profile;
  saiProfile = pkgs.writeText "sai.profile" (concatStringsSep "\n" (lib.mapAttrsToList (k: v: "${k}=${v}") saiProfileAttrs) + "\n");
  portConfigIni = pkgs.writeText "port_config.ini" (''
    # name lanes alias index speed
  '' + concatMapStringsSep "\n" (p: concatStringsSep " " [
    p.name (concatMapStringsSep "," str p.lanes) p.alias (str p.cage) (str p.speed) ]) P.ports + "\n");
  # /usr/share/sonic as SONiC tools expect it (device/<platform>/<hwsku>, templates).
  shareSonic = pkgs.runCommand "sonic-share-${cfg.hwsku}" { } ''
    d=$out/device/${cfg.platform}
    mkdir -p $d/${cfg.hwsku}
    echo "${cfg.hwsku}" > $d/default_sku
    cp ${portConfigIni} $d/${cfg.hwsku}/port_config.ini
    cp ${saiProfile} $d/${cfg.hwsku}/sai.profile
    ${lib.optionalString vs "cp ${laneMap} $d/${cfg.hwsku}/lanemap.ini"}
    ln -s ${spy.config-engine}/share/sonic/templates $out/templates
    ln -s device/${cfg.platform} $out/platform
    ln -s device/${cfg.platform}/${cfg.hwsku} $out/hwsku
  '';
  coppCfg = pkgs.runCommand "copp_cfg.json" { nativeBuildInputs = [ sonicBuild.python.config-engine ]; } ''
    sonic-cfggen -t ${sonicBuild.sources.buildimage}/files/image_config/copp/copp_cfg.j2 > $out
  '';

  pyEnv = spy.python3.withPackages (_: [ spy.yang-mgmt spy.swsscommon ]);
  sonicNix = pkgs.writeShellScriptBin "sonic-nix" ''
    export SONIC_NIX_BASE=${validatedBase} SONIC_NIX_MODES=${modesJson} SONIC_NIX_META=${metaJson}
    export PATH=${lib.makeBinPath [ pkgs.util-linux config.systemd.package ]}:$PATH
    exec ${pyEnv}/bin/python3 -I ${./sonic-nix.py} "$@"
  '';
  sonicNixDiff = pkgs.writeShellScriptBin "sonic-nix-diff" ''exec ${sonicNix}/bin/sonic-nix diff "$@"'';

  # ExecStopPost of syncd: in 10 s, if syncd is neither running, starting nor
  # queued, start sai.restartOnStop again (an immediate start would fight a
  # restart of syncd through Conflicts=).
  restartOnStop = pkgs.writeShellScript "sonic-syncd-restart-on-stop" ''
    [ -e /run/sonic-nix/reloading ] && exit 0      # sonic-nix reload / config reload
    state=$(systemctl show -p ActiveState --value sonic-syncd.service)
    case "$state" in active|activating|reloading|deactivating) exit 0 ;; esac
    systemctl list-jobs --no-legend | grep -q ' sonic-syncd.service ' && exit 0
    echo "syncd stopped: starting ${concatStringsSep " " cfg.sai.restartOnStop}"
    exec systemctl start --no-block ${lib.escapeShellArgs cfg.sai.restartOnStop}
  '';
  swss = sonic.swss;
  # nbrmgrd runs ndisc6; take only that binary (the package's dnssort script
  # references the build machine's perl in cross builds).
  # (Its only self-reference is the gettext locale dir.)
  ndisc6 = pkgs.runCommand "ndisc6-bin-${pkgs.ndisc6.version}" { nativeBuildInputs = [ pkgs.buildPackages.removeReferencesTo ]; } ''
    install -D ${pkgs.ndisc6}/bin/ndisc6 $out/bin/ndisc6
    remove-references-to -t ${pkgs.ndisc6} $out/bin/ndisc6
  '';
  daemonPath = with pkgs; [ iproute2 bridge-utils iptables conntrack-tools ndisc6 procps
    coreutils gnugrep gnused kmod util-linux sonic.libteam ];
  # syncd -> {portsyncd, orchagent} -> managers: a stop/crash of the upstream
  # unit stops the downstream ones (BindsTo), and they come back with it
  # (Upholds), like SONiC restarting the swss/teamd containers with syncd.
  daemon = { description, exec, bindsTo, after ? [ ], extra ? { } }: lib.recursiveUpdate {
    inherit description;
    wantedBy = [ "sonic.target" ];
    partOf = [ "sonic.target" ];
    bindsTo = [ bindsTo ];
    after = [ bindsTo "sonic-config-loader.service" ] ++ after;
    requires = [ "sonic-config-loader.service" ];
    path = daemonPath;
    environment = { platform = cfg.asicType; ASIC_VENDOR = cfg.asicType; };
    unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
    serviceConfig = {
      ExecStart = exec;
      Restart = "always";
      RestartSec = 3;
    };
  } extra;
  # tlm_teamd: teamd state -> STATE_DB LAG_TABLE (runner.active, ...), which
  # "show interfaces portchannel" reads.
  managers = [ "portmgrd" "intfmgrd" "vlanmgrd" "nbrmgrd" "teammgrd" "teamsyncd" "tlm_teamd" "neighsyncd" "fdbsyncd" ]
    ++ optional cfg.copp.enable "coppmgrd";
  unit = n: "sonic-${n}.service";
in {
  imports = [ ./frr.nix ./recovery.nix ./vrf.nix ./counters.nix ./pmon.nix ./lldp.nix ./bfd.nix ./metrics.nix ];
  options.services.sonic = {
    enable = mkEnableOption "the SONiC switch control plane (redis, syncd, orchagent, managers, CLI)";
    packages = mkOption {
      type = types.raw;
      default = import ../../../packages/sonic { inherit pkgs; };
      defaultText = lib.literalExpression "import ai_re/nix/packages/sonic { inherit pkgs; }";
      description = "The SONiC package set (swss-common, sairedis, swss, libteam, python).";
    };
    platform = mkOption { type = types.str; example = "x86_64-accton_as7712_32x-r0"; description = "SONiC platform string (DEVICE_METADATA platform, /usr/share/sonic/device/<platform>)."; };
    hwsku = mkOption { type = types.str; example = "Accton-AS7712-32X"; description = "SONiC hwsku (DEVICE_METADATA hwsku)."; };
    macAddress = mkOption { type = types.strMatching "([0-9a-f]{2}:){5}[0-9a-f]{2}"; description = "Switch MAC (DEVICE_METADATA mac, orchagent -m); lower case."; };
    asicType = mkOption {
      type = types.str;
      default = if vs then "vs" else "broadcom";
      defaultText = lib.literalExpression ''if sai.kind == "vs" then "vs" else "broadcom"'';
      description = "asic_type (sonic_version.yml, orchagent's platform environment variable).";
    };
    cages = mkOption {
      type = types.listOf (types.listOf types.ints.unsigned);
      example = lib.literalExpression "import ai_re/nix/modules/switch/sonic/port-maps/accton-as7712-32x.nix";
      description = "Front-panel cages in order (cage n = element n-1): the SerDes lanes of each. Every cage is one port at defaultSpeed unless breakout overrides it.";
    };
    defaultSpeed = mkOption { type = types.ints.positive; default = 100000; description = "Speed of an unsplit cage (Mb/s), seeded."; };
    validSpeeds = mkOption {
      type = types.attrsOf (types.listOf types.ints.positive);
      default = { "1" = [ 10000 25000 ]; "2" = [ 50000 ]; "4" = [ 40000 100000 ]; };
      description = "Valid port speeds by lane count (build-time check of breakout overrides).";
    };
    allowUnusedLanes = mkOption {
      type = types.bool;
      default = false;
      description = "A breakout may leave a cage's upper lanes unused (e.g. Teralynx 7 1x100G or 4x25G on an 8-lane QSFP-DD cage).";
    };
    breakout = mkOption {
      type = types.attrsOf (types.listOf (types.submodule {
        options = {
          alias = mkOption { type = types.str; example = "Ethernet1/2"; description = "Front-panel name Ethernet<cage>/<subport>; the SONiC name is generated."; };
          lanes = mkOption { type = types.listOf types.ints.unsigned; description = "Lanes of this port (of the cage's lanes)."; };
          speed = mkOption { type = types.ints.positive; description = "Speed in Mb/s (enforced)."; };
        };
      }));
      default = { };
      example = lib.literalExpression ''{ "1" = [ { alias = "Ethernet1/1"; lanes = [ 49 ]; speed = 25000; } ... ]; }'';
      description = "Per-cage breakout, keyed by cage number: every resulting port listed explicitly (config-ownership.md). Enforced.";
    };
    settings = mkOption {
      type = types.attrsOf (types.attrsOf (types.attrsOf (types.oneOf [ types.str types.int types.bool (types.listOf types.str) ])));
      default = { };
      example = { VLAN.Vlan100.vlanid = "100"; VLAN_MEMBER."Vlan100|Ethernet3/1".tagging_mode = "untagged"; PORT."Ethernet3/1".admin_status = "up"; };
      description = "CONFIG_DB entries Nix declares (TABLE.key.field). Key parts that are front-panel aliases become SONiC port names. Mode per field from fieldModes > keyModes > hardware rules > tableModes > defaultMode (PORT: seed).";
    };
    defaultMode = mkOption { type = types.enum [ "enforce" "seed" ]; default = "enforce"; description = "Mode of declared settings no other rule covers."; };
    tableModes = mkOption { type = types.attrsOf (types.enum [ "enforce" "seed" ]); default = { }; example = { VLAN = "seed"; }; description = "Mode per table."; };
    keyModes = mkOption { type = types.attrsOf (types.attrsOf (types.enum [ "enforce" "seed" ])); default = { }; example = { PORT."Ethernet3/1" = "enforce"; }; description = "Mode per key (TABLE.key)."; };
    fieldModes = mkOption { type = types.attrsOf (types.attrsOf (types.attrsOf (types.enum [ "enforce" "seed" ]))); default = { }; example = { PORT."Ethernet3/1".admin_status = "enforce"; }; description = "Mode per field (TABLE.key.field)."; };
    hostTables = mkOption {
      type = types.listOf types.str;
      default = [ "MGMT_INTERFACE" "MGMT_PORT" "MGMT_VRF_CONFIG" "NTP" "NTP_SERVER" "NTP_KEY" "SYSLOG*"
        "DNS_NAMESERVER" "DNS_OPTIONS" "AAA" "TACPLUS" "TACPLUS_SERVER" "RADIUS" "RADIUS_SERVER" "LDAP*"
        "SSH_SERVER" "PASSW_HARDENING" "BANNER_MESSAGE" "KDUMP" "SNMP*" "FIPS" "SERIAL_CONSOLE" ];
      description = "Tables NixOS owns: never loaded into CONFIG_DB (dropped from the saved file, flagged when written at runtime). A trailing * matches a prefix.";
    };
    hostFields = mkOption { type = types.listOf types.str; default = [ ]; example = [ "DEVICE_METADATA|localhost|bgp_asn" ]; description = "TABLE|key|field entries NixOS owns and keeps out of CONFIG_DB (dropped like hostTables). The hostname and time zone are mirrored (enforced) instead."; };
    tablesWithoutYang = mkOption { type = types.listOf types.str; default = [ ]; description = "Tables allowed in settings without a YANG model (not validated)."; };
    redisDatabases = mkOption { type = types.ints.positive; default = 20; description = "Number of redis databases (SONiC's DB map uses ids up to 18)."; };
    copp.enable = mkOption { type = types.bool; default = true; description = "Run coppmgrd with SONiC's default copp_cfg.json (traps LACP/ARP/... to the CPU)."; };
    sai = {
      kind = mkOption { type = types.enum [ "vs" "vendor" ]; default = "vs"; description = "vs: the virtual SAI in sonic.sairedis; vendor: sai.package's syncd linked to a libsai."; };
      package = mkOption {
        type = types.package;
        default = sonic.sairedis;
        defaultText = lib.literalExpression "sonic.sairedis";
        description = "sairedis package whose bin/syncd runs, e.g. sonic.sairedis-openbcm libsai.";
      };
      libraryPath = mkOption { type = types.listOf types.str; default = [ ]; description = "LD_LIBRARY_PATH for syncd (vendor libsai and its SDK)."; };
      initConfigFile = mkOption {
        type = types.nullOr types.str;
        default = if sdkConfig != null then "${sdkConfig}" else null;
        defaultText = lib.literalExpression "the file sdkConfig.generator makes, else null";
        example = "/var/lib/sonic/config.bcm";
        description = "SAI_INIT_CONFIG_FILE (vendor SDK config, e.g. Broadcom config.bcm). Default: generated from the ports by sdkConfig.generator; a path outside the store works too.";
      };
      sdkConfig = {
        generator = mkOption {
          type = types.nullOr (types.functionTo types.package);
          default = null;
          example = lib.literalExpression "import ai_re/nix/modules/boards/as7760/sdk-config.nix { inherit openbcm sonic; }";
          description = ''
            Board function { ports, settings } -> SDK config file (a derivation),
            called with the front-panel ports (name, alias, lanes, speed, the
            same as CONFIG_DB PORT) and sdkConfig.settings. It must fail its
            build unless the config's ports are exactly these (lanes, speeds).
            Its file is the default sai.initConfigFile and system.build.sonic.sdkConfig.
          '';
        };
        settings = mkOption {
          type = types.attrsOf (types.nullOr types.str);
          default = { };
          example = { l3_alpm_enable = null; bcm_num_cos = "8"; };
          description = "Global SDK config keys for the generator (override the board's; null removes one).";
        };
      };
      profile = mkOption { type = types.attrsOf types.str; default = { }; example = { SAI_OPENBCM_BACKEND = "openbcm"; SAI_OPENBCM_TEMP_FILE = "/run/asic/temperature"; }; description = "Extra sai.profile keys (override the generated ones)."; };
      conflicts = mkOption {
        type = types.listOf types.str;
        default = optional (config.hardware.openbcm.temperature.enable or false) "openbcm-asic-temp.service";
        defaultText = lib.literalExpression ''[ "openbcm-asic-temp.service" ] when hardware.openbcm.temperature.enable'';
        description = "Units that own the ASIC and must stop while syncd runs (one SDK process per unit).";
      };
      restartOnStop = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "openbcm-asic-temp.service" ];
        description = "Units to start again when syncd stays stopped (checked 10 s after it stops, so restarts of syncd are left alone), e.g. the ASIC temperature reader it displaced.";
      };
      vs.interfaces = mkOption { type = types.attrsOf types.str; default = { }; example = { "Ethernet5/1" = "eth1"; }; description = "vs only: kernel interface carrying a port's packets (alias -> ifname); other ports get dummy interfaces."; };
    };
    syncdArgs = mkOption { type = types.listOf types.str; default = [ "-u" "-z" "redis_sync" ]; description = "syncd arguments besides -p <profile>."; };
    orchagentArgs = mkOption { type = types.listOf types.str; default = [ "-b" "1024" "-z" "redis_sync" ]; description = "orchagent arguments besides -d and -m."; };
    cliNamingMode = mkOption { type = types.enum [ "default" "alias" ]; default = "alias"; description = "SONIC_CLI_IFACE_MODE for login shells (alias: Ethernet<cage>/<sub>)."; };
    kernelModules = mkOption { type = types.listOf types.str; default = [ "team" "team_mode_loadbalance" "bridge" "8021q" "dummy" ] ++ optional vs "tun"; description = "Kernel modules the managers need (team for PortChannels, bridge for VLANs)."; };
  };

  config = mkIf cfg.enable {
    assertions = P.assertions ++ lib.concatLists (lib.mapAttrsToList (t: entries: map (k: {
      assertion = t != "PORT" || hasAttr (translateKey k) portByName;
      message = "services.sonic.settings.PORT.\"${k}\": no such port (ports come from cages/breakout)";
    }) (attrNames entries)) cfg.settings) ++ map (a: {
      assertion = hasAttr a P.aliasToName;
      message = "services.sonic.sai.vs.interfaces.\"${a}\": no such port";
    }) (attrNames cfg.sai.vs.interfaces);
    warnings = optional (!vs && cfg.sai.conflicts != [ ] && !(saiProfileAttrs ? SAI_OPENBCM_TEMP_FILE))
      "services.sonic: syncd stops ${concatStringsSep ", " cfg.sai.conflicts} and its profile has no SAI_OPENBCM_TEMP_FILE: no ASIC temperature for fan control while syncd runs";

    # Generated files, for inspection and tests (nix build ...config.system.build.sonic.base).
    system.build.sonic = {
      base = validatedBase;
      modes = modesJson;
      inherit saiProfile portConfigIni dbConfig shareSonic laneMap coppCfg sonicNix sdkConfig;
      ports = P.ports;
    };

    boot.kernelModules = cfg.kernelModules;
    boot.kernel.sysctl = { "net.ipv4.ip_forward" = lib.mkDefault 1; "net.ipv6.conf.all.forwarding" = lib.mkDefault 1; };

    # redis links libsystemd (Type=notify); use the system's systemd, not
    # pkgs.systemd (a trimmed board systemd would otherwise pull a second,
    # full one into a cross build).
    services.redis.package = lib.mkDefault (pkgs.redis.override { systemd = config.systemd.package; });
    services.redis.servers.sonic = {
      enable = true;
      port = 6379;
      bind = "127.0.0.1";
      unixSocket = "/run/redis-sonic/redis.sock";
      unixSocketPerm = 770;
      databases = cfg.redisDatabases;
      save = [ ];
      appendOnly = false;
      # SubscriberStateTable and ConfigDBConnector listen on keyspace events.
      settings.notify-keyspace-events = "AKE";
    };

    systemd.tmpfiles.rules = [
      "d /run/redis 0755 root root -"
      "L+ /run/redis/redis.sock - - - - /run/redis-sonic/redis.sock"
      "d /run/redis/sonic-db 0755 root root -"
      "L+ /run/redis/sonic-db/database_config.json - - - - ${dbConfig}"
      "d /usr/share 0755 root root -"
      "L+ /usr/share/sonic - - - - ${shareSonic}"
      "d /var/lib/sonic 0755 root root -"
      "d /run/sonic-nix 0755 root root -"
      "d /var/warmboot/teamd 0755 root root -"
      # orchagent binds ZMQ endpoints there (a volume in SONiC's swss container).
      "d /zmq_swss 0755 root root -"
    ];

    environment.etc = {
      "sonic/sonic_version.yml".text = builtins.toJSON {
        build_version = "nixos-${config.system.nixos.release}";
        asic_type = cfg.asicType;
        built_by = "nix";
        commit_id = "";
        build_date = "";
        debian_version = "";
        kernel_version = "";
      };
      "sonic/copp_cfg.json" = mkIf cfg.copp.enable { source = coppCfg; };
      # agetty reads /etc/issue.d/*.issue; the loader keeps the text current.
      "issue.d/sonic-nix.issue".source = "/run/sonic-nix/issue";
    };
    users.motdFile = "/run/sonic-nix/motd";

    environment.systemPackages = [ spy.utilities spy.config-engine sonic.swss-common sonic.libteam
      sonicNix sonicNixDiff config.services.redis.package ];
    environment.variables.SONIC_CLI_IFACE_MODE = cfg.cliNamingMode;
    security.sudo.extraConfig = ''
      Defaults env_keep += "SONIC_CLI_IFACE_MODE"
    '';

    systemd.targets.sonic = {
      description = "SONiC switch stack";
      wantedBy = [ "multi-user.target" ];
    };

    systemd.services = {
      # Boot: Nix base + saved file -> CONFIG_DB. Not restarted by
      # nixos-rebuild switch (that is sonic-nix-apply's job) or by
      # "systemctl restart sonic.target" (unsaved runtime config survives).
      sonic-config-loader = {
        description = "SONiC CONFIG_DB from the Nix base and the saved config";
        wantedBy = [ "sonic.target" ];
        requires = [ "redis-sonic.service" ];
        after = [ "redis-sonic.service" "systemd-tmpfiles-setup.service" ];
        restartIfChanged = false;
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; ExecStart = "${sonicNix}/bin/sonic-nix load"; };
      };
      # nixos-rebuild switch: restarted when the Nix base changes; writes the
      # changed enforce/seed fields into the running CONFIG_DB.
      sonic-nix-apply = {
        description = "Apply changed Nix SONiC settings to the running CONFIG_DB";
        wantedBy = [ "sonic.target" ];
        requires = [ "sonic-config-loader.service" ];
        after = [ "sonic-config-loader.service" ];
        restartTriggers = [ validatedBase modesJson ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; ExecStart = "${sonicNix}/bin/sonic-nix apply"; };
      };
      sonic-nix-watch = {
        description = "Flag runtime changes to Nix-enforced SONiC settings";
        wantedBy = [ "sonic.target" ];
        partOf = [ "sonic.target" ];
        requires = [ "sonic-config-loader.service" ];
        after = [ "sonic-config-loader.service" ];
        serviceConfig = { ExecStart = "${sonicNix}/bin/sonic-nix watch"; Restart = "always"; RestartSec = 5; };
      };

      sonic-syncd = {
        description = "SONiC syncd (SAI: ${cfg.sai.kind})";
        wantedBy = [ "sonic.target" ];
        partOf = [ "sonic.target" ];
        requires = [ "redis-sonic.service" "sonic-config-loader.service" ] ++ optional vs "sonic-vs-ports.service";
        after = [ "redis-sonic.service" "sonic-config-loader.service" ] ++ optional vs "sonic-vs-ports.service" ++ cfg.sai.conflicts;
        conflicts = cfg.sai.conflicts;
        upholds = [ (unit "portsyncd") (unit "orchagent") ];
        path = daemonPath;
        environment = optionalAttrs (cfg.sai.libraryPath != [ ]) { LD_LIBRARY_PATH = concatStringsSep ":" cfg.sai.libraryPath; };
        unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
        serviceConfig = {
          ExecStart = concatStringsSep " " ([ "${cfg.sai.package}/bin/syncd" "-p" "${saiProfile}" ] ++ cfg.syncdArgs);
          Restart = "always";
          RestartSec = 3;
          ExecStopPost = lib.optional (cfg.sai.restartOnStop != [ ])
            "-${config.systemd.package}/bin/systemd-run --quiet --collect --on-active=10 --property=Environment=PATH=${config.systemd.package}/bin:${pkgs.gnugrep}/bin ${restartOnStop}";
          StateDirectory = "sonic-syncd";
          WorkingDirectory = "/var/lib/sonic-syncd";
        };
      };
      sonic-portsyncd = daemon {
        description = "SONiC portsyncd (CONFIG_DB PORT -> APPL_DB, netdev state)";
        exec = "${swss}/bin/portsyncd";
        bindsTo = unit "syncd";
      };
      sonic-orchagent = daemon {
        description = "SONiC orchagent";
        exec = concatStringsSep " " ([ "${swss}/bin/orchagent" "-d" "/var/log/swss" "-m" cfg.macAddress ] ++ cfg.orchagentArgs);
        bindsTo = unit "syncd";
        after = [ (unit "portsyncd") ];
        extra = {
          upholds = map unit managers;
          serviceConfig.LogsDirectory = "swss";
        };
      };
    } // listToAttrs (map (n: nameValuePair "sonic-${n}" (daemon {
      description = "SONiC ${n}";
      exec = "${swss}/bin/${n}";
      bindsTo = unit "orchagent";
      after = optional (n == "teamsyncd" || n == "tlm_teamd") (unit "teammgrd");
      extra = optionalAttrs (n == "teammgrd") {
        # teamd instances run in this unit's cgroup (stopped with it).
        serviceConfig.RuntimeDirectory = "teamd";
      };
    })) managers) // optionalAttrs vs {
      # The virtual SAI forwards each port's packets to an interface from the
      # lane map: real NICs for sai.vs.interfaces, dummies for the rest.
      sonic-vs-ports = {
        description = "Interfaces behind the virtual SAI's ports";
        before = [ (unit "syncd") ];
        path = [ pkgs.iproute2 ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = concatMapStringsSep "\n" (p: let i = vsIfname p; in
          if hasAttr p.alias cfg.sai.vs.interfaces then "ip link set ${i} up"
          else "ip link show ${i} >/dev/null 2>&1 || ip link add ${i} type dummy; ip link set ${i} up") P.ports;
      };
    };
  };
}

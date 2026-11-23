# MACsec for services.sonic (docker-macsec minus the container): macsecmgrd
# (CONFIG_DB MACSEC_PROFILE + PORT.macsec -> one wpa_supplicant per port),
# sonic-wpa-supplicant with the macsec_sonic driver (MKA; what it negotiates
# goes to APPL_DB MACSEC_* tables) and orchagent's MACsecOrch (already part
# of orchagent), which creates the SAI MACsec objects. The SAI must
# implement sai_macsec_api_t (sai-openbcm: an external MACsec table,
# packages/sai-openbcm/README.md). CONFIG_DB FEATURE|macsec is seeded
# enabled: coppmgrd installs the EAPOL trap only for an enabled feature.
#
# Keys. A profile's CAK is read from a file outside the store when the stack
# starts (cakFile: one line of hex, 32 digits for the 128-bit suites, 64 for
# the 256-bit ones; root only) and written to CONFIG_DB the way SONiC keeps
# it ("type 7" obfuscation, which macsecmgrd undoes). It is never in the Nix
# store; it is in redis and, after "config save", in
# /var/lib/sonic/config_db.json, as on SONiC.
# Profiles and port assignments are declared here, not in
# services.sonic.settings (the profile needs the key; a PORT.macsec without
# its profile would fail the build-time YANG check): sonic-macsec-config
# writes both at every stack start and when the declaration changes, and
# removes what it wrote earlier and is no longer declared. Entries made with
# "config macsec" are left alone.
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkEnableOption mkIf mkDefault types;
  cfg = config.services.sonic;
  mcfg = cfg.macsec;
  spy = cfg.packages.python;
  wpa = cfg.packages.wpa-supplicant;
  bi = cfg.packages.sources.buildimage;
  P = import ./ports.nix { inherit lib; } { inherit (cfg) cages breakout defaultSpeed validSpeeds allowUnusedLanes; };
  portName = p: P.aliasToName.${p} or p;
  suites = { "GCM-AES-128" = 32; "GCM-AES-256" = 64; "GCM-AES-XPN-128" = 32; "GCM-AES-XPN-256" = 64; };
  declared = pkgs.writeText "sonic-macsec-nix.json" (builtins.toJSON {
    profiles = lib.mapAttrs (_: p: {
      inherit (p) cakFile;
      cakHexDigits = suites.${p.cipherSuite};
      fields = {
        cipher_suite = p.cipherSuite;
        primary_ckn = p.ckn;
        priority = toString p.priority;
        policy = p.policy;
        send_sci = "true";
        enable_replay_protect = if p.replayProtect then "true" else "false";
      } // lib.optionalAttrs p.replayProtect { replay_window = toString p.replayWindow; }
        // lib.optionalAttrs (p.rekeyPeriod != 0) { rekey_period = toString p.rekeyPeriod; };
    }) mcfg.profiles;
    ports = lib.mapAttrs' (p: prof: lib.nameValuePair (portName p) prof) mcfg.ports;
    stateFile = "/var/lib/sonic/macsec-nix.json";
  });
  pyEnv = spy.python3.withPackages (_: [ spy.swsscommon ]);
  # "show macsec" / "sonic-clear macsec": docker-macsec's CLI plug-ins, run
  # beside sonic-utilities (its plug-in directories are in the store) by
  # wrappers that take the "macsec" sub-command and pass the rest on.
  cliEnv = spy.python3.withPackages (_: [ spy.utilities ]);
  plugins = pkgs.runCommand "sonic-macsec-cli-plugins" { } ''
    mkdir -p $out
    cp ${bi}/dockers/docker-macsec/cli/show/plugins/show_macsec.py $out/show_macsec.py
    cp ${bi}/dockers/docker-macsec/cli/clear/plugins/clear_macsec_counter.py $out/clear_macsec_counter.py
  '';
  wrap = name: module: entry: lib.hiPrio (pkgs.writeShellScriptBin name ''
    if [ "${"$"}{1:-}" = macsec ]; then
      shift
      exec ${cliEnv}/bin/python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); sys.argv[1:2] = []
    import ${module} as m; m.${entry}(prog_name="${name} macsec")' ${plugins} "$@"
    fi
    exec ${spy.utilities}/bin/${name} "$@"
  '');
  unitBase = {
    partOf = [ "sonic.target" ];
    unitConfig = { StartLimitIntervalSec = 300; StartLimitBurst = 10; };
  };
in {
  options.services.sonic.macsec = {
    enable = mkEnableOption "MACsec (macsecmgrd, wpa_supplicant with the macsec_sonic driver; needs a SAI with the MACsec API)";
    profiles = mkOption {
      default = { };
      description = "CONFIG_DB MACSEC_PROFILE entries; the CAK comes from a file outside the store.";
      type = types.attrsOf (types.submodule {
        options = {
          cipherSuite = mkOption {
            type = types.enum (lib.attrNames suites);
            default = "GCM-AES-XPN-256";
            description = "Cipher suite. The XPN suites (64-bit packet number) are the default: a 32-bit packet number lasts 29 s at 100G with 64-byte frames.";
          };
          cakFile = mkOption {
            type = types.str;
            example = "/var/lib/sonic-macsec/lab.cak";
            description = "File with the CAK as one line of hex (32 digits for the 128-bit suites, 64 for the 256-bit ones). Not a store path.";
          };
          ckn = mkOption {
            type = types.strMatching "([0-9a-fA-F]{2}){1,32}";
            description = "CKN (the CAK's name, sent in clear in every MKPDU), hex, up to 64 digits.";
          };
          priority = mkOption { type = types.ints.between 0 255; default = 255; description = "MKA key server priority (lower wins; 255 = never the key server unless nobody else is)."; };
          policy = mkOption { type = types.enum [ "security" "integrity_only" ]; default = "security"; description = "security = encrypt; integrity_only needs a SAI that can."; };
          rekeyPeriod = mkOption { type = types.ints.unsigned; default = 0; description = "Seconds between SAK changes started by the key server; 0 = only on packet number exhaustion."; };
          replayProtect = mkOption { type = types.bool; default = false; description = "Replay protection."; };
          replayWindow = mkOption { type = types.ints.unsigned; default = 0; description = "Replay window in frames (with replayProtect)."; };
        };
      });
    };
    ports = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = { "Ethernet1/1" = "lab"; };
      description = "Port (alias or SONiC name) -> profile.";
    };
    logLevel = mkOption { type = types.enum [ "NOTICE" "INFO" "DEBUG" ]; default = "NOTICE"; description = "macsecmgrd log level (swssloglevel)."; };
  };

  config = mkIf (cfg.enable && mcfg.enable) {
    assertions = lib.mapAttrsToList (p: prof: {
      assertion = mcfg.profiles ? ${prof};
      message = "services.sonic.macsec.ports.\"${p}\": no profile \"${prof}\"";
    }) mcfg.ports ++ lib.mapAttrsToList (n: p: {
      assertion = !lib.hasPrefix builtins.storeDir p.cakFile;
      message = "services.sonic.macsec.profiles.${n}.cakFile is a store path: the key would be world-readable";
    }) mcfg.profiles;

    environment.systemPackages = [ wpa (wrap "show" "show_macsec" "macsec") (wrap "sonic-clear" "clear_macsec_counter" "macsec") ];
    services.sonic.settings.FEATURE.macsec = lib.mapAttrs (_: mkDefault) {
      state = "enabled";
      has_global_scope = "False";
      has_per_asic_scope = "True";
      auto_restart = "enabled";
    };
    services.sonic.tableModes.FEATURE = mkDefault "seed";

    systemd.services = {
      sonic-orchagent.upholds = [ "sonic-macsecmgrd.service" ];
      # The declared profiles (with their keys) and port assignments -> CONFIG_DB.
      sonic-macsec-config = unitBase // {
        description = "SONiC MACsec profiles and port assignments from the NixOS configuration";
        wantedBy = [ "sonic.target" ];
        requires = [ "sonic-config-loader.service" ];
        after = [ "sonic-config-loader.service" ];
        before = [ "sonic-macsecmgrd.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${pyEnv}/bin/python3 -I ${./sonic-macsec-config.py} ${declared}";
        };
      };
      sonic-macsecmgrd = unitBase // {
        description = "SONiC macsecmgrd (CONFIG_DB MACSEC_PROFILE / PORT.macsec -> wpa_supplicant per port)";
        wantedBy = [ "sonic.target" ];
        bindsTo = [ "sonic-orchagent.service" ];
        wants = [ "sonic-macsec-config.service" ];
        after = [ "sonic-orchagent.service" "sonic-config-loader.service" "sonic-macsec-config.service" ];
        requires = [ "sonic-config-loader.service" ];
        serviceConfig = {
          # No waiting for addresses here: macsecmgrd itself starts a port's MKA session only when the netdev
          # has the port's CONFIG_DB INTERFACE mac_addr (if one is set) and starts it again when the address
          # changes (packages/sonic/swss/patches/0003). A unit-level wait cannot do that: at a stack restart
          # the previous syncd's netdevs still exist, with the right address, until the new syncd replaces them.
          ExecStart = "${cfg.packages.swss}/bin/macsecmgrd";
          # Stop: SIGTERM to macsecmgrd only. It ends each MKA session itself (wpa_cli interface_remove, then
          # SIGINT to that wpa_supplicant and wait), which needs the supplicants alive. With the default
          # (everything in the unit signalled at once) its wpa_cli calls ran into supplicants that were already
          # exiting and timed out: a stop of 15 s to more than a minute. What is left afterwards is killed.
          KillMode = "mixed";
          Restart = "always";
          RestartSec = 3;
          # macsecmgrd runs /sbin/wpa_supplicant and /sbin/wpa_cli with
          # /etc/wpa_supplicant.conf (compiled-in paths); give it those names
          # in its own mount namespace instead of patching swss for every board.
          TemporaryFileSystem = [ "/sbin" ];
          BindReadOnlyPaths = [
            "${wpa}/bin/wpa_supplicant:/sbin/wpa_supplicant"
            "${wpa}/bin/wpa_cli:/sbin/wpa_cli"
            "${bi}/dockers/docker-macsec/etc/wpa_supplicant.conf:/etc/wpa_supplicant.conf"
          ];
        };
      };
    };
  };
}

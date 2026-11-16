# NixOS module for consoled, the serial console server. Import this file, then e.g.
#
#   services.consoled = {
#     enable = true;
#     ports.P1 = { device = "/dev/P1"; baud = 115200; label = "cs8210"; };
#   };
#
# Users reach a port with `ssh -t console@<host> P1` / `... watch P1` / `ssh -t console@<host>` (menu)
# (sshd ForceCommand runs `line ssh`; keys: services.consoled.ssh.authorizedKeys, by default
# root's), or `line` on the box (menu, list, info, watch, attach, log); scripts and agents use
# `ssh console@<host> run P1 'uname -a'` (also send, expect, log --follow). sshd accepts
# CONSOLED_PREFIX from the client (`ssh -o SetEnv=CONSOLED_PREFIX=C-a`) for the attach keys.
# Scrollback lives in RAM only; nothing is logged to disk but attach/detach lines.
# Metrics: `line metrics` prints Prometheus text (use it as a textfile producer).
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption types;
  cfg = config.services.consoled;
  socket = "/run/consoled/console.sock";
  portType = types.submodule ({ name, ... }: {
    options = {
      device = mkOption { type = types.str; default = "/dev/${name}"; description = "Device node."; };
      label = mkOption { type = types.str; default = ""; description = "What is connected (also usable as the port name)."; };
      baud = mkOption { type = types.int; default = 9600; description = "Line speed."; };
      dataBits = mkOption { type = types.enum [ 5 6 7 8 ]; default = 8; };
      parity = mkOption { type = types.enum [ "none" "even" "odd" ]; default = "none"; };
      stopBits = mkOption { type = types.enum [ 1 2 ]; default = 1; };
      flow = mkOption { type = types.enum [ "none" "rtscts" "xonxoff" ]; default = "none"; };
    };
  });
  # Ports in natural order (P2 before P10).
  names = lib.sort (a: b:
    let na = lib.toInt (lib.concatStrings (builtins.filter (c: builtins.match "[0-9]" c != null) (lib.stringToCharacters a)) + "0");
        nb = lib.toInt (lib.concatStrings (builtins.filter (c: builtins.match "[0-9]" c != null) (lib.stringToCharacters b)) + "0");
    in if na == nb then a < b else na < nb) (builtins.attrNames cfg.ports);
  configJson = pkgs.writeText "consoled.json" (builtins.toJSON {
    inherit socket;
    inherit (cfg) scrollback;
    ports = map (n: let p = cfg.ports.${n}; in {
      name = n;
      inherit (p) device label baud parity flow;
      data_bits = p.dataBits;
      stop_bits = p.stopBits;
    }) names;
  });
  checker = if pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform then cfg.package else cfg.checkPackage;
  configFile = pkgs.runCommand "consoled.json" { nativeBuildInputs = [ checker ]; } ''
    consoled --config ${configJson} --check
    cp ${configJson} $out
  '';
in
{
  options.services.consoled = {
    enable = lib.mkEnableOption "consoled, the serial console server";
    package = mkOption {
      type = types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
    };
    checkPackage = mkOption {
      type = types.package;
      default = pkgs.buildPackages.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.buildPackages.callPackage ./package.nix { }";
      description = "consoled for the build machine (config check when cross compiling).";
    };
    ports = mkOption { type = types.attrsOf portType; default = { }; description = "Serial ports by name."; };
    scrollback = mkOption {
      type = types.ints.positive;
      default = 1024 * 1024;
      description = "Scrollback per port in bytes, RAM only (grows as the port talks).";
    };
    ssh = {
      enable = mkOption { type = types.bool; default = true; description = "The `console` ssh user (ForceCommand line ssh)."; };
      user = mkOption { type = types.str; default = "console"; };
      authorizedKeys = mkOption {
        type = types.listOf types.str;
        default = config.users.users.root.openssh.authorizedKeys.keys;
        defaultText = lib.literalExpression "config.users.users.root.openssh.authorizedKeys.keys";
        description = "Keys that may use the ports (all keys, all ports).";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    users.groups.consoled = { };
    users.users.consoled = { isSystemUser = true; group = "consoled"; extraGroups = [ "dialout" ]; };
    users.users.${cfg.ssh.user} = lib.mkIf cfg.ssh.enable {
      isSystemUser = true;
      group = "consoled";
      home = "/var/empty";
      shell = pkgs.runtimeShell; # sshd runs the ForceCommand through the login shell
      openssh.authorizedKeys.keys = cfg.ssh.authorizedKeys;
    };
    services.openssh.extraConfig = lib.mkIf cfg.ssh.enable ''
      Match User ${cfg.ssh.user}
        ForceCommand ${cfg.package}/bin/line ssh
        AllowTcpForwarding no
        AllowStreamLocalForwarding no
        X11Forwarding no
        AllowAgentForwarding no
        PermitTunnel no
        PermitTTY yes
        AcceptEnv CONSOLED_PREFIX
    '';
    environment.systemPackages = [ cfg.package ];

    systemd.services.consoled = {
      description = "consoled serial console server";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udev-settle.service" ];
      restartTriggers = [ configFile ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/consoled --config ${configFile}";
        User = "consoled";
        Group = "consoled";
        SupplementaryGroups = [ "dialout" ];
        RuntimeDirectory = "consoled";
        RuntimeDirectoryMode = "0750";
        UMask = "0007";
        Restart = "always";
        RestartSec = 2;
        # Sandbox: the ttys and its socket, nothing else.
        DeviceAllow = [ "char-ttyUSB rw" "char-ttyS rw" "char-ttyACM rw" "/dev/pts rw" ];
        DevicePolicy = "closed";
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateNetwork = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
      };
    };
  };
}

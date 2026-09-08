# A permanent glances dashboard on its own VT, switched to at boot so the
# console shows system state rather than a login prompt.
#
# tty7 rather than the megarepo's tty5 on purpose: logind spawns autovt@ttyN
# for any VT you switch to while N <= NAutoVTs (default 6), so a getty would
# race glances for the terminal. tty7 is outside that range and stays ours.
{ pkgs, lib, config, ... }:

let
  cfg = config.services.glancesTty;
in

{
  options.services.glancesTty = {
    enable = lib.mkEnableOption "a glances dashboard pinned to a virtual terminal";

    tty = lib.mkOption {
      type = lib.types.ints.positive;
      default = 7;
      description = "VT to run on. Keep it above services.logind NAutoVTs.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.glances-tty = {
      description = "glances on tty${toString cfg.tty}";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-logind.service" ];

      serviceConfig = {
        ExecStart = "${pkgs.glances}/bin/glances --disable-check-update";
        # chvt makes this the view you actually see; "+" runs it with full
        # privileges despite DynamicUser.
        ExecStartPost = "+${pkgs.kbd}/bin/chvt ${toString cfg.tty}";
        Restart = "always";
        RestartSec = "5s";
        # Unprivileged: the dashboard is readable by anyone at the console, so
        # it must not be a root shell's worth of authority.
        DynamicUser = true;
        RuntimeDirectory = "glances-tty";
        TTYPath = "/dev/tty${toString cfg.tty}";
        TTYReset = true;
        TTYVTDisallocate = true;
        StandardInput = "null";
        StandardOutput = "tty";
        StandardError = "journal";
      };

      environment = {
        TERM = "xterm";
        HOME = "/run/glances-tty";
      };
    };

    environment.systemPackages = [ pkgs.glances ];
  };
}

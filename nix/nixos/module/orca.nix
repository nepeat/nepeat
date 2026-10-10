# Orca (stablyai/orca) as a headless Remote Orca Server.
#
# `orca serve` owns the whole runtime -- repos, worktrees, terminals, agent
# sessions -- and the desktop/mobile apps pair to it as thin clients. State
# lives in erin's home (Electron userData), so paired-client grants survive
# restarts as long as /home does.
#
# Reachable over Tailscale only: upstream is explicit that the port must never
# face the internet, so it is opened on tailscale0 and nowhere else, and the
# advertised pairing address is looked up from tailscaled at start rather than
# hardcoded (chickennugget's closure is not host-specific).
#
# Pairing: `journalctl -u orca` shows the pairing URL printed at startup. Treat
# it like a password. For the phone, stop the unit and run
# `orca serve --pairing-address $(tailscale ip -4) --mobile-pairing` by hand
# once to get the QR code.
{ inputs, lib, pkgs, ... }:

let
  orca = inputs.llm-agents.packages.${pkgs.stdenv.hostPlatform.system}.orca;
  port = 6768;
in
{
  # Provides `orca` (CLI shim) and `orca-ide` (the Electron app). The Linux
  # docs say to start the server as `orca-ide serve` because GNOME's screen
  # reader owns /usr/bin/orca; in this package `orca-ide` is the GUI binary
  # and `orca` is the CLI, so it is `orca serve` here.
  environment.systemPackages = [ orca ];

  networking.firewall.interfaces.tailscale0.allowedTCPPorts = [ port ];

  systemd.services.orca = {
    description = "Orca headless runtime server";
    wantedBy = [ "multi-user.target" ];
    wants = [ "network-online.target" ];
    after = [
      "network-online.target"
      "tailscaled.service"
    ];

    # Agents and terminals run with the server's PATH, not the client's, so
    # give it erin's home-manager profile (claude-code, codex, opencode) plus
    # the system profile and setuid wrappers.
    environment.PATH = lib.mkForce (
      lib.concatStringsSep ":" [
        "/run/wrappers/bin"
        "/etc/profiles/per-user/erin/bin"
        "/run/current-system/sw/bin"
      ]
    );

    script = ''
      # Fails (and the unit retries) until `tailscale up` has been run.
      addr=$(${pkgs.tailscale}/bin/tailscale ip -4)
      exec ${orca}/bin/orca serve --port ${toString port} --pairing-address "$addr"
    '';

    serviceConfig = {
      User = "erin";
      Group = "users";
      WorkingDirectory = "~";
      Restart = "on-failure";
      RestartSec = 10;
    };
  };
}

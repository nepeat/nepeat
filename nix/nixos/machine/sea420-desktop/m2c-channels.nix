# The mpeg2cuda test channels (aislop/mpeg2cuda) streamed to the EQAM-420B,
# one supervised sender per channel so a crash or a network blip only costs
# that channel a few seconds.
#
# Each unit runs `scripts/channels.sh run <ch> <eqam>` straight out of the
# checkout: lineup.tsv, the sources under media/ and the FFmpeg build
# (build/ffmpeg) all live there and change while the project is worked on.
# Restart=always with no start limit keeps a unit retrying forever, e.g.
# through a long network outage or while the GPU driver is unavailable.
#
#   sudo systemctl stop m2c-channels.target      # all senders
#   sudo systemctl restart m2c-channel-906       # one
#   aislop/mpeg2cuda/scripts/channels.sh status  # speed per channel
{ lib, pkgs, ... }:

let
  repo = "/home/erin/nepeat/aislop/mpeg2cuda";
  # EQAM GigE data port (renumbered from 10.60.30.47 on 2026-10-20); its slots
  # expect UDP from this host on 5900 + ch - 900
  eqam = "10.158.117.47";
  # must match media/channels/lineup.tsv and the EQAM slot plan
  # (scripts/eqam_channels.py)
  channels = [ 900 901 902 903 904 905 906 907 908 ];

  sender = ch: lib.nameValuePair "m2c-channel-${toString ch}" {
    description = "mpeg2cuda test channel ${toString ch} to the EQAM";
    wantedBy = [ "m2c-channels.target" ];
    partOf = [ "m2c-channels.target" ];
    wants = [ "network-online.target" ];
    after = [ "network-online.target" ];
    # bash for the script, fontconfig for its drawtext font fallback
    path = with pkgs; [ bash coreutils gnugrep findutils fontconfig ];
    environment.PORT_BASE = "5900";
    unitConfig = {
      RequiresMountsFor = repo;
      StartLimitIntervalSec = 0;
    };
    serviceConfig = {
      User = "erin";
      Group = "users";
      ExecStart = "${repo}/scripts/channels.sh run ${toString ch} ${eqam}";
      Restart = "always";
      RestartSec = 5;
      # a global OOM (big nix builds, no swap) should pick a compiler first
      OOMScoreAdjust = -500;
    };
  };
in
{
  systemd.targets.m2c-channels = {
    description = "mpeg2cuda test channels to the EQAM";
    wantedBy = [ "multi-user.target" ];
  };

  systemd.services = lib.listToAttrs (map sender channels);
}

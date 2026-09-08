# Remote build machines, mirroring what base/darwin.nix and machine/m1laptop.nix
# already give the Macs. Worth having on chickennugget in particular: it is a
# 4-core Ryzen 3550H, so anything that cannot be substituted is slow locally.
{ lib, config, ... }:

let
  cfg = config.nix.remoteBuilders;
in

{
  options.nix.remoteBuilders = {
    enable = lib.mkEnableOption "offloading builds to the lab build machines" // {
      default = true;
    };

    sshKey = lib.mkOption {
      type = lib.types.str;
      default = "/root/.ssh/id_ed25519";
      description = ''
        Private key root uses to reach the builders. NOT managed here -- it has
        to exist on the host, and its public half must be authorized on each
        builder. Until then nix simply falls back to building locally.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    nix.distributedBuilds = true;

    # Let builders fetch from substituters themselves rather than round-tripping
    # every dependency through us -- the whole point on a slow uplink.
    nix.settings.builders-use-substitutes = true;

    nix.buildMachines = [
      {
        hostName = "10g.warc.zip";
        sshUser = "erin";
        sshKey = cfg.sshKey;
        system = "x86_64-linux";
        protocol = "ssh-ng";
        maxJobs = 8;
        supportedFeatures = [ "nixos-test" "benchmark" "big-parallel" ];
        mandatoryFeatures = [ ];
      }
      {
        hostName = "dreamflasher.skate-gopher.ts.net";
        sshUser = "root";
        sshKey = cfg.sshKey;
        system = "aarch64-linux";
        protocol = "ssh-ng";
        maxJobs = 8;
        supportedFeatures = [ "nixos-test" "benchmark" "big-parallel" ];
        mandatoryFeatures = [ ];
      }
    ];
  };
}

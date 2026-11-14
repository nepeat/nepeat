# Remote build machines, mirroring what base/darwin.nix and machine/m1laptop.nix
# already give the Macs. Worth having on chickennugget in particular: it is a
# 4-core Ryzen 3550H, so anything that cannot be substituted is slow locally.
{ lib, config, ... }:

let
  cfg = config.nix.remoteBuilders;
  keys = import ./remote-builder-keys.nix;
  desktop = {
    inherit (keys.sea420-desktop) hostName publicHostKey;
    sshUser = "nix-ssh";
    sshKey = cfg.desktopKey;
    protocol = "ssh-ng";
  };
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

    desktopKey = lib.mkOption {
      type = lib.types.str;
      default = "/root/.ssh/nix-remote-builder";
      description = ''
        Private key for sea420-desktop's nix-ssh builder user (see
        remote-builder-host.nix). Copied onto each client by hand; the public
        half is in remote-builder-keys.nix.
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
    ]
    # sea420-desktop (Ryzen 9 7950X, 32 threads): native x86, plus aarch64/POWER
    # via qemu-user binfmt -- which on this box still outruns the small native
    # ARM/PPC machines, so it is preferred for those too.
    # Not on the desktop itself, which would otherwise ssh to itself.
    ++ lib.optionals (!config.nix.remoteBuilderHost.enable) [
      (desktop // {
        systems = [ "x86_64-linux" "i686-linux" ];
        maxJobs = 16;
        speedFactor = 4;
        supportedFeatures = [ "nixos-test" "benchmark" "big-parallel" "kvm" ];
        mandatoryFeatures = [ ];
      })
      (desktop // {
        systems = [ "aarch64-linux" "powerpc64-linux" "powerpc64le-linux" ];
        maxJobs = 16;
        speedFactor = 4;
        supportedFeatures = [ "benchmark" "big-parallel" ];
        mandatoryFeatures = [ ];
      })
    ];
  };
}

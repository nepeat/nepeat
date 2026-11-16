# On top of profile-appliance: a full, rebuildable Nix system. nix + the
# switcher on the box, so the workstation can deploy in place
#   nixos-rebuild switch --flake path:./nix#<host> --target-host root@<box>
# (built on the workstation, closure copied over ssh, activated on the box),
# with real generations and `nixos-rebuild switch --rollback` / the boot menu.
# Sized for small flash: no nixpkgs source, channel or registry in the closure,
# no builds or store optimising on the box, idle-priority daemon, bounded GC.
{ config, lib, ... }:
{
  nix.enable = lib.mkForce true;
  system.switch.enable = lib.mkForce true;
  system.tools.nixos-rebuild.enable = lib.mkForce true;   # --rollback, list-generations on the box
  # Installer tools come with nix.enable; the box never installs or generates configs
  # (nixos-generate-config alone drags in bcachefs-tools).
  system.tools.nixos-generate-config.enable = false;
  system.tools.nixos-install.enable = false;
  system.tools.nixos-enter.enable = false;
  system.tools.nixos-build-vms.enable = false;

  # The box never evaluates nixpkgs: keep its source (~150 MiB) out.
  nix.channel.enable = false;
  nixpkgs.flake.setNixPath = false;
  nixpkgs.flake.setFlakeRegistry = false;

  nix.settings = {
    trusted-users = [ "root" ];
    max-jobs = 0;                  # never build here; substitutes and copies still work
    auto-optimise-store = false;   # no hard-linking pass on every copy (writes)
    keep-derivations = false;      # the box only needs outputs
    keep-outputs = false;
    # Auto-GC for builds/substitutions on the box only; pushes from the workstation
    # (nixos-rebuild --target-host) don't trigger it, so keep an eye on df before big ones.
    min-free = 512 * 1024 * 1024;
    max-free = 2048 * 1024 * 1024;
  };
  nix.daemonCPUSchedPolicy = "idle";
  nix.daemonIOSchedClass = "idle";
  # Keep exactly the system generations the boot menu lists (generic-extlinux-compatible's
  # configurationLimit, 10 here by default), then collect: deleting by age could remove a
  # generation whose menu entry is still there.
  nix.gc = {
    automatic = true;
    dates = "weekly";
    randomizedDelaySec = "1h";
    options = "";
  };
  systemd.services.nix-gc.preStart = ''
    ${config.nix.package.out}/bin/nix-env -p /nix/var/nix/profiles/system \
      --delete-generations +${toString (config.boot.loader.generic-extlinux-compatible.configurationLimit or 10)}
  '';
}

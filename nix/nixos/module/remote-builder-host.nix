# Serve Nix builds to the rest of the fleet over ssh-ng: native x86_64/i686,
# plus aarch64 and 64-bit POWER (big + little endian) through qemu-user binfmt.
# Clients pick this up from remote-builders.nix.
#
# Builds arrive as the locked-down nix-ssh user (nix.sshServe: ForceCommand
# nix-daemon --stdio, no shell). It has to be trusted for remote building, so
# whoever holds the client key can put arbitrary paths into this store -- treat
# that key like a root credential for the store.
{ config, lib, ... }:

let
  cfg = config.nix.remoteBuilderHost;
  keys = import ./remote-builder-keys.nix;
in

{
  options.nix.remoteBuilderHost = {
    enable = lib.mkEnableOption "serving remote Nix builds to the fleet";

    emulatedSystems = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "aarch64-linux" "powerpc64-linux" "powerpc64le-linux" ];
      description = "Foreign systems built through qemu-user binfmt.";
    };

    clientKeys = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ keys.clientKey ];
      description = "Public keys allowed to submit builds as nix-ssh.";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.binfmt.emulatedSystems = cfg.emulatedSystems;
    # Static emulators + fix-binary: the interpreter is opened at registration,
    # so it works inside the build sandbox without extra-sandbox-paths games.
    boot.binfmt.preferStaticEmulators = true;
    boot.binfmt.registrations = lib.genAttrs cfg.emulatedSystems (_: { fixBinary = true; });

    nix.sshServe = {
      enable = true;
      protocol = "ssh-ng";
      write = true;
      trusted = true;
      keys = cfg.clientKeys;
    };  };
}

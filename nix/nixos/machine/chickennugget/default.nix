# chickennugget -- the mini desktop thin client. Owns no local storage it is
# allowed to touch: the bootloader (iPXE, driven by aislop/ipxe_iscsi_mgr)
# attaches an iSCSI LUN and publishes an iBFT, and everything else follows
# from that.
#
# The closure is deliberately not host-specific. Initiator IQN, target, portal
# and IP all come from firmware at boot, so the same system image can be booted
# by any host the boot manager has assigned a volume to.
{ inputs, pkgs, ... }:

{
  system.stateVersion = "26.05";

  imports = [
    ../../module/iscsi-ibft.nix
    ../../module/iscsi-xfs.nix
    ../../module/glances-tty.nix
    ../../module/erin.nix

    inputs.paseo.nixosModules.paseo

    ../../module/vault-agent.nix
    ../../module/agent-secrets.nix
  ];

  # Machine secrets, same AppRole shape as the megarepo fleet. The role_id /
  # secret_id pair is seeded out of band -- `just vault-provision` at install
  # time, `just vault-rekey` afterwards -- and never lives in the closure.
  vaultAgent.enable = true;
  agentSecrets.enable = true;

  # Enabled but deliberately not joined: `tailscale up` is run by hand. State
  # lives in /var/lib/tailscale, which is on the persistent XFS root here (no
  # impermanence on this host), so the join survives reboots.
  services.tailscale.enable = true;

  # Paseo -- deliberately NOT on the desktop, this host only.
  services.paseo = {
    enable = true;

    # Runs as erin, not the `paseo` system user. The daemon spawns coding
    # agents, and as a system user its PATH is only coreutils/grep/sed/systemd
    # -- no codex, claude, opencode, git or ssh -- and it cannot read erin's
    # credentials. Setting user here also flips `inheritUserEnvironment` on by
    # itself (it defaults to `user != "paseo"`), which puts
    # /etc/profiles/per-user/erin/bin on the service PATH.
    #
    # Consequence: dataDir moves to /home/erin/.paseo, so the daemon keypair
    # and server-id are regenerated; the old /var/lib/paseo is orphaned.
    user = "erin";
    group = "users";

    # Hosted relay (app.paseo.sh) for remote access.
    relay.enable = true;

    # The web UI defaults to off, but "default off" is not the same as
    # "cannot be turned on": the daemon resolves it as
    #   cli ?? env PASEO_WEB_UI_ENABLED ?? persisted features.webUi.enabled ?? false
    # so anything writing config.json could flip it. env outranks the persisted
    # value, so pin both.
    environment.PASEO_WEB_UI_ENABLED = "false";
    settings.features.webUi.enabled = false;

    # Still loopback-only. Remote access is via the relay, so there is no
    # reason to publish the port on the LAN as well.
  };

  # `paseo` on PATH for interactive use; the daemon gets its own copy via
  # services.paseo.package.
  environment.systemPackages = [ inputs.paseo.packages.${pkgs.stdenv.hostPlatform.system}.default ];

  hardware.iscsiRoot.enable = true;

  # Diskless box with no desk in front of it: the console is more useful as a
  # dashboard than as a login prompt.
  services.glancesTty.enable = true;

  # /dev/sda is correct because the iSCSI LUN is the only SCSI device here; the
  # UM350's local NVMe shows up as nvme0n1 and disko never looks at it. Confirm
  # with `lsblk -o NAME,TRAN` (TRAN=iscsi) if a machine ever grows a second
  # SCSI disk -- the NVMe's contents are expendable, but the ordering is not.
  disko.devices.disk.iscsi0.device = "/dev/sda";

  # The LUN's ESP is the only boot medium. systemd-boot installs both
  # \EFI\systemd\systemd-bootx64.efi and the removable-media fallback
  # \EFI\BOOT\BOOTX64.EFI, and the fallback is the first filename iPXE's
  # sanboot sweep tries -- so this boots with no per-host bootloader setup.
  boot.loader.systemd-boot.enable = true;
  # Nothing writes NVRAM here: the machine is pointed at its disk by the
  # bootloader over the network, not by an EFI boot entry.
  boot.loader.efi.canTouchEfiVariables = false;

  # Kept out on purpose: impermanence (the root filesystem is already
  # disposable -- a volume the manager can reassign or re-image) and gitops
  # (a generic image has no single machine to track a branch for).
}

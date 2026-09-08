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
    ../../module/paseo.nix

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

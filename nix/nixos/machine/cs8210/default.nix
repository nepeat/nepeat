# cs8210 -- Celestica CS8210-32X switch running NixOS on its Alpine v3 CPU card.
#
# Bring-up stage: the goal is boot + console, then management Ethernet, PCIe
# (switch ASIC + FPGAs), TPM. Nothing here drives the front-panel ports.
#
# Install is a raw image, not an installer: build
#   nix build .#nixosConfigurations.cs8210.config.system.build.sdImage
# and dd it onto the M.2 SSD (/dev/sda) from ONIE. Partition 1 is the unused
# "firmware" FAT stub the sd-image module always creates, partition 2 is the
# ext4 root, grown to the whole disk on first boot. See module/cs8210.
{
  lib,
  modulesPath,
  config,
  ...
}:

{
  system.stateVersion = "26.05";

  imports = [
    (modulesPath + "/installer/sd-card/sd-image.nix")
    ../../module/cs8210
  ];

  # Kernel + al_eth are cross-built from the x86 desktop; the only aarch64
  # builder otherwise is binfmt emulation.
  hardware.cs8210.kernelBuildSystem = "x86_64-linux";

  image.baseName = "nixos-cs8210";

  # Cage 17 holds a QSA with a 10G SFP+ DAC to the lab aggregation switch
  # (2026-11-10). 4x10G breakout; the QSA carries lane 1 (the first of the
  # four logical ports on cage 17). Everything else stays 1x400G.
  hardware.cs8210.vendorSdk.innoviumPorts =
    "1-16:copper_1x400G:fec-def;17:copper_4x10G:fec-def;18-32:optics_1x400G:fec-def";

  sdImage = {
    compressImage = false;
    firmwareSize = 16;
    populateFirmwareCommands = "";
    populateRootCommands = ''
      mkdir -p ./files/boot
      CS8210_BOOT_TARGET=./files/boot CS8210_PROFILES_DIR=/nonexistent \
        ${config.system.build.installBootLoader} ${config.system.build.toplevel}
    '';
  };

  # One LACP bond over the management port and every front-panel port.
  # systemd-networkd, not scripted networking: fpp* only exist while
  # cs8210-switch runs (and are re-created on every SDK restart), and
  # networkd enslaves them whenever they appear. inno_* (SDK-internal) and
  # enp0s3 (the CPU card's link to ASIC devport 129) stay unmanaged.
  #
  # Links that negotiate with the same partner form one aggregator. Links
  # with different speeds or partners form separate aggregators, and
  # ad_select=bandwidth keeps the fastest one active. So with the SDK up,
  # front-panel links carry traffic; with it down (or wedged and its links
  # dropped), the bond falls back to enp0s1. A link with no LACP partner
  # still works as an individual one-link aggregator.
  #
  # Bond MAC: the ASIC only traps unicast to the CPU for MACs the SDK
  # programmed as its own: the base MAC passed to auto_load_user.sh -a (ONIE
  # MAC#1Base with the first byte + 2, see cs8210-switch-up) and the per-port
  # fpp MACs (base + n). Unicast to any other MAC (e.g. a random bond MAC) is
  # switched or dropped by the ASIC and never reaches Linux; broadcast and
  # multicast still do, so DHCP looks fine while ARP/ping fail (2026-11-10).
  # The base MAC works on the front panel (tested) and on enp0s1. ipd's
  # set_mac is software-only, so the bonding MAC rewrite of fpp* is harmless.
  # DHCP client id = MAC, so the lease follows this MAC on whichever
  # aggregator is active.
  networking.useDHCP = false;
  networking.useNetworkd = true;
  systemd.network = {
    enable = true;
    # Nothing here should hold up boot for a cable.
    wait-online.enable = false;
    netdevs."10-bond0" = {
      netdevConfig = {
        Kind = "bond";
        Name = "bond0";
        MACAddress = "0e:48:c6:7a:55:ce";
      };
      bondConfig = {
        Mode = "802.3ad";
        LACPTransmitRate = "fast";
        AdSelect = "bandwidth";
        TransmitHashPolicy = "layer3+4";
        MIIMonitorSec = "100ms";
      };
    };
    networks."20-bond0-members" = {
      matchConfig.Name = "enp0s1 fpp*";
      networkConfig.Bond = "bond0";
    };
    networks."30-bond0" = {
      matchConfig.Name = "bond0";
      networkConfig = {
        DHCP = "yes";
        IPv6AcceptRA = true;
        LinkLocalAddressing = "ipv6";
      };
      dhcpV4Config.ClientIdentifier = "mac";
      linkConfig.RequiredForOnline = "carrier";
    };
  };

  # Bring-up convenience: the serial console is the only way in until al_eth
  # works, and every account here is SSH-key-only. Physical access to the
  # console port already means U-Boot access, so this gives away nothing new.
  services.getty.autologinUser = "root";

  # Nothing on this box wants these from the shared base.
  programs.nix-index.enable = lib.mkForce false;
  programs.nix-index-database.comma.enable = lib.mkForce false;
  documentation.nixos.enable = false;

  # lldpd bundles an old copy of linux/types.h that shadows the system one and
  # has no __u128. arm64's asm/sigcontext.h in current kernel headers needs
  # it, so lldpd 1.0.22 fails to compile for aarch64 (x86 is unaffected, and
  # there is no cached aarch64 build on nixpkgs master). Add the typedefs the
  # real header has. Drop this once nixpkgs/lldpd carry a fix.
  nixpkgs.overlays = [
    (final: prev: {
      lldpd = prev.lldpd.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./lldpd-bundled-types-u128.patch ];
      });
    })
  ];

  # Root logins: the keys published at https://github.com/nepeat.keys (as
  # fetched 2026-11-09) plus root on sea420-desktop, the build/deploy host.
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
  ];
}

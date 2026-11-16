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
    ../../module/cs8210/sonic
  ];

  # Kernel + al_eth are cross-built from the x86 desktop; the only aarch64
  # builder otherwise is binfmt emulation.
  hardware.cs8210.kernelBuildSystem = "x86_64-linux";

  image.baseName = "nixos-cs8210";

  # Cage 2 holds an FS QSFP28->4xSFP28 breakout DAC to the lab aggregation
  # switch (2026-11-11; it was in cage 17 before). 4x10G on the ASIC *and* on
  # the Credo PHY in front of cage 2 (Credo still in MACsec bypass, see
  # cs8210-switch-up). Everything else stays 1x400G.
  hardware.cs8210.vendorSdk.innoviumPorts =
    "1:copper_1x400G:fec-def;2:copper_4x10G:fec-def;3-16:copper_1x400G:fec-def;17-32:optics_1x400G:fec-def";
  hardware.cs8210.vendorSdk.credoPorts = "1:optics_1x400G;2:copper_4x10G;3-32:optics_1x400G";

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

  # Networking with systemd-networkd (fpp* only exist while cs8210-switch
  # runs and are re-created on every SDK restart; networkd picks them up
  # whenever they appear, scripted networking/dhcpcd cannot).
  #   * enp0s1 (management): plain DHCP on its own MAC, as before.
  #   * bond0: an 802.3ad test LAG over front-panel cage 2 only (fpp2 in
  #     1-port mode, fpp2-1..4 as a 4x10G breakout; see innoviumPorts).
  #     A link with no LACP partner still works as an individual link.
  #   * Every other fpp*, inno_* (SDK-internal) and enp0s3 (the CPU card's
  #     link to ASIC devport 129) stay unmanaged.
  #
  # Bond MAC: the ASIC only traps unicast to the CPU for MACs the SDK
  # programmed as its own: the base MAC passed to auto_load_user.sh -a (ONIE
  # MAC#1Base with the first byte + 2, see cs8210-switch-up) and the per-port
  # fpp MACs (base + n). Unicast to any other MAC (e.g. a random bond MAC) is
  # switched or dropped by the ASIC and never reaches Linux; broadcast and
  # multicast still do, so DHCP looks fine while ARP/ping fail (2026-11-10).
  # ipd's set_mac is software-only, so the bonding MAC rewrite of the members is
  # harmless.
  networking.useDHCP = false;
  networking.useNetworkd = true;
  systemd.network = lib.mkMerge [ {
    enable = true;
    # Nothing here should hold up boot for a cable.
    wait-online.enable = false;
    networks."10-enp0s1" = {
      matchConfig.Name = "enp0s1";
      networkConfig = {
        DHCP = "yes";
        IPv6AcceptRA = true;
      };
      dhcpV4Config.ClientIdentifier = "mac";
    };
  }
  # The vendor stack's front-panel test path; not with SONiC (no fpp*
  # netdevs, and SONiC's vrfmgrd deletes VRF devices it did not create).
  (lib.mkIf (!config.hardware.cs8210.sonic.enable) {
    # bond0 lives in its own VRF so the front-panel test path has its own
    # routing table (its DHCP default route) and is not answered through
    # enp0s1's default route. tcp/udp_l3mdev_accept (below) lets sshd and
    # other default-VRF listeners accept connections arriving in the VRF.
    # Run clients in it with `ip vrf exec vrf-fp <cmd>`.
    netdevs."15-vrf-fp" = {
      netdevConfig = {
        Kind = "vrf";
        Name = "vrf-fp";
      };
      vrfConfig.Table = 100;
    };
    networks."15-vrf-fp" = {
      matchConfig.Name = "vrf-fp";
      linkConfig.RequiredForOnline = "no";
    };
    netdevs."20-bond0" = {
      netdevConfig = {
        Kind = "bond";
        Name = "bond0";
        MACAddress = "0e:48:c6:7a:55:ce";
      };
      bondConfig = {
        Mode = "802.3ad";
        LACPTransmitRate = "fast";
        TransmitHashPolicy = "layer3+4";
        MIIMonitorSec = "100ms";
      };
    };
    networks."20-bond0-members" = {
      matchConfig.Name = "fpp2 fpp2-*";
      networkConfig.Bond = "bond0";
      # Members must carry the bond MAC (else replies are PACKET_OTHERHOST
      # and dropped). This pin alone is not enough: the SDK overwrites the
      # MAC ~50 ms after networkd enslaves the new fpp netdev, so
      # cs8210-switch-up (sync_bond_macs) puts it back once the SDK is up.
      linkConfig.MACAddress = "0e:48:c6:7a:55:ce";
    };
    networks."30-bond0" = {
      matchConfig.Name = "bond0";
      networkConfig = {
        VRF = "vrf-fp";
        DHCP = "yes";
        IPv6AcceptRA = true;
        LinkLocalAddressing = "ipv6";
      };
      dhcpV4Config.ClientIdentifier = "mac";
    };
  }) ];

  boot.kernel.sysctl = {
    "net.ipv4.tcp_l3mdev_accept" = 1;
    "net.ipv4.udp_l3mdev_accept" = 1;
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

  # MACsec shim + cs8210-macsec + MKA driver (macsec.md); ports stay in bypass until configured.
  # Its units run with the vendor stack (cs8210-switch) only.
  hardware.cs8210.macsec.enable = true;

  # SONiC on our sai-teralynx (nix/nixos/module/cs8210/sonic), started at boot
  # (2026-11-16). The vendor stack stays installed: systemctl start
  # cs8210-switch stops SONiC's syncd (Conflicts=). Only cages 1 and 16 hold
  # modules, a 400G CR8 DAC loop between them (allowedCages: syncd refuses to
  # start if any other cage is populated).
  hardware.cs8210.sonic = {
    enable = true;
    macAddress = "0e:48:c6:7a:55:ce";
    lineMedia = { "1" = "copper"; "16" = "copper"; };
  };
  # Nix base for CONFIG_DB (seeded: config + config save win): the two loop
  # ports up (RS FEC: 400G PAM4), the other 30 stay down. Routing, VRFs and
  # QoS are the saved file's (ai_re/devices/cs8210-32x/sonic-loop/).
  services.sonic.settings.PORT = lib.genAttrs [ "Ethernet0" "Ethernet120" ] (_: {
    admin_status = "up";
    fec = "rs";
  });
}

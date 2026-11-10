# hardware.cs8210.vendorSdk: glue for running the vendor switch SDK on mainline.
#
# What is ours (GPL, in this tree):
#   * the ipd kernel driver (Marvell's published source, built in ../kernel.nix)
#   * libmdio/: a drop-in libMdio.so that drives the MDIO FPGA from userspace,
#     replacing the vendor's 4.9-only mdio-fpga / mdio-lattice / cms50216 stack
#   * cs8210-sdk-chroot: enters the vendor userland with the right mounts
#   * cs8210-switch-up / -down: start the SDK (knet netdevs fpp1..32) and
#     the Credo PHY bring-up in MACsec bypass
#   * cs8210-switch-power off|on|status: data plane to sleep (Credo PHYs and
#     ASIC held in reset), control plane keeps running
#
# What is NOT in this tree: the vendor userland itself (ONIE root filesystem
# with glibc 2.25 + python2, the Innovium SDK, Credo firmware and tools).
# It has to be placed at /var/lib/cs8210-sdk/root on the switch. The copy
# installed in this unit's SPI flash is the initramfs of the ONIE FIT image
# (mtd "onie"), with the SDK at /root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.cs8210.vendorSdk;

  # Only GLIBC_2.17 symbols are referenced (checked with readelf -V), so
  # the library loads into the vendor's glibc 2.25 processes. Keep it that
  # way: no fortify, no stack protector, nothing newer from libc.
  libmdio = pkgs.stdenv.mkDerivation {
    pname = "cs8210-libmdio";
    version = "1";
    src = ./libmdio;
    hardeningDisable = [ "all" ];
    dontConfigure = true;
    buildPhase = ''
      $CC -std=gnu11 -O2 -Wall -Wextra -fPIC -shared -U_FORTIFY_SOURCE \
        -fno-stack-protector -Wl,-soname,libMdio.so -o libMdio.so libMdio.c -lpthread
      if $READELF -V libMdio.so | grep -o 'GLIBC_[0-9.]*' | sort -u | grep -v '^GLIBC_2.17$'; then
        echo "libMdio.so references glibc symbols newer than 2.17" >&2
        exit 1
      fi
    '';
    installPhase = ''
      install -D -m 0755 libMdio.so $out/lib/libMdio.so
    '';
  };

  chroot = pkgs.replaceVarsWith {
    src = ./cs8210-sdk-chroot;
    name = "cs8210-sdk-chroot";
    dir = "bin";
    isExecutable = true;
    replacements.libmdio = "${libmdio}/lib/libMdio.so";
  };
  switchUp =
    pkgs.runCommand "cs8210-switch-up"
      {
        nativeBuildInputs = [ pkgs.makeWrapper ];
      }
      ''
        install -D -m 0755 ${./cs8210-switch-up} $out/libexec/cs8210-switch-up
        patchShebangs $out/libexec
        makeWrapper $out/libexec/cs8210-switch-up $out/bin/cs8210-switch-up \
          --prefix PATH : ${lib.makeBinPath [ pkgs.procps pkgs.gnused pkgs.coreutils ]}
        makeWrapper $out/libexec/cs8210-switch-up $out/bin/cs8210-switch-down \
          --add-flags --down \
          --prefix PATH : ${lib.makeBinPath [ pkgs.procps pkgs.gnused pkgs.coreutils ]}
      '';
  pyEnv = pkgs.python3;
  switchPower =
    pkgs.runCommand "cs8210-switch-power"
      {
        nativeBuildInputs = [ pkgs.makeWrapper ];
      }
      ''
        install -D -m 0755 ${./cs8210-credo-reset.py} $out/libexec/cs8210-credo-reset
        install -D -m 0755 ${./cs8210-switch-power} $out/libexec/cs8210-switch-power
        sed -i '1s|.*|#!${pyEnv}/bin/python3|' $out/libexec/cs8210-credo-reset
        patchShebangs $out/libexec/cs8210-switch-power
        makeWrapper $out/libexec/cs8210-credo-reset $out/bin/cs8210-credo-reset
        makeWrapper $out/libexec/cs8210-switch-power $out/bin/cs8210-switch-power \
          --prefix PATH : ${lib.makeBinPath [ pkgs.i2c-tools pkgs.gawk pkgs.coreutils pkgs.systemd ]}:$out/bin
      '';
in
{
  options.hardware.cs8210.vendorSdk = {
    enable = lib.mkEnableOption "kernel driver and glue for running the vendor Innovium/Credo SDK userland";
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      chroot
      switchUp
      switchPower
    ];

    # Not started at boot on purpose while this is experimental:
    #   systemctl start cs8210-switch
    # Note: once the ASIC is initialised the system CPLD forces the fans to
    # full speed (reg 0x05 bits 7/5 clear) and there is no ASIC die
    # temperature reading on mainline yet, so leave that alone.
    systemd.services.cs8210-switch = {
      description = "CS8210 switch ASIC + Credo PHY bring-up (vendor SDK in chroot)";
      after = [ "systemd-modules-load.service" "network-pre.target" ];
      path = [ chroot switchUp config.system.path ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${switchUp}/bin/cs8210-switch-up 32x400";
        ExecStop = "${switchUp}/bin/cs8210-switch-down";
        TimeoutStartSec = "10min";
        # cs8210-switch-down asks both daemons to exit, waits up to 20 s and
        # then kills them; whatever is still in the cgroup after that is
        # removed by systemd.
        TimeoutStopSec = "90s";
      };
    };

    # The vendor SDK on this box is from 2020 and speaks the ipd 0.12.x ioctl
    # layouts; see kernel/patches/ipd-legacy-abi.patch.
    boot.extraModprobeConfig = ''
      options ipd legacy_abi=1
    '';

    # inno_0_0129 / inno_0_1000 are SDK-internal netdevs, not ports. Anything
    # the host sends on them comes straight back on CPU RX ring 3 without a
    # switch header, which ipd reports as "Unsupported VF 3 / Invalid LDH".
    # With IPv6 link-local autoconfiguration that is a steady trickle of MLD,
    # NS and RS packets (the stock ONIE never sends any). Keep the host quiet
    # there: no IPv6, and lldpd only on real ports.
    services.udev.extraRules = ''
      SUBSYSTEM=="net", ACTION=="add", KERNEL=="inno_*", RUN+="${pkgs.procps}/bin/sysctl -q -w net.ipv6.conf.%k.disable_ipv6=1"
    '';
    services.lldpd.extraArgs = [ "-I" "enp0s1,fpp*" ];

    systemd.tmpfiles.rules = [ "d /var/lib/cs8210-sdk 0700 root root - -" ];
  };
}

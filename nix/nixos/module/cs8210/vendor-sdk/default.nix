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
  # The SDK writes its own log files into its /tmp and never rotates them.
  # On 2026-11-10 a wedged innovium.user logged "Learn WB w/o expected SOP"
  # at ~25 MB/s and filled the 1.9 GB /run tmpfs (RAM) in about a minute.
  # The SDK /tmp is now its own size-capped tmpfs (below), and this loop keeps
  # every *.log in it small: past `limit` allocated bytes it punches a hole
  # over everything except the first MiB (init messages) and the last 4 MiB.
  # Hole punching, not truncation: the SDK writes without O_APPEND, so a
  # truncated file just becomes sparse at the old offset anyway, and punching
  # keeps the start of the log. Readers see NUL bytes where the hole is
  # (`grep -a`, `tr -d '\0'`). The loop only logs a summary every 10 minutes,
  # so the journal on the SSD does not inherit the flood.
  logTrim = pkgs.writeShellScript "cs8210-sdk-logtrim" ''
    set -u
    dir=/run/cs8210-sdk/tmp
    keep_head=$((1 << 20))
    keep_tail=$((4 << 20))
    limit=$((16 << 20))
    trims=0
    punched=0
    last=$(date +%s)
    while sleep 2; do
      while IFS= read -r -d "" f; do
        read -r size blocks bsize < <(stat -c '%s %b %B' -- "$f") || continue
        used=$((blocks * bsize))
        ((used > limit)) || continue
        end=$(((size - keep_tail) / 4096 * 4096))
        ((end > keep_head)) || continue
        if fallocate --punch-hole --offset "$keep_head" --length $((end - keep_head)) -- "$f"; then
          trims=$((trims + 1))
          punched=$((punched + used - keep_head - keep_tail))
        fi
      done < <(find "$dir" -xdev -type f -name '*.log' -print0 2>/dev/null)
      now=$(date +%s)
      if ((trims > 0 && now - last >= 600)); then
        echo "trimmed SDK logs $trims times in $(((now - last) / 60)) min, ~$((punched >> 20)) MiB dropped"
        trims=0
        punched=0
        last=$now
      fi
    done
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
    innoviumPorts = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "1-16:copper_1x400G:fec-def;17:copper_4x10G;18-32:optics_1x400G:fec-def";
      description = ''
        ASIC port spec for the vendor dynamic_config.sh (auto_load_user.sh -m),
        replacing the uniform one cs8210-switch-up derives from its port mode.
        Breakouts add logical ports, so devport/fpp numbers after them shift.
      '';
    };
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
    # full speed (reg 0x05 bits 7/5 clear). cs8210-fancontrol sets those
    # bits again while it has a valid ASIC die temperature (see
    # ../fan-control/fancontrol.py); without that daemon the fans stay at full.
    systemd.services.cs8210-switch = {
      description = "CS8210 switch ASIC + Credo PHY bring-up (vendor SDK in chroot)";
      after = [ "systemd-modules-load.service" "network-pre.target" ];
      path = [ chroot switchUp config.system.path ];
      environment = lib.optionalAttrs (cfg.innoviumPorts != null) {
        CS8210_INNV_PORTS = cfg.innoviumPorts;
      };
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

    # The chroot's /tmp (fifos, pid files, the SDK's own logs, the libMdio
    # trace) on its own tmpfs, so nothing the SDK writes there can fill /run
    # (RAM) or reach the SSD. See logTrim above for the logs. The libMdio
    # trace (CS8210_MDIO_TRACE) is not trimmed: it stops at this cap.
    fileSystems."/run/cs8210-sdk/tmp" = {
      device = "tmpfs";
      fsType = "tmpfs";
      options = [
        "size=256M"
        "mode=1777"
        "nosuid"
        "nodev"
      ];
    };

    systemd.services.cs8210-sdk-logtrim = {
      description = "CS8210: keep the vendor SDK's log files small";
      wantedBy = [ "multi-user.target" ];
      unitConfig.RequiresMountsFor = "/run/cs8210-sdk/tmp";
      path = [
        pkgs.coreutils
        pkgs.findutils
        pkgs.util-linux
      ];
      serviceConfig = {
        ExecStart = logTrim;
        Restart = "always";
        RestartSec = 5;
        Nice = 10;
      };
    };
  };
}

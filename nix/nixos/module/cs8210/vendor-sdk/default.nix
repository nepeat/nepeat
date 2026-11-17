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
  avsUnit = lib.optional config.hardware.cs8210.asicAvs.enable "cs8210-asic-avs.service";

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
  binPath = lib.makeBinPath [
    pkgs.procps
    pkgs.gnused
    pkgs.coreutils
    pkgs.ethtool
    pkgs.iproute2
    pkgs.systemd
    pkgs.util-linux
    pkgs.bash
  ];
  switchUp =
    pkgs.runCommand "cs8210-switch-up"
      {
        nativeBuildInputs = [ pkgs.makeWrapper ];
      }
      ''
        install -D -m 0755 ${./cs8210-switch-up} $out/libexec/cs8210-switch-up
        patchShebangs $out/libexec
        # /run/current-system/sw/bin last, at runtime: cs8210-show (EEPROM
        # MAC) and the chroot helper's util-linux come from there. A runtime
        # path, not a store path, so system.path changes don't touch the unit.
        makeWrapper $out/libexec/cs8210-switch-up $out/bin/cs8210-switch-up \
          --prefix PATH : ${binPath} --suffix PATH : /run/current-system/sw/bin
        makeWrapper $out/libexec/cs8210-switch-up $out/bin/cs8210-switch-down \
          --add-flags --down \
          --prefix PATH : ${binPath} --suffix PATH : /run/current-system/sw/bin
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
  sdkWatchdog = pkgs.writeShellScript "cs8210-sdk-watchdog" ''
    set -u
    last=0
    seen_active=0
    waiting=0
    while sleep 60; do
      # MemoryMax + OOMPolicy=stop (below): an OOM kill inside the unit stops
      # cs8210-switch (Result=oom-kill); bring it back like a wedge.
      state=$(systemctl show -p ActiveState --value cs8210-switch)
      result=$(systemctl show -p Result --value cs8210-switch)
      if [ "$state" = active ]; then
        seen_active=1
      elif [ "$seen_active" = 1 ] && [ "$state" = failed ] && [ "$result" = oom-kill ]; then
        now=$(date +%s)
        if ((now - last < 900)); then
          # keep seen_active: retried once the 15 min have passed
          [ "$waiting" = 1 ] || echo "cs8210-switch OOM-killed (MemoryMax) again within 15 min of the last restart; restarting it when they have passed"
          waiting=1
        else
          seen_active=0
          waiting=0
          echo "cs8210-switch OOM-killed (MemoryMax, most likely the learn thread's 32 GiB im_malloc), restarting it"
          last=$now
          systemctl restart cs8210-switch || echo "restart of cs8210-switch failed"
        fi
        continue
      fi
      [ "$state" = active ] || continue
      since=$(systemctl show -p ActiveEnterTimestamp --value cs8210-switch)
      [ -n "$since" ] || continue
      epoch=$(date -d "$since" +%s 2>/dev/null) || continue
      hit=$(journalctl -k -q --no-pager --since "@$epoch" -g 'comm: ifcs-learn' -n 1 2>/dev/null) || hit=""
      [ -n "$hit" ] || continue
      now=$(date +%s)
      if ((now - last < 900)); then
        echo "SDK learn thread wedged again within 15 min of the last restart; not restarting: $hit"
        continue
      fi
      echo "SDK learn thread wedged, restarting cs8210-switch: $hit"
      last=$now
      systemctl restart cs8210-switch || echo "restart of cs8210-switch failed"
    done
  '';
  # ASPM off on the ASIC's PCIe link. Mainline enables L0s/L1 by default on
  # devicetree platforms (drivers/pci/pcie/aspm.c, 7.3.4; DT override
  # "aspm-no-l0s"); here that leaves the root port 0001:00:00.0 with
  # L0s on, and the link then logs replay timeouts, BadTLP and (ASIC side)
  # REPLAY_NUM rollovers every second; with L0s off, none (chip A/B
  # 2026-11-16, ai_re/devices/cs8210-32x/learn-wedge.md). Those link errors
  # are the prime suspect for the dead CPU TX ring and the all-ones reads.
  # Exits non-zero when the root port still has ASPM on afterwards. Reads only the
  # root port's config space (on the SoC); re-run by cs8210-switch-power after its
  # PCI remove/rescan (a new link state starts with the kernel's default again).
  asicAspmOff = pkgs.writeShellScript "cs8210-asic-aspm" ''
    set -u
    found=0
    rc=0
    for d in /sys/bus/pci/devices/*; do
      [ "$(cat "$d/vendor")" = 0x1d98 ] || continue
      found=1
      dev=$(basename "$d")
      for s in l0s_aspm l1_aspm; do
        f=$d/link/$s
        [ -e "$f" ] || continue
        if [ "$(cat "$f")" != 0 ]; then
          if echo 0 > "$f"; then echo "$dev: $s off"; else echo "$dev: $s off failed" >&2; fi
        fi
      done
      rp=$(basename "$(dirname "$(readlink -f "$d")")")
      ctl=$(setpci -s "$rp" CAP_EXP+0x10.w)
      echo "$dev: root port $rp LnkCtl=$ctl"
      if (( 0x$ctl & 0x3 )); then echo "$dev: ASPM still enabled on root port $rp (LnkCtl=$ctl)" >&2; rc=1; fi
    done
    [ "$found" = 1 ] || { echo "no Teralynx (vendor 0x1d98) on PCI" >&2; rc=1; }
    exit $rc
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
    ipdPsample = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        ipd psample=1 (kernel/patches/ipd-sflow-psample.patch): an sFlow netlink the
        SAI creates (sai-teralynx tl_sflow, SONiC's genetlink hostif "psample")
        delivers the chip's samples to the kernel's psample groups 1/2 instead of
        registering a genetlink family of its own. Nothing changes until such a
        netlink exists (the vendor SDK never creates one).
      '';
    };
    credoPorts = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "1:optics_1x400G;2:copper_4x10G;3-32:optics_1x400G";
      description = ''
        Credo PHY port spec for the vendor c-phy/auto_load_user.sh -m,
        replacing the uniform one cs8210-switch-up derives from its port
        mode. Ports 1-16 are behind the Credo PHYs; a breakout there needs
        the same mode here and in innoviumPorts.
      '';
    };
    phyPostLoad = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/etc/cs8210-macsec.cmd";
      description = ''
        phy_shell command file, as a path inside the SDK chroot, that
        cs8210-switch-up loads after the Credo bring-up (for example static
        MACsec SAs). Kept on the box, not in the Nix store: it holds keys.
      '';
    };
    phyPreload = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/etc/cs8210-macsec/libcs8210-macsec-shim.so";
      description = ''
        Host path of a library to LD_PRELOAD into the Credo phy_server (the
        chroot helper binds it in as /usr/lib/libcs8210-phy-preload.so).
        Set by hardware.cs8210.macsec. Use a stable path (/etc), not a store
        path: a changed unit environment restarts cs8210-switch.
      '';
    };
    memoryMaxMiB = lib.mkOption {
      type = lib.types.nullOr lib.types.ints.positive;
      default = 1536;
      description = ''
        MemoryMax of cs8210-switch (measured 2026-11-16 on the loop: 387 MB
        steady, 494 MB peak; plus up to the SDK's 256M /tmp tmpfs; kept low
        so it plus PaperMC's 4G stays below the box's 7.6 GB). OOMPolicy=stop: libifcs's learn
        thread asking for ~32 GiB after an all-ones ring index (with
        vm.overcommit_memory = 1, which SONiC's redis sets system-wide, the
        memset succeeds) is killed inside the unit, and cs8210-sdk-watchdog
        restarts it, instead of the global OOM killer taking the box's
        memory (2026-11-16). null: no limit.
      '';
    };
    asicAspm = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Leave ASPM (L0s/L1) on the ASIC's PCIe link as the kernel set it.
        Default false: cs8210-asic-aspm turns it off at boot, before ipd and
        either switch stack (L0s on that link = link errors every second,
        ai_re/devices/cs8210-32x/learn-wedge.md).
      '';
    };
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
    # full speed (reg 0x05 bits 7/5 clear). The fan daemon (fand's cpld_force
    # output, ../fan-control) sets those bits again every tick it is not at
    # 100 %; without it the fans stay at full.
    systemd.services.cs8210-switch = {
      description = "CS8210 switch ASIC + Credo PHY bring-up (vendor SDK in chroot)";
      # ASIC core voltage from its fuses first (../avs; it also loads ipd);
      # refuses to start if that failed.
      requires = avsUnit;
      after = [ "systemd-modules-load.service" "network-pre.target" ] ++ avsUnit;
      # Not config.system.path: that changes with any package in the system
      # and then every deploy restarted this unit (= chip reset). The
      # wrappers fall back to /run/current-system/sw/bin at runtime instead.
      path = [
        chroot
        switchUp
      ];
      environment =
        lib.optionalAttrs (cfg.innoviumPorts != null) {
          CS8210_INNV_PORTS = cfg.innoviumPorts;
        }
        // lib.optionalAttrs (cfg.credoPorts != null) {
          CS8210_CREDO_PORTS = cfg.credoPorts;
        }
        // lib.optionalAttrs (cfg.phyPostLoad != null) {
          CS8210_PHY_POSTLOAD = cfg.phyPostLoad;
        }
        // lib.optionalAttrs (cfg.phyPreload != null) {
          CS8210_PHY_PRELOAD = cfg.phyPreload;
        };
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        # A runaway allocation is OOM-killed inside the unit (option memoryMaxMiB);
        # cs8210-sdk-watchdog restarts it.
        MemoryMax = lib.mkIf (cfg.memoryMaxMiB != null) "${toString cfg.memoryMaxMiB}M";
        OOMPolicy = "stop";
        ExecStart = "${switchUp}/bin/cs8210-switch-up 32x400";
        ExecStop = "${switchUp}/bin/cs8210-switch-down";
        TimeoutStartSec = "10min";
        # cs8210-switch-down asks both daemons to exit, waits up to 20 s and
        # then kills them; whatever is still in the cgroup after that is
        # removed by systemd.
        TimeoutStopSec = "90s";
      };
    };

    systemd.services.cs8210-asic-aspm = lib.mkIf (!cfg.asicAspm) {
      description = "CS8210: ASPM off on the switch ASIC's PCIe link";
      wantedBy = [ "multi-user.target" "cs8210-switch.service" "sonic-syncd.service" ];
      before = [ "cs8210-asic-avs.service" "cs8210-switch.service" "sonic-syncd.service" ];
      # after the root ports answer failed reads with all-ones (no SError)
      wants = [ "cs8210-pcie-errors.service" ];
      after = [ "cs8210-pcie-errors.service" ];
      path = [ pkgs.coreutils pkgs.pciutils ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = asicAspmOff;
      };
    };

    # The SDK's learn thread wedges every few hours on this box (2026-11-10
    # review: one all-ones PCIe read of the learn producer index, then a 32 GiB
    # malloc and an endless loop; reviews/2026-11-10-sdk-learn-crash.md). The
    # data plane is dead until cs8210-switch restarts, and restarts recover
    # unattended (bond, DHCP, speeds). SONiC's libsai guards that read; the
    # vendor innovium.user does not: watch for the kernel's signature line
    # (overcommit 0) or an OOM stop (MemoryMax, overcommit 1) and restart, at
    # most once per 15 min.
    systemd.services.cs8210-sdk-watchdog = {
      description = "CS8210: restart the vendor SDK when its learn thread wedges";
      wantedBy = [ "multi-user.target" ];
      path = [
        pkgs.coreutils
        pkgs.systemd
      ];
      serviceConfig = {
        ExecStart = sdkWatchdog;
        Restart = "always";
        RestartSec = 30;
      };
    };

    # The vendor SDK on this box is from 2020 and speaks the ipd 0.12.x ioctl
    # layouts; see kernel/patches/ipd-legacy-abi.patch.
    boot.extraModprobeConfig = ''
      options ipd legacy_abi=1${lib.optionalString config.hardware.cs8210.vendorSdk.ipdPsample " psample=1"}
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

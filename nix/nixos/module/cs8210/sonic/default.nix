# hardware.cs8210.sonic: SONiC's switch stack (ai_re services.sonic) on this
# box instead of the vendor SDK stack (cs8210-switch). Default off.
#
#   * syncd links our own libsai (ai_re/nix/packages/sai-teralynx: the
#     sai-openbcm SAI frontend + an IFCS backend that dlopen()s the vendor
#     libifcs.so 0.12.41 from the SDK root at run time; nothing vendor is in
#     the store). No arm64 Teralynx SAI exists (ai_re/devices/cs8210-32x/sonic.md).
#   * Board layer (ports, IFCS init yaml, sai.profile, pmon):
#     ai_re/nix/modules/boards/cs8210.
#   * The Credo PHYs (cages 1-16) are not a SAI gearbox here: a oneshot runs the
#     vendor c-phy loader in the SDK chroot in MACsec bypass before syncd, as
#     cs8210-switch-up does.
#   * Mutually exclusive with cs8210-switch: syncd Conflicts= it (starting one
#     stops the other). Both need hardware.cs8210.vendorSdk (ipd legacy ABI,
#     the chroot, the vendor userland at /var/lib/cs8210-sdk/root).
#   * Not started at boot unless autoStart: systemctl start sonic.target.
#     Before syncd starts, a guard refuses to run unless the populated cages
#     are a subset of allowedCages (house network, PROGRESS 2026-11-11).
{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib) mkOption mkEnableOption mkIf mkForce mkDefault types;
  cfg = config.hardware.cs8210.sonic;
  aiRe = ../../../../../ai_re/nix;
  hostSystem = pkgs.stdenv.hostPlatform.system;
  # Like kernel.nix's buildSystem: cross-compile the SONiC packages from
  # this system (the result is still aarch64; only store paths differ).
  spkgs =
    if cfg.buildSystem == null || cfg.buildSystem == hostSystem then
      pkgs
    else
      import pkgs.path {
        localSystem = cfg.buildSystem;
        crossSystem = hostSystem;
        # The machine's overlays too (lldpd's __u128 patch for aarch64 headers).
        inherit (config.nixpkgs) overlays;
      };
  sonic = import (aiRe + "/packages/sonic") { pkgs = spkgs; };
  sai = spkgs.callPackage (aiRe + "/packages/sai-teralynx") { inherit sonic; };
  sdkRoot = "/var/lib/cs8210-sdk/root";
  sdkDir = config.hardware.cs8210.sonic.sdkDir; # host path, under sdkRoot
  # the same directory as seen inside the SDK chroot (the Credo tools run there)
  sdkDirChroot = lib.removePrefix sdkRoot sdkDir;
  ports = config.system.build.sonic.ports;

  # Credo c-phy spec for cages 1-16 from the SONiC ports (<n>x<speed>G per
  # cage) and lineMedia (the module in the cage), e.g. the cage 1 <-> 16 CR8
  # DAC loop: 1:copper_1x400G:fec-def (macsec-400g.md).
  credoSpec =
    let
      cage = c: lib.filter (p: p.cage == c) ports;
      one = c:
        let ps = cage c; n = lib.length ps; g = (lib.head ps).speed / 1000; in
        "${toString c}:${cfg.lineMedia.${toString c} or "optics"}_${toString n}x${toString g}G:fec-def";
    in
    lib.concatMapStringsSep ";" one (lib.range 1 16);
  credo = if cfg.credoPorts != null then cfg.credoPorts else credoSpec;

  allowed = lib.concatMapStringsSep " " toString cfg.allowedCages;
  guard = pkgs.writeShellScript "sonic-cs8210-cage-guard" ''
    set -eu
    # cs8210-qsfp --enable only turns on PCI memory decode of the I2C FPGA
    # (sysfs enable); the status table is MMIO reads.
    raw=$(cs8210-qsfp --enable | sed -n 's/^raw: present=\([0-9a-f]*\) .*/\1/p')
    [ -n "$raw" ] || { echo "cage guard: cannot read cage presence" >&2; exit 1; }
    bad=""
    for c in $(seq 1 32); do
      if (( (0x$raw >> (c - 1)) & 1 )); then
        case " ${allowed} " in *" $c "*) ;; *) bad="$bad $c" ;; esac
      fi
    done
    if [ -n "$bad" ]; then
      echo "cage guard: modules in cages$bad, allowed only: ${allowed}; not starting syncd" >&2
      exit 1
    fi
    echo "cage guard: present mask 0x$raw within allowed cages (${allowed})"
  '';

  credoUp = pkgs.writeShellScript "sonic-cs8210-credo-up" ''
    set -euo pipefail
    SDK=${sdkDirChroot}
    cs8210-sdk-chroot /bin/bash -c "
      set -e
      cd $SDK
      # A FIFO left by a phy_server that is gone makes the loader's write to
      # it block forever (open() for writing without a reader; chip 2026-11-16).
      pgrep -x phy_server >/dev/null || rm -f /tmp/cls_phy_fifo
      ./c-phy/auto_load_user.sh -m '${credo}' -e macsecbypass -l
      # phy_server busy-loops on its FIFO once the last writer is gone;
      # hold one open (same as cs8210-switch-up).
      for _ in 1 2 3 4 5 6 7 8 9 10; do [ -p /tmp/cls_phy_fifo ] && break; sleep 1; done
      if [ -p /tmp/cls_phy_fifo ]; then
        sleep 2147483647 </dev/null >/tmp/cls_phy_fifo 2>/dev/null &
      else
        echo 'sonic-cs8210-credo: /tmp/cls_phy_fifo missing, phy_server may busy-loop' >&2
      fi
      # MACsec bypass: the loader's double_CRC makes the ASIC drop every
      # frame as an FCS error (PROGRESS 2026-11-11); turn it off.
      sed -n 's/^double_CRC \\(slice_id=[0-9]* port=[0-9]*\\) enable=1\$/\\1/p' c-phy/dynamic_config.txt |
        while read -r s p; do
          (cd c-phy && ./phy_shell \"double_CRC \$s \$p enable=0\" >/dev/null) ||
            echo \"sonic-cs8210-credo: double_CRC off failed for \$s \$p\" >&2
        done
    "
  '';
  credoDown = pkgs.writeShellScript "sonic-cs8210-credo-down" ''
    cs8210-sdk-chroot /bin/bash -c "cd ${sdkDirChroot} && ./c-phy/auto_load_user.sh -r" || true
    for _ in $(seq 1 20); do pgrep -x phy_server >/dev/null || exit 0; sleep 1; done
    pkill -x phy_server || true
  '';

  # Same signature and policy as vendor-sdk's cs8210-sdk-watchdog (the IFCS
  # learn thread wedges after an all-ones PCIe read), for syncd: libifcs's
  # threads have the same names inside syncd.
  ifcsWatchdog = pkgs.writeShellScript "sonic-cs8210-ifcs-watchdog" ''
    set -u
    last=0
    while sleep 60; do
      systemctl -q is-active sonic-syncd || continue
      since=$(systemctl show -p ActiveEnterTimestamp --value sonic-syncd)
      [ -n "$since" ] || continue
      epoch=$(date -d "$since" +%s 2>/dev/null) || continue
      hit=$(journalctl -k -q --no-pager --since "@$epoch" -g 'comm: ifcs-learn' -n 1 2>/dev/null) || hit=""
      [ -n "$hit" ] || continue
      now=$(date +%s)
      if ((now - last < 900)); then
        echo "IFCS learn thread wedged again within 15 min; not restarting: $hit"
        continue
      fi
      echo "IFCS learn thread wedged, restarting sonic-syncd: $hit"
      last=$now
      systemctl restart sonic-syncd || echo "restart of sonic-syncd failed"
    done
  '';
  # systemd.services.<n>.path appends /bin and /sbin: the profile root, not its bin
  runtimePath = "/run/current-system/sw";
in
{
  imports = [
    (aiRe + "/modules/switch/sonic")
    (aiRe + "/modules/boards/cs8210")
  ];

  options.hardware.cs8210.sonic = {
    enable = mkEnableOption "SONiC (services.sonic) on the Teralynx 7 instead of the vendor SDK stack (cs8210-switch)";
    buildSystem = mkOption {
      type = types.nullOr types.str;
      default = "x86_64-linux";
      description = ''
        Cross-compile the SONiC packages and the SAI from this system (as
        hardware.cs8210.kernelBuildSystem does for the kernel); null builds
        them natively (aarch64, i.e. under emulation on the desktop).
      '';
    };
    autoStart = mkOption {
      type = types.bool;
      default = false;
      description = "Start sonic.target at boot. Off: systemctl start sonic.target by hand, like cs8210-switch.";
    };
    allowedCages = mkOption {
      type = types.nullOr (types.listOf (types.ints.between 1 32));
      default = [ 1 16 ];
      description = ''
        syncd only starts while every populated cage is one of these
        (read with cs8210-qsfp right before syncd starts). null: no check.
        Default: the cage 1 <-> 16 400G loop, nothing cabled to the house.
      '';
    };
    lineMedia = mkOption {
      type = types.attrsOf (types.enum [ "copper" "optics" ]);
      default = { };
      example = { "1" = "copper"; "16" = "copper"; };
      description = "Credo line side per cage 1-16 (what is plugged in: copper = DAC, line link training); default optics.";
    };
    credoPorts = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "1:optics_1x400G;2:copper_4x10G;3-32:optics_1x400G";
      description = "Credo c-phy auto_load_user.sh -m spec; default derived from the SONiC ports of cages 1-16.";
    };
    macAddress = mkOption {
      type = types.strMatching "([0-9a-f]{2}:){5}[0-9a-f]{2}";
      description = "Switch (router) MAC for DEVICE_METADATA / orchagent -m.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = config.hardware.cs8210.vendorSdk.enable;
        message = "hardware.cs8210.sonic needs hardware.cs8210.vendorSdk (ipd driver with legacy_abi=1, the SDK chroot, libifcs.so from it)";
      }
    ];

    services.sonic = {
      enable = true;
      packages = sonic;
      inherit (cfg) macAddress;
      sai = {
        kind = "vendor";
        package = sonic.sairedis-openbcm sai;
        # libifcs.so NEEDED: libAAPL.so (SDK dir), libjansson.so.4,
        # libyaml-0.so.2, libz.so; libstdc++ and glibc come with syncd.
        libraryPath = [
          "${sai}/lib"
          sdkDir   # libAAPL.so (a host path: syncd runs outside the chroot)
          "${lib.getLib spkgs.jansson}/lib"   # getLib: jansson's default output is -dev
          "${lib.getLib spkgs.libyaml}/lib"
          "${lib.getLib spkgs.zlib}/lib"
        ];
        # sai.conflicts (cs8210-switch), the profile and the IFCS config
        # come from the board layer (ai_re boards/cs8210/sonic.nix).
      };
      lldp = {
        enable = mkDefault true;
        interfaces = [ "Ethernet[0-9]*" "enp0s1" ];
        chassisIdInterface = "enp0s1";
      };
    };
    # SONiC's lldpd takes /run/lldpd.socket and also covers enp0s1.
    services.lldpd.enable = mkIf config.services.sonic.lldp.enable (mkForce false);

    systemd.targets.sonic.wantedBy = mkIf (!cfg.autoStart) (mkForce [ ]);

    systemd.services = {
      sonic-cs8210-guard = mkIf (cfg.allowedCages != null) {
        description = "CS8210: refuse SONiC unless only the allowed cages are populated";
        requiredBy = [ "sonic-syncd.service" ];
        before = [ "sonic-syncd.service" ];
        partOf = [ "sonic.target" ];
        path = [ pkgs.gnused pkgs.coreutils runtimePath ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = guard;
        };
      };
      sonic-cs8210-credo = {
        description = "CS8210: Credo MACsec PHYs (cages 1-16) in MACsec bypass for SONiC";
        requiredBy = [ "sonic-syncd.service" ];
        before = [ "sonic-syncd.service" ];
        after = [ "systemd-modules-load.service" ] ++ lib.optional (cfg.allowedCages != null) "sonic-cs8210-guard.service";
        requires = lib.optional (cfg.allowedCages != null) "sonic-cs8210-guard.service";
        partOf = [ "sonic.target" ];
        conflicts = [ "cs8210-switch.service" ];
        path = [ pkgs.procps pkgs.gnused pkgs.coreutils pkgs.util-linux pkgs.bash runtimePath ];
        restartIfChanged = false;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = credoUp;
          ExecStop = credoDown;
          TimeoutStartSec = "5min";
          TimeoutStopSec = "60s";
        };
      };
      sonic-cs8210-ifcs-watchdog = {
        description = "CS8210: restart syncd when the IFCS learn thread wedges";
        wantedBy = [ "sonic.target" ];
        partOf = [ "sonic.target" ];
        path = [ pkgs.coreutils pkgs.systemd ];
        serviceConfig = {
          ExecStart = ifcsWatchdog;
          Restart = "always";
          RestartSec = 30;
        };
      };
      # The libsai run dir (the merged IFCS config, SAI_TERALYNX_RUN_DIR).
      sonic-syncd.serviceConfig.RuntimeDirectory = "sai-teralynx";
      sonic-syncd.serviceConfig.RuntimeDirectoryPreserve = "yes";
    };

    # cs8210-sai-files reads the board version from the system CPLD (i2cget).
    boot.kernelModules = [ "i2c-dev" ];
    environment.systemPackages = [ sai ];
  };
}

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
#   * Mutually exclusive with cs8210-switch: syncd and the Credo unit
#     Conflicts= it (starting one stops the other). Both need
#     hardware.cs8210.vendorSdk (ipd legacy ABI, the chroot, the vendor
#     userland at /var/lib/cs8210-sdk/root).
#   * Started at boot (autoStart, the default), after the PCIe all-ones setup;
#     autoStart = false leaves it to systemctl start sonic.target. Before
#     syncd starts, a guard refuses to run unless the populated cages are a
#     subset of allowedCages (default: the admin-up ports' cages) and the I2C
#     FPGA answers (house network, PROGRESS 2026-11-11; FPGA stall 2026-11-16).
#   * The cages are cs8210-optics' (../optics): declared cages = the admin-up
#     ports' cages, each in its ports' mode (e.g. 1x400).
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
  # The libsai syncd links (option saiImplementation). The Rust drop-in has the C++ package's
  # lib/ and bin/ layout, so everything below uses ${sai} either way. rust/sai is outside
  # ai_re's path:./nix: a "rust" system builds from the root git flake (new files: git add -N).
  rustSai = ../../../../../rust/sai;
  saiCpp = spkgs.callPackage (aiRe + "/packages/sai-teralynx") { inherit sonic; };
  sai =
    if cfg.saiImplementation == "rust" then
      spkgs.callPackage (rustSai + "/nix/sai-teralynx-rs.nix") {
        saiRs = spkgs.callPackage (rustSai + "/nix/package.nix") {
          saiSrc = sonic.sources.sairedis;
          glueSrc = aiRe + "/packages/sai-openbcm/src";
        };
        cppTeralynx = saiCpp; # read only: the file-list / export checks
      }
    else
      saiCpp;
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

  # Declared cages: the cages of the admin-up ports of the Nix base CONFIG_DB
  # (services.sonic.settings.PORT, by name or alias). Their modules are brought up by
  # cs8210-optics in the mode the cage's ports need (ports x speed); the cage guard
  # refuses syncd while any other cage holds a module (house network, PROGRESS
  # 2026-11-11).
  portSettings = config.services.sonic.settings.PORT or { };
  adminUp = p: ((portSettings.${p.name} or portSettings.${p.alias} or { }).admin_status or "down") == "up";
  declaredCages = lib.unique (map (p: p.cage) (lib.filter adminUp ports));
  cageMode =
    c:
    let
      ps = lib.filter (p: p.cage == c) ports;
    in
    "${toString (lib.length ps)}x${toString ((lib.head ps).speed / 1000)}";
  optics = config.hardware.cs8210.optics.package;
  allowed = if cfg.allowedCages == [ ] then "none" else lib.concatMapStringsSep "," toString cfg.allowedCages;

  # The MACsec shim (hardware.cs8210.macsec: vendorSdk.phyPreload) in phy_server, as
  # cs8210-switch-up does: cs8210-sdk-chroot binds $CS8210_PHY_PRELOAD in at this path.
  # Under SONiC it serves cs8210-macsec status/metrics (ports stay in bypass).
  phyPreload = config.hardware.cs8210.vendorSdk.phyPreload;
  phyPre = lib.optionalString (phyPreload != null) "LD_PRELOAD=/usr/lib/libcs8210-phy-preload.so ";
  credoUp = pkgs.writeShellScript "sonic-cs8210-credo-up" ''
    set -euo pipefail
    SDK=${sdkDirChroot}
    # Line side per cage from the module that is in it (cs8210-optics: DAC -> copper with
    # line link training, optics -> no AN/LT); the Nix spec for empty / unknown cages.
    spec=$(${optics}/bin/cs8210-optics credo-spec --spec '${credo}' --wait 30) || spec='${credo}'
    echo "sonic-cs8210-credo: auto_load_user.sh -m $spec"
    cs8210-sdk-chroot /bin/bash -c "
      set -e
      cd $SDK
      # A FIFO left by a phy_server that is gone makes the loader's write to
      # it block forever (open() for writing without a reader; chip 2026-11-16).
      pgrep -x phy_server >/dev/null || rm -f /tmp/cls_phy_fifo
      ${phyPre}./c-phy/auto_load_user.sh -m '$spec' -e macsecbypass -l
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

  # IFCS watchdog (./ifcs-watchdog.py, unit tests at build): restarts syncd
  # when libifcs's learn thread wedges on a garbage ring index (the vendor
  # stack's cs8210-sdk-watchdog signature, plus its SONiC shapes: the ~32 GiB
  # mapping, the thread spinning, syncd RSS near MemoryMax) or when every
  # transmitting port stops sending (the ASIC's CPU path died first on
  # 2026-11-16: ai_re/devices/cs8210-32x/oom-20261116.txt). libifcs's threads
  # keep their names inside syncd. The bad read itself is defused in libsai
  # (sai-teralynx's learn-ring guard); this stays as the backstop.
  ifcsWatchdog =
    pkgs.runCommand "sonic-cs8210-ifcs-watchdog"
      {
        nativeBuildInputs = [ pkgs.buildPackages.python3 ];
      }
      ''
        python3 -I ${./test_ifcs_watchdog.py} ${./ifcs-watchdog.py}
        install -D -m 0755 ${./ifcs-watchdog.py} $out/bin/sonic-cs8210-ifcs-watchdog
        sed -i '1s|.*|#!${pkgs.python3}/bin/python3 -I|' $out/bin/sonic-cs8210-ifcs-watchdog
      '';
  syncdMemMax = cfg.syncdMemoryMaxMiB;
  # The watchdog's counters (kept across its own restarts within a boot) and the
  # metrics producer that reads them (+ libsai's learn-ring guard file).
  watchdogState = "/run/sonic-cs8210-ifcs-watchdog/state.json";
  mon = config.monitoring.nodeExporter;
  # systemd.services.<n>.path appends /bin and /sbin: the profile root, not its bin
  runtimePath = "/run/current-system/sw";
  # Everything that touches the I2C FPGA or the ASIC over PCIe waits for the
  # root ports to answer failed reads with all-ones (else an SError panic;
  # module/cs8210 pcieErrorsAsAllOnes): matters at boot.
  pcieReady = {
    wants = [ "cs8210-pcie-errors.service" ];
    after = [ "cs8210-pcie-errors.service" ];
  };
  avsUnit = lib.optional config.hardware.cs8210.asicAvs.enable "cs8210-asic-avs.service";
  # cs8210-asic-avs loads ipd after the AVS step (../avs); syncd is ordered after
  # it, so this only waits when the AVS module is off (udev loads ipd then).
  waitIpd = pkgs.writeShellScript "sonic-cs8210-wait-ipd" ''
    for _ in $(seq 1 120); do [ -c /dev/ipd ] && exit 0; sleep 0.5; done
    echo "/dev/ipd missing after 60 s (ipd not loaded with legacy_abi=1?)" >&2
    exit 1
  '';
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
      default = true;
      description = "Start sonic.target at boot. Off: systemctl start sonic.target by hand, like cs8210-switch.";
    };
    allowedCages = mkOption {
      type = types.nullOr (types.listOf (types.ints.between 1 32));
      default = declaredCages;
      defaultText = lib.literalMD "the cages of the admin-up ports in `services.sonic.settings.PORT`";
      description = ''
        syncd only starts while every populated cage is one of these (asked
        of cs8210-optics right before syncd starts) and the I2C FPGA answers.
        null: no check. Default: the declared (admin-up) ports' cages.
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
    syncdMemoryMaxMiB = mkOption {
      type = types.nullOr types.ints.positive;
      default = 2048;
      description = ''
        MemoryMax of sonic-syncd (normal RSS ~330 MiB on the loop config). A
        runaway allocation (libifcs's learn thread memsets ~32 GiB under the
        vm.overcommit_memory = 1 that the NixOS redis module sets) is then
        OOM-killed inside the unit and systemd restarts syncd, instead of the
        global OOM killer taking the box's memory (2026-11-16). The IFCS
        watchdog restarts syncd at 3/4 of it. Raise it for large route
        tables; null: no limit.
      '';
    };
    saiImplementation = mkOption {
      type = types.enum [ "cpp" "rust" ];
      default = "rust";
      description = ''
        The libsai syncd links: "rust" (default since H4, 2026-11-27) =
        rust/sai/nix/sai-teralynx-rs.nix (the Rust drop-in: the same lib/, bin/ layout,
        the same exports + 2, the same profile keys; adds SAI_TERALYNX_POST_INIT_REGS /
        SAI_TERALYNX_DIAG_REGS), "cpp" = ai_re/nix/packages/sai-teralynx (the C++ IFCS
        backend, still selectable until H5). Rollback: "cpp" or the previous generation.
      '';
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
      # sFlow: sflowmgrd, hsflowd (samples via psample: ipd psample=1, vendorSdk.ipdPsample)
      sflow.enable = mkDefault true;
      lldp = {
        enable = mkDefault true;
        interfaces = [ "Ethernet[0-9]*" "enp0s1" ];
        chassisIdInterface = "enp0s1";
      };
    };
    # SONiC's lldpd takes /run/lldpd.socket and also covers enp0s1.
    services.lldpd.enable = mkIf config.services.sonic.lldp.enable (mkForce false);

    systemd.targets.sonic.wantedBy = mkIf (!cfg.autoStart) (mkForce [ ]);

    hardware.cs8210.optics.cages = mkDefault (
      lib.listToAttrs (map (c: lib.nameValuePair (toString c) (cageMode c)) declaredCages)
    );

    systemd.services = {
      # Asks cs8210-optics (the FPGA's owner; it checks the root port and the
      # FPGA itself): FPGA alive and every populated cage declared, else syncd
      # does not start.
      sonic-cs8210-guard = mkIf (cfg.allowedCages != null) {
        description = "CS8210: refuse SONiC unless only the declared cages are populated";
        requiredBy = [ "sonic-syncd.service" ];
        before = [ "sonic-syncd.service" ];
        wants = pcieReady.wants ++ [ "cs8210-optics.service" ];
        after = pcieReady.after ++ [ "cs8210-optics.service" ];
        requires = [ "cs8210-optics.service" ];
        partOf = [ "sonic.target" ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${optics}/bin/cs8210-optics guard --allowed ${allowed} --wait 30";
        };
      };
      # xcvrd reads the cages through cs8210-optics (the one FPGA user); it
      # starts once the guard and the Credo bring-up are done.
      sonic-xcvrd = mkIf (lib.elem "xcvrd" config.services.sonic.pmon.daemons) {
        wants = pcieReady.wants ++ [ "cs8210-optics.service" ];
        after = pcieReady.after ++ [ "cs8210-optics.service" "sonic-cs8210-credo.service" ] ++ lib.optional (cfg.allowedCages != null) "sonic-cs8210-guard.service";
      };
      sonic-cs8210-credo = {
        description = "CS8210: Credo MACsec PHYs (cages 1-16) in MACsec bypass for SONiC";
        requiredBy = [ "sonic-syncd.service" ];
        before = [ "sonic-syncd.service" ];
        inherit (pcieReady) wants;
        after = pcieReady.after ++ [ "systemd-modules-load.service" "cs8210-optics.service" ] ++ lib.optional (cfg.allowedCages != null) "sonic-cs8210-guard.service";
        requires = lib.optional (cfg.allowedCages != null) "sonic-cs8210-guard.service";
        partOf = [ "sonic.target" ];
        conflicts = [ "cs8210-switch.service" ];
        environment = lib.optionalAttrs (phyPreload != null) { CS8210_PHY_PRELOAD = phyPreload; };
        # the chroot's /tmp (Credo FIFOs) is its own tmpfs (vendor-sdk)
        unitConfig.RequiresMountsFor = [ "/run/cs8210-sdk/tmp" sdkRoot ];
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
        description = "CS8210: restart syncd when libifcs wedges (learn thread, dead CPU path)";
        wantedBy = [ "sonic.target" ];
        partOf = [ "sonic.target" ];
        # systemctl / journalctl: the system's systemd is on every unit's PATH
        serviceConfig = {
          ExecStart = "${ifcsWatchdog}/bin/sonic-cs8210-ifcs-watchdog --state ${watchdogState}"
            + lib.optionalString (syncdMemMax != null) " --rss-limit ${toString (syncdMemMax * 3 / 4 * 1048576)}";
          Restart = "always";
          RestartSec = 30;
          RuntimeDirectory = "sonic-cs8210-ifcs-watchdog";
          RuntimeDirectoryPreserve = "yes";
        };
      };
      # The libsai run dir (the merged IFCS config, SAI_TERALYNX_RUN_DIR).
      sonic-syncd.serviceConfig.RuntimeDirectory = "sai-teralynx";
      sonic-syncd.serviceConfig.RuntimeDirectoryPreserve = "yes";
      sonic-syncd.serviceConfig.ExecStartPre = [ "${waitIpd}" ];
      # A runaway is killed inside the unit (OOMPolicy=stop: the whole unit,
      # then Restart=always and the recovery chain), not by the global OOM
      # killer (option syncdMemoryMaxMiB).
      sonic-syncd.serviceConfig.MemoryMax = mkIf (syncdMemMax != null) "${toString syncdMemMax}M";
      sonic-syncd.serviceConfig.OOMPolicy = "stop";
      sonic-syncd.wants = pcieReady.wants;
      # ASIC core voltage from its fuses first (../avs); syncd refuses to start
      # if that failed.
      sonic-syncd.requires = avsUnit;
      sonic-syncd.after = pcieReady.after ++ avsUnit;
    };

    # cs8210_ifcs_watchdog_* (triggers by signal, restarts), the ASIC root port's
    # AER bits / link / ASPM, ipd's ring counters and libsai's learn-ring guard
    # (sai_teralynx_ifcs_guard_*): ai_re/devices/cs8210-32x/learn-wedge.md.
    monitoring.nodeExporter.textfile.producers.cs8210-ifcs = mkIf mon.enable {
      command = "${ifcsWatchdog}/bin/sonic-cs8210-ifcs-watchdog --metrics --state ${watchdogState}";
      interval = 15;
      timeout = 10;
      serviceConfig.LogLevelMax = "notice";
    };

    # Credo PHY die temperatures (cs8210_credo_temperature_celsius{cage}): the vendor's
    # read-only query through phy_shell, answered by phy_server (the MDIO FPGA's one
    # owner; phy_shell's cls_phy_cmd_running flag serialises it with cs8210-macsec).
    # Fails (file dropped) while phy_server is not running.
    monitoring.nodeExporter.textfile.producers.cs8210-credo = mkIf mon.enable {
      command = toString (pkgs.writeShellScript "cs8210-credo-temps" ''
        set -euo pipefail
        export PATH=/run/current-system/sw/bin:$PATH   # cs8210-sdk-chroot
        cs8210-sdk-chroot /bin/bash -c "cd ${sdkDirChroot}/c-phy && ./phy_shell 'firmware_get_slice_temp slice_id=all'" |
          ${config.hardware.cs8210.optics.package}/bin/cs8210-optics credo-temps
      '');
      interval = 60;
      timeout = 20;
      serviceConfig = {
        LogLevelMax = "notice";
        # cs8210-sdk-chroot writes /run/cs8210-sdk and sets up its mounts
        ProtectSystem = false;
        PrivateTmp = false;
      };
    };

    # cs8210-sai-files reads the board version from the system CPLD (i2cget).
    boot.kernelModules = [ "i2c-dev" ];
    environment.systemPackages = [ sai ];
  };
}

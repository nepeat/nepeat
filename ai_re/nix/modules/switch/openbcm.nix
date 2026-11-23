# Broadcom OpenBCM SDK on a switch board: BDE + KNET kernel modules loaded at
# boot, their /dev nodes, one-SDK-process-per-unit locking for bcm.user, and
# the ASIC die temperature reader (openbcm-asic-temp, attach only) that fan
# control and monitoring use. Chip support is whatever Make.local selects in
# packages/openbcm (Tomahawk on the AS7760). Needs the SDK packages allowed as
# unfree: openbcm-bcm-user, openbcm-libsdk (the Rust builds that link the SDK
# are allowed here).
# The SAI the units and syncd run: the Rust libsai and its tools from the
# monorepo's rust/sai (nix/sai-openbcm-rs.nix, hardware.openbcm.rustSai), on
# the C SDK glue in packages/sai-openbcm/src. The C++ sai-openbcm it replaced
# was deleted on 2026-11-23 (rust/sai/README.md, gates G5 / G6).
{ config, lib, options, pkgs, utils, ... }:
let
  cfg = config.hardware.openbcm;
  ms = cfg.mdioScript;
  sonicOn = config.services.sonic.enable or false;
  openbcm = import ../../packages/openbcm { inherit pkgs; };
  sonic = import ../../packages/sonic { inherit pkgs; };
  kmods = openbcm.kmodsFor config.boot.kernelPackages.kernel;
  sai = cfg.package;
  # The Rust SAI lives in the monorepo's rust/sai, outside this tree: its nix
  # directory is given by hardware.openbcm.rustSai (a git flake over the
  # repository, or an --impure evaluation of the working tree, can name it).
  rustFile = name:
    if cfg.rustSai == null
    then throw "hardware.openbcm: the SAI is the Rust one: set hardware.openbcm.rustSai to the monorepo's rust/sai/nix (path:./nix cannot reach it: build such a system from the root git flake, or give hardware.openbcm.package)"
    else cfg.rustSai + "/${name}";
  # The C glue between the Rust backend and the Broadcom SDK (sdk_glue.c / .h).
  glueSrc = ../../packages/sai-openbcm/src;
  libsdk = if cfg.sdk then openbcm.libsdk { } else null;
  # lib/libsai.so, bin/openbcm-* (rust/sai/nix/sai-openbcm-rs.nix).
  rustPackageFor = p: libsdk: p.callPackage (rustFile "sai-openbcm-rs.nix") {
    saiSrc = sonic.sources.sairedis; inherit libsdk glueSrc;
  };
  # The tools the units, phy-status and bcm.user expect of the package
  # (checked at build time: see system.checks below).
  packageTools = [ "openbcm-mdio-script" "openbcm-diag" "openbcm-ext-phy" "openbcm-ext-macsec" ]
    ++ lib.optional cfg.sdk "openbcm-asic-temp";
  mdioTool = ms.package;
  mdioApart = mdioTool.outPath != sai.outPath;
  bcmUser = openbcm.bcm-user { };
  lock = "/run/openbcm-unit0.lock";
  # Same lock as sdk_glue.c, held for the life of the shell.
  bcmUserLocked = pkgs.writeShellScriptBin "bcm.user" ''
    exec 9>>${lock}
    if ! ${pkgs.util-linux}/bin/flock -n 9; then
      echo "bcm.user: another SDK process owns the switch (${lock}); stop it first, e.g. systemctl stop openbcm-asic-temp" >&2
      exit 3
    fi
    exec ${bcmUser}/bin/bcm.user "$@"
  '';
  # openbcm-mdio-script's record of its last run (JSON, and the same for
  # people next to it), kept in /run after the unit ended either way.
  msDir = "/run/openbcm-mdio-script";
  msJson = "${msDir}/status.json";
  msTxt = "${msDir}/status.txt";
  msNames = pkgs.writeText "openbcm-mdio-names" (lib.concatStrings (
    lib.mapAttrsToList (a: n: "phy ${lib.removePrefix "0x" a} ${n}\n") ms.phyNames
    ++ lib.mapAttrsToList (r: n: "reg ${r} ${n}\n") ms.registerNames));
  msNameArgs = lib.optionals (ms.phyNames != { } || ms.registerNames != { }) [ "--names" "${msNames}" ];
  tempProm = lib.optionals (config.monitoring.nodeExporter.enable or false) [ "--prom" "/run/prometheus-textfile/asic.prom" ];
  # phy-status: the last bring-up and what the PHYs read now.
  phyStatus = pkgs.writeShellScriptBin "phy-status" ''
    # phy-status            last run of openbcm-mdio-script.service + the PHYs' verification registers now
    # phy-status --json     the last run, machine readable (${msJson})
    # phy-status --live     read the PHYs with the SDK itself when syncd is not running
    PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.gnused config.systemd.package ]}:$PATH
    tool=${mdioTool}/bin/openbcm-mdio-script
    sock=${lib.escapeShellArg (if ms.diagSocket == null then "" else ms.diagSocket)}
    case "''${1:-}" in
      --json) exec cat ${msJson} ;;
      ""|--live) ;;
      *) sed -n '2,4s/^# //p' "$0"; exit 1 ;;
    esac
    echo "== last run of openbcm-mdio-script.service (${msTxt}) =="
    if [ -r ${msTxt} ]; then cat ${msTxt}; else echo "no record: the unit has not run since boot (systemctl status openbcm-mdio-script)"; fi
    echo
    echo "== the PHYs now (read only) =="
    if [ -n "$sock" ] && [ -S "$sock" ]; then
      echo "through syncd ($sock)"
      exec $tool --verify-only --diag-socket "$sock" ${lib.escapeShellArgs (msNameArgs ++ ms.files)}
    fi
    if [ "$(systemctl show -P ActiveState openbcm-mdio-script.service)" = activating ]; then
      echo "the bring-up is running right now (systemctl status openbcm-mdio-script; ${msTxt} shows its progress)"
      exit 0
    fi
    if [ "''${1:-}" != --live ]; then
      echo "syncd is not running, so nothing reads the PHYs now; the table above is from the last run."
      echo "phy-status --live reads them with the SDK itself: it stops openbcm-asic-temp for a few seconds and"
      echo "re-initialises the switch chip (init soc, init misc), which is harmless only while the stack is down."
      exit 0
    fi
    if systemctl is-active --quiet sonic-syncd.service 2>/dev/null; then
      echo "syncd is running but has no diag socket: not touching the chip"; exit 1
    fi
    reader=0
    if systemctl is-active --quiet openbcm-asic-temp.service; then reader=1; systemctl stop openbcm-asic-temp.service; fi
    $tool --verify-only --config ${ms.sdkConfig} ${lib.escapeShellArgs (msNameArgs ++ ms.files)}
    rc=$?
    [ $reader = 1 ] && systemctl start --no-block openbcm-asic-temp.service
    exit $rc
  '';
in {
  imports = map (o: lib.mkRemovedOptionModule [ "hardware" "openbcm" o ]
    "the C++ sai-openbcm was deleted on 2026-11-23: the SAI, its tools and the MDIO tool are always the Rust ones (rust/sai/README.md)")
    [ "implementation" "mdioTool" "rustPhySeq" "rustMacsec" ];
  options.hardware.openbcm = {
    enable = lib.mkEnableOption "the Broadcom OpenBCM SDK kernel modules (BDE, KNET) and tools";
    # The SAI the switch runs: the Rust libsai and tool set (rust/sai, the same
    # file names and SAI_OPENBCM_* profile keys the C++ sai-openbcm had).
    # Boards wire syncd to hardware.openbcm.package (boards/as7760), so libsai,
    # syncd, openbcm-diag, openbcm-asic-temp, the table checkers, the boot-time
    # MDIO tool and phy-status come from one package.
    rustSai = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = lib.literalExpression "../../../rust/sai/nix";
      description = ''
        The monorepo's rust/sai/nix directory (sai-openbcm-rs.nix). Required unless
        hardware.openbcm.package is given. It lies outside ai_re/nix: the root git flake
        (nix/configs/default.nix sets it when the rust modules are there) and --impure
        evaluations can name it, path:./nix cannot.
      '';
    };
    sdk = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Build the package on the Broadcom SDK (packages/openbcm libsdk); false: without it, fake / virtual backend only (the VM tests).";
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = rustPackageFor pkgs libsdk;
      defaultText = lib.literalExpression ''rust/sai's sai-openbcm-rs.nix (hardware.openbcm.rustSai), on libsdk with hardware.openbcm.sdk'';
      description = "The SAI build whose library syncd links and whose tools the units here run (the VM test of the unit wiring puts stand-ins in).";
    };
    # The same package built for the build host without the SDK, for
    # build-time checks of the board's tables (boards/as7772:
    # openbcm-ext-phy / openbcm-ext-macsec --check).
    nativePackage = lib.mkOption {
      type = lib.types.package;
      internal = true;
      readOnly = true;
      default = rustPackageFor pkgs.buildPackages null;
      description = "The SAI package built for the build host without the SDK (its table checkers).";
    };
    kernelBdeOptions = lib.mkOption {
      type = lib.types.str;
      default = "himem=0 maxpayload=128 dmasize=16M";
      description = "linux-kernel-bde module options (the AS7760 factory values).";
    };
    # PHYs in front of the ASIC that the SDK has no driver for (retimers,
    # MACsec PHYs with a vendor-only library): a recorded MDIO script is run
    # through the chip's MDIO master once per boot, before anything else
    # uses the SDK. Format and tool: rust/sai crates mdio-script, mdio-tool.
    mdioScript = {
      enable = lib.mkEnableOption "running MDIO scripts for external PHYs at boot (openbcm-mdio-script.service)";
      package = lib.mkOption {
        type = lib.types.package;
        default = cfg.package;
        defaultText = lib.literalExpression "hardware.openbcm.package";
        description = "The package whose bin/openbcm-mdio-script the unit and phy-status run (the units VM test puts a stand-in in).";
      };
      files = lib.mkOption {
        type = lib.types.listOf (lib.types.strMatching "/.*");
        example = [ "/var/lib/evora-phy/init.mdio" ];
        description = ''
          Scripts, run in order. Absolute paths on the target, read at run
          time: private data (vendor firmware words) that is installed by
          hand and never enters the Nix store. A missing or malformed file
          fails the unit before any PHY is written.
        '';
      };
      sdkConfig = lib.mkOption {
        type = lib.types.path;
        description = "SDK config.bcm for init soc + init misc (it sets the external MDIO clock and hold delay).";
      };
      requiredBy = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = lib.optionals sonicOn [ "sonic.target" "sonic-syncd.service" ];
        defaultText = lib.literalExpression ''[ "sonic.target" "sonic-syncd.service" ] with services.sonic'';
        description = ''
          Units that must not start when the PHY bring-up failed (Requires=
          and After= on them): without its PHYs the switch has no usable
          port, and a stack that came up on dead ports hides the reason.
          Starting one of them while the bring-up unit is in the failed
          state runs the bring-up again first.
        '';
      };
      stopOnFailure = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = lib.optional sonicOn "sonic.target";
        defaultText = lib.literalExpression ''[ "sonic.target" ] with services.sonic'';
        description = "Units stopped when the bring-up fails (a target takes its PartOf= units with it).";
      };
      phyNames = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        example = { "0x24" = "cage5"; };
        description = "Names of the PHYs (MDIO address word, hex) in messages, the status files and phy-status.";
      };
      registerNames = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        example = { "1.8215" = "fw"; };
        description = "Names of verified registers (dev.reg, hex), the column heads of phy-status.";
      };
      hints = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        example = { phy-not-answering = "cold power cycle"; };
        description = "This board's \"what to do\" per failure class (rust/sai crate mdio-report, `FailureClass`); replaces the tool's general text.";
      };
      diagSocket = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = if sonicOn then config.services.sonic.sai.profile.SAI_OPENBCM_DIAG_SOCKET or null else null;
        defaultText = lib.literalExpression "syncd's SAI_OPENBCM_DIAG_SOCKET";
        description = "syncd's diag socket: phy-status reads the PHYs through it while syncd owns the chip.";
      };
      statusFile = lib.mkOption {
        type = lib.types.str;
        default = msJson;
        readOnly = true;
        description = "The last run's result as JSON (state, operations, per-PHY verification, failure); status.txt next to it is the same for people.";
      };
    };
    temperature = {
      enable = lib.mkEnableOption "the ASIC die temperature reader (openbcm-asic-temp)";
      interval = lib.mkOption { type = lib.types.ints.between 1 60; default = 5; description = "Seconds between reads."; };
      file = lib.mkOption {
        type = lib.types.str;
        default = "/run/asic/temperature";
        readOnly = true;
        description = "Hottest current monitor in C; removed while there is no reading.";
      };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [ {
    boot.extraModulePackages = [ kmods ];
    boot.kernelModules = [ "linux-kernel-bde" "linux-user-bde" "linux-bcm-knet" ];
    boot.extraModprobeConfig = "options linux_kernel_bde ${cfg.kernelBdeOptions}";
    environment.systemPackages = lib.optional ((sai.libsdk or null) != null) bcmUserLocked ++ [ sai ] ++ lib.optional ms.enable phyStatus
      # the boot unit's tool first on PATH when it is not the package's own
      ++ lib.optional (ms.enable && mdioApart) (lib.hiPrio mdioTool);

    # The modules register fixed majors but create no device nodes.
    systemd.services.openbcm-devices = {
      description = "Broadcom SDK device nodes";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-modules-load.service" ];
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      path = [ pkgs.coreutils pkgs.gawk ];
      script = ''
        for d in linux-kernel-bde linux-user-bde linux-bcm-knet; do
          major=$(awk -v d=$d '$2 == d { print $1 }' /proc/devices)
          if [ -z "$major" ]; then echo "$d: module not loaded" >&2; exit 1; fi
          [ -c /dev/$d ] || mknod -m 0600 /dev/$d c "$major" 0
        done
      '';
    };

    # Once per boot (RemainAfterExit; not re-run by nixos-rebuild or a SONiC
    # restart: the PHYs keep their state while the switch chip is re-inited).
    # It holds the SDK lock while it runs, so the temperature reader and syncd
    # are ordered after it, and it publishes the ASIC temperature itself
    # meanwhile (the same files as the reader and syncd).
    # Hard fail: mdioScript.requiredBy (the SONiC stack) does not start
    # without it; openbcm-mdio-script-failed then stops what did start and
    # hands the ASIC temperature to the reader, so the box stays cooled by
    # its curve and reachable. Why it failed: systemctl status (Status: line),
    # the status files (mdioScript.statusFile), phy-status, the login message.
    # Again by hand: systemctl restart sonic.target (or start any unit that
    # requires this one: a failed oneshot is run again), or systemctl restart
    # openbcm-mdio-script, which restarts its dependents around it.
    systemd.services.openbcm-mdio-script = lib.mkIf ms.enable {
      description = "External PHY bring-up over the switch chip's MDIO bus";
      wantedBy = [ "multi-user.target" ];
      requires = [ "openbcm-devices.service" ];
      after = [ "openbcm-devices.service" ];
      before = lib.unique ([ "openbcm-asic-temp.service" "sonic-syncd.service" ] ++ ms.requiredBy);
      requiredBy = ms.requiredBy;
      onFailure = [ "openbcm-mdio-script-failed.service" ];
      restartIfChanged = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        # STATUS= (the one-line outcome) for systemctl status.
        NotifyAccess = "main";
        # The reader may hold the SDK (after an earlier failure): this unit takes over.
        ExecStartPre = lib.optional cfg.temperature.enable "${config.systemd.package}/bin/systemctl stop openbcm-asic-temp.service";
        ExecStart = utils.escapeSystemdExecArgs ([ "${mdioTool}/bin/openbcm-mdio-script" "--config" "${ms.sdkConfig}" "--status-file" msJson ]
          ++ msNameArgs
          ++ lib.concatLists (lib.mapAttrsToList (c: t: [ "--hint" "${c}=${t}" ]) ms.hints)
          ++ lib.optionals cfg.temperature.enable ([ "--temp-out" cfg.temperature.file "--temp-interval" (toString cfg.temperature.interval) ]
            ++ lib.optionals (tempProm != [ ]) [ "--temp-prom" (lib.last tempProm) ])
          ++ ms.files);
        RuntimeDirectory = "openbcm-mdio-script";
        RuntimeDirectoryPreserve = "yes";
        TimeoutStartSec = "15min";
        NoNewPrivileges = true;
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
    systemd.services.openbcm-mdio-script-failed = lib.mkIf ms.enable {
      description = "External PHY bring-up failed: say why, keep the stack down and the ASIC temperature published";
      path = [ config.systemd.package pkgs.coreutils ];
      serviceConfig.Type = "oneshot";
      script = ''
        line=$(head -n 1 ${msTxt} 2>/dev/null) || line=""
        [ -n "$line" ] || line="openbcm-mdio-script: FAILED (no status file: see systemctl status openbcm-mdio-script)"
        echo "<2>$line"
        ${lib.optionalString (ms.stopOnFailure != [ ]) ''
          echo "<2>not starting: ${lib.concatStringsSep " " ms.requiredBy}; stopping ${lib.concatStringsSep " " ms.stopOnFailure}. Details: phy-status"
          systemctl stop --no-block ${lib.escapeShellArgs ms.stopOnFailure}
        ''}
        ${lib.optionalString cfg.temperature.enable "systemctl start --no-block openbcm-asic-temp.service"}
      '';
    };
    # Login message while the bring-up is failed (line 1 of the status text + what to do).
    # Interactive shells only: bash sources /etc/profile for "ssh host command"
    # too, and text there would corrupt scp, nix copy and nixos-rebuild.
    environment.loginShellInit = lib.mkIf ms.enable ''
      if [[ $- == *i* ]] && [ -r ${msTxt} ] && grep -q '^openbcm-mdio-script: FAILED' ${msTxt}; then
        echo; echo "*** PHY bring-up failed${lib.optionalString (ms.requiredBy != [ ]) "; not started: ${lib.concatStringsSep " " ms.requiredBy}"} ***"
        head -n 2 ${msTxt}; echo "  details: phy-status"; echo
      fi
    '';

    systemd.services.openbcm-asic-temp = lib.mkIf cfg.temperature.enable {
      description = "Switch ASIC die temperature reader (Broadcom SDK, attach only)";
      wantedBy = [ "multi-user.target" ];
      requires = [ "openbcm-devices.service" ];
      after = [ "openbcm-devices.service" ];
      # fand waits for the first reading instead of starting in failsafe.
      before = lib.optional (config.services.fand.enable or false) "fand.service";
      serviceConfig = {
        # "started" = the first reading is on disk (or 30 s passed).
        ExecStartPost = "${pkgs.bash}/bin/bash -c 'for i in $(seq 60); do [ -s ${cfg.temperature.file} ] && exit 0; sleep 0.5; done; echo no reading after 30 s >&2'";
        ExecStart = lib.concatStringsSep " " [
          "${sai}/bin/openbcm-asic-temp --interval ${toString cfg.temperature.interval}"
          "--out ${cfg.temperature.file}"
          (lib.concatStringsSep " " tempProm)
        ];
        RuntimeDirectory = "asic";
        Restart = "always";
        RestartSec = "5s";
        # Exit 5 = it powered the ASIC core down (--shutdown-at). Any SDK access
        # after that can crash the host (2026-11-15: the box panicked and
        # rebooted right after the test), so stay down: no reading means fand
        # holds 100% until a power cycle.
        RestartPreventExitStatus = "5";
        # The SDK maps the BDE devices and its DMA pool; it runs as root.
        NoNewPrivileges = true;
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
  } (lib.mkIf cfg.sdk {
    # The Rust builds that link the Broadcom SDK carry its licence (unfree)
    # under their own names.
    nixpkgs.config.allowUnfreePackages = [ "sai-openbcm-rs" "sai-rs-libsai-openbcm" ];
  }) (lib.mkIf (options.hardware.openbcm.package.highestPrio >= (lib.mkOptionDefault { }).priority || (ms.enable && mdioApart)) {
    # The package must carry every tool the units and scripts here name: a
    # missing one fails the system build (not checked for a package given by
    # hand, e.g. the stand-ins of the units VM test).
    system.checks = [ (pkgs.runCommand "openbcm-tools-present" { } ''
      for t in ${lib.concatMapStringsSep " " (t: "${sai}/bin/${t}") packageTools} ${lib.optionalString ms.enable "${mdioTool}/bin/openbcm-mdio-script"} ${sai}/lib/libsai.so; do
        [ -e "$t" ] || { echo "hardware.openbcm: $t is missing" >&2; exit 1; }
      done
      touch $out
    '') ];
  }) ]);
}

# CS8210-32X SONiC board layer: what services.sonic needs to know about this
# board, all mkDefault and nothing set until services.sonic.enable.
#  - port map (switch/sonic/port-maps/celestica-cs8210-32x.nix), 1x400G per
#    cage, Teralynx 7 speeds per lane count;
#  - the IFCS init config generated from the SONiC ports (./sdk-config.nix)
#    and its sidecars (SAI_TERALYNX_PORT_MAP, the generated init commands);
#  - vendor tuning data at runtime from the SDK tree (hardware.cs8210.sonic.sdkDir,
#    never in the store): cs8210-sai-files.service (before syncd) links the isg
#    yaml for this board version and writes the pre-emphasis commands for the
#    current devports (./sai-files.py) into hardware.cs8210.sonic.runDir.
# Left to the machine: sai.kind/package/libraryPath (the Teralynx SAI and
# libifcs' library path), macAddress, the vendor-SDK units (cs8210-switch is
# in sai.conflicts; its watchdog and the Credo PHY bring-up are the machine's).
{ config, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkDefault mkIf types concatStringsSep;
  cfg = config.hardware.cs8210.sonic;
  sonic = config.services.sonic;
  teralynx = import ../../../packages/sai-teralynx/configs { inherit pkgs; };
  # The generator's file for the current ports (null without a generator).
  sdk = config.system.build.sonic.sdkConfig;
  python = sonic.packages.python.python3;
in {
  options.hardware.cs8210.sonic = {
    sdkDir = mkOption {
      type = types.str;
      default = "/var/lib/cs8210-sdk/root/root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK";
      description = "Vendor Briggs SDK V0.1.7 tree on the switch: libifcs.so (+ libAAPL.so), cel_configs/isg*.yaml, cel_cmds/ pre-emphasis tables.";
    };
    media = mkOption {
      type = types.attrsOf (types.enum [ "copper" "optics" ]);
      default = { };
      example = { "17" = "copper"; };
      description = ''
        ASIC-side media per cage (key: cage number). Default: copper for 1-16
        (lanes to the Credo PHYs, host link training), optics for 17-32. copper
        on 17-32 = a DAC: link training in the PAM4 modes, the vendor's DAC
        tables, its copper FEC defaults.
      '';
    };
    runDir = mkOption {
      type = types.str;
      default = "/run/cs8210-sai";
      description = "Where cs8210-sai-files writes isg.yaml (link), preemphasis.cmd and board-version.";
    };
    boardVersion = mkOption {
      type = types.nullOr (types.ints.between 0 15);
      default = null;
      description = "Board version instead of the system CPLD's (reg 0x01 bits 7:4); 2 selects isg_R01.yaml.";
    };
  };

  config = mkIf sonic.enable {
    services.sonic = {
      platform = mkDefault "arm64-celestica_cs8210-r0";
      hwsku = mkDefault "Celestica-CS8210-32X";
      asicType = mkDefault "marvell-teralynx";
      cages = mkDefault (import ../../switch/sonic/port-maps/celestica-cs8210-32x.nix);
      defaultSpeed = mkDefault 400000;
      validSpeeds = mkDefault teralynx.validSpeeds;
      # 1x100G/1x40G/4x25G/4x10G use lanes 0-3 of the 8-lane cage.
      allowUnusedLanes = mkDefault true;
      sai.sdkConfig.generator = mkDefault (import ./sdk-config.nix {
        inherit pkgs teralynx;
        sonic = sonic.packages;
        inherit (cfg) media;
      });
      # One process on the ASIC: the vendor bring-up stops while syncd runs.
      sai.conflicts = mkDefault [ "cs8210-switch.service" ];
      # BGP/BFD stay closed on the management port and on the CPU card's
      # 10G link to ASIC devport 129 (not a SONiC port).
      frr.managementInterfaces = mkDefault [ "enp0s1" "enp0s3" ];
      # Traditional buffer model (BUFFER_* in CONFIG_DB, as SONiC's default QoS
      # config): buffermgrd copies them to APPL_DB, else bufferorch never marks
      # the ports ready and they stay admin down (chip 2026-11-16). The lookup
      # rows only feed lossless PG profiles, which this init config cannot
      # program (no lossless DTM partition: kept, not programmed).
      buffers.enable = mkDefault true;
      buffers.pgProfileLookup = mkDefault (pkgs.writeText "cs8210-pg_profile_lookup.ini" ''
        # PG lossless profiles (not programmed on this init config, see sai-features.md)
        # speed cable size xon xoff threshold xon_offset
        400000 5m 18796 0 236800 0 3112
        100000 5m 18796 0 165200 0 3112
        40000 5m 18796 0 80000 0 3112
        25000 5m 18796 0 60000 0 3112
        10000 5m 18796 0 40000 0 3112
      '');
      sai.profile = lib.mapAttrs (_: mkDefault) {
        SAI_OPENBCM_BACKEND = "openbcm";
        SAI_TERALYNX_LIBIFCS = "${cfg.sdkDir}/libifcs.so";
        SAI_TERALYNX_CONFIG_APPEND = "${cfg.runDir}/isg.yaml";
        # The libsai's read-only diag shell (copp, acl, counters <devport>, mirror):
        # openbcm-diag /run/sai-teralynx/diag.sock <command>
        SAI_OPENBCM_DIAG_SOCKET = "/run/sai-teralynx/diag.sock";
        SAI_TERALYNX_RUN_DIR = "/run/sai-teralynx";
        # sFlow: samples land on CPU queue 2, the sflow trap group's queue (sdk-config.nix
        # traffic_monitor_cpu_queue); its policer is not applied (SAI_TERALYNX_SFLOW_METER
        # stays off: the queue meter misbehaved on the chip, sai-features.md). Egress
        # sampling: node EGRESS_SFLOW_ENABLE before node enable (the yaml key is ignored).
        SAI_TERALYNX_SFLOW_CPU_QUEUE = "2";
        SAI_TERALYNX_EGRESS_SFLOW = "on";
      } // {
        # Generated with the IFCS config; init commands in the vendor's order:
        # rx_eq_fine_tune, the pre-emphasis tables, link training.
        SAI_TERALYNX_PORT_MAP = mkIf (sdk != null) (mkDefault sdk.portMap);
        SAI_TERALYNX_INIT_CMDS = mkIf (sdk != null) (mkDefault
          (concatStringsSep ":" [ sdk.initCmds "${cfg.runDir}/preemphasis.cmd" sdk.linkTraining ]));
      };
    };

    systemd.services.cs8210-sai-files = mkIf (sdk != null) {
      description = "CS8210 SAI files from the vendor SDK (isg by board version, pre-emphasis per devport)";
      before = [ "sonic-syncd.service" ];
      requiredBy = [ "sonic-syncd.service" ];
      path = [ pkgs.i2c-tools ];
      unitConfig.RequiresMountsFor = cfg.sdkDir;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = concatStringsSep " " ([ "${python}/bin/python3" "-I" "${./sai-files.py}"
          "--sdk" cfg.sdkDir "--layout" sdk.layout "--out" cfg.runDir ]
          ++ lib.optionals (cfg.boardVersion != null) [ "--board-version" (toString cfg.boardVersion) ]);
      };
    };
  };
}

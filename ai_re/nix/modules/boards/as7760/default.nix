# Accton/Edgecore AS7760-32X (NXP T2080 + BCM56930) board profile: the shared
# T2080 platform, this board's kernel machine patch, fan/thermal handling, the
# Broadcom SDK modules + ASIC temperature reader, how SONiC runs on this
# board (services.sonic, off until a config enables it),
# Prometheus metrics on :9100 (./monitoring.nix).
# Add ./sd-install.nix (installed system on SD) or
# ../../profiles/netboot-root.nix + ./sd-persist.nix (root in RAM).
{ config, lib, pkgs, ... }:
let
  sonic = import ../../../packages/sonic { inherit pkgs; };
  openbcm = import ../../../packages/openbcm { inherit pkgs; };
  # libsai and its tools: the Rust SAI (hardware.openbcm.package, rust/sai).
  sai = config.hardware.openbcm.package;
in {
  # The Linux device tree source the boot files are built from. Derivative
  # boards (boards/as7772) point it at their own tree.
  options.hardware.as7760.deviceTreeSource = lib.mkOption {
    type = lib.types.path;
    default = builtins.path { path = ../../../packages/linux-qoriq-t2080/boards/as7760/as7760-vendor.dts; name = "as7760-vendor.dts"; };
    description = "Device tree source (.dts) for this board's kernel.";
  };
  imports = [ ../../platforms/qoriq-t2080.nix ../../thermal/as7760-fan.nix ../../switch/openbcm.nix
              ../../switch/sonic ./monitoring.nix ./sonic-platform.nix ];
  config = {
  # BCM56930: SDK modules at boot and the die temperature reader (fan control input).
  hardware.openbcm = { enable = true; temperature.enable = true; };
  # Front-panel module EEPROM reader (works for hot-plugged modules).
  environment.systemPackages = [ (pkgs.callPackage ../../../packages/as7760-qsfp { }) ]
    # openbcm-diag <cmd>: SDK diag shell inside syncd (SAI_OPENBCM_DIAG_SOCKET below).
    ++ lib.optional config.services.sonic.enable (pkgs.runCommand "openbcm-diag" { } ''
      install -D ${sai}/bin/openbcm-diag $out/bin/openbcm-diag
    '');
  hardware.qoriqT2080.kernelPatches = [ {
    name = "accton-as7760-corenet-machine";
    patch = pkgs.writeText "as7760-machine.patch"
      (builtins.readFile ../../../packages/linux-qoriq-t2080/boards/as7760/as7760-machine.patch);
  } ];

  # SONiC on this board. syncd (the Rust libsai) owns the ASIC while it runs, so
  # openbcm-asic-temp stops and syncd publishes the same temperature file for
  # fand; it is started again when the stack stops.
  # With SONiC on, syncd owns the ASIC from boot: don't start the reader at
  # boot or on activation (it would be stopped mid-start by syncd's Conflicts
  # and show as failed); restartOnStop brings it back when syncd stops.
  systemd.services.openbcm-asic-temp.wantedBy = lib.mkIf config.services.sonic.enable (lib.mkForce [ ]);

  services.sonic = {
    platform = lib.mkDefault "powerpc64-accton_as7760_32x-r0";
    hwsku = lib.mkDefault "Accton-AS7760-32X";
    cages = import ../../switch/sonic/port-maps/accton-as7712-32x.nix;
    sai = {
      kind = "vendor";
      package = sonic.sairedis-openbcm sai;
      libraryPath = [ "${sai}/lib" ];
      # config.bcm generated from the SONiC ports (cages + breakout), public
      # AS7712-32X values; sai.initConfigFile can still point at a file.
      sdkConfig.generator = import ./sdk-config.nix { inherit openbcm sonic; };
      restartOnStop = [ "openbcm-asic-temp.service" ];
      profile = {
        SAI_OPENBCM_BACKEND = "openbcm";
        SAI_OPENBCM_TEMP_FILE = config.hardware.openbcm.temperature.file;
        SAI_OPENBCM_TEMP_PROM = "/run/prometheus-textfile/asic.prom";
        # Debug: root-only socket for diag-shell commands while syncd owns
        # the chip (client: openbcm-diag). See rust/sai/README.md (sai-diag).
        SAI_OPENBCM_DIAG_SOCKET = "/run/sai-openbcm/diag.sock";
      };
    };
  };
  };
}

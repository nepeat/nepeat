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
  sai = pkgs.callPackage ../../../packages/sai-openbcm { inherit sonic; libsdk = openbcm.libsdk { }; };
in {
  imports = [ ../../platforms/qoriq-t2080.nix ../../thermal/as7760-fan.nix ../../switch/openbcm.nix
              ../../switch/sonic ./monitoring.nix ];
  # BCM56930: SDK modules at boot and the die temperature reader (fan control input).
  hardware.openbcm = { enable = true; temperature.enable = true; };
  # Front-panel module EEPROM reader (works for hot-plugged modules).
  environment.systemPackages = [ (pkgs.callPackage ../../../packages/as7760-qsfp { }) ];
  hardware.qoriqT2080.kernelPatches = [ {
    name = "accton-as7760-corenet-machine";
    patch = pkgs.writeText "as7760-machine.patch"
      (builtins.readFile ../../../packages/linux-qoriq-t2080/boards/as7760/as7760-machine.patch);
  } ];

  # SONiC on this board. syncd (sai-openbcm) owns the ASIC while it runs, so
  # openbcm-asic-temp stops and syncd publishes the same temperature file for
  # fand; it is started again when the stack stops.
  services.sonic = {
    platform = lib.mkDefault "powerpc64-accton_as7760_32x-r0";
    hwsku = lib.mkDefault "Accton-AS7760-32X";
    cages = import ../../switch/sonic/port-maps/accton-as7712-32x.nix;
    sai = {
      kind = "vendor";
      package = sonic.sairedis-openbcm sai;
      libraryPath = [ "${sai}/lib" ];
      # Factory 32x100G SDK config, copied by hand (not in the repo or store)
      # until Nix generates config.bcm; only the 32x100G layout matches it.
      initConfigFile = lib.mkDefault "/var/lib/sonic/config-32x100.bcm";
      restartOnStop = [ "openbcm-asic-temp.service" ];
      profile = {
        SAI_OPENBCM_BACKEND = "openbcm";
        SAI_OPENBCM_TEMP_FILE = config.hardware.openbcm.temperature.file;
        SAI_OPENBCM_TEMP_PROM = "/run/prometheus-textfile/asic.prom";
      };
    };
  };
}

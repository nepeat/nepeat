# Accton/Edgecore AS7760-32X (NXP T2080 + BCM56930) board profile: the shared
# T2080 platform, this board's kernel machine patch, fan/thermal handling,
# Prometheus metrics on :9100 (./monitoring.nix).
# Add ./sd-install.nix (installed system on SD) or
# ../../profiles/netboot-root.nix + ./sd-persist.nix (root in RAM).
{ pkgs, ... }:
{
  imports = [ ../../platforms/qoriq-t2080.nix ../../thermal/as7760-fan.nix ./monitoring.nix ];
  # Front-panel module EEPROM reader (works for hot-plugged modules).
  environment.systemPackages = [ (pkgs.callPackage ../../../packages/as7760-qsfp { }) ];
  hardware.qoriqT2080.kernelPatches = [ {
    name = "accton-as7760-corenet-machine";
    patch = pkgs.writeText "as7760-machine.patch"
      (builtins.readFile ../../../packages/linux-qoriq-t2080/boards/as7760/as7760-machine.patch);
  } ];
}

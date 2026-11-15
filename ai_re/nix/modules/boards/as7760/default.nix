# Accton/Edgecore AS7760-32X (NXP T2080 + BCM56930) board profile: the shared
# T2080 platform, this board's kernel machine patch, fan/thermal handling.
# Add ./sd-install.nix (installed system on SD) or
# ../../profiles/netboot-root.nix + ./sd-persist.nix (root in RAM).
{ pkgs, ... }:
{
  imports = [ ../../platforms/qoriq-t2080.nix ../../thermal/as7760-fan.nix ];
  hardware.qoriqT2080.kernelPatches = [ {
    name = "accton-as7760-corenet-machine";
    patch = pkgs.writeText "as7760-machine.patch"
      (builtins.readFile ../../../packages/linux-qoriq-t2080/boards/as7760/as7760-machine.patch);
  } ];
}

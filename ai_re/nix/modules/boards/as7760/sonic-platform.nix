# AS7760-32X SONiC platform integration (devices/edge-core-7760-32x/sonic-platform.md):
#  - this board's sonic_platform (./sonic_platform) for the pmon daemons
#    (services.sonic.pmon: xcvrd, thermalctld monitor-only, psud, syseepromd);
#  - the front-panel LED program, loaded by syncd at SDK init
#    (SAI_OPENBCM_RC_FILE): the public sonic-buildimage AS7712-32X
#    led_proc_init.soc, line for line the factory portled.soc. Its "led auto
#    on" registers the diag shell's linkscan callback, so the SDK's linkscan
#    (inside syncd) writes each port's link bit into the LED data RAM; no ledd;
#  - LLDP (services.sonic.lldp).
{ config, lib, pkgs, ... }:
let
  cfg = config.services.sonic;
  bi = cfg.packages.sources.buildimage;
  openbcm = import ../../../packages/openbcm { inherit pkgs; };
  platformPackage = pkgs.callPackage ./sonic-platform-package.nix { sonic = cfg.packages; };
  ledProgram = openbcm.configs.ledProgram {
    name = "as7760-32x-led_proc_init.soc";
    src = "${bi}/device/accton/x86_64-accton_as7712_32x-r0/led_proc_init.soc";
    label = "sonic-buildimage ${builtins.substring 0 12 bi.rev} device/accton/x86_64-accton_as7712_32x-r0/led_proc_init.soc";
  };
in {
  options.hardware.as7760.ledProgram = lib.mkOption {
    type = lib.types.nullOr lib.types.path;
    default = ledProgram;
    defaultText = lib.literalExpression "the public AS7712-32X led_proc_init.soc";
    description = "LED processor rc script syncd runs at SDK init (SAI_OPENBCM_RC_FILE); null: none (LEDs stay dark).";
  };

  config = {
    services.sonic.pmon = {
      enable = lib.mkDefault true;
      platformPackage = lib.mkDefault platformPackage;
      environment.AS7760_ASIC_TEMP_FILE = config.hardware.openbcm.temperature.file;
    };
    services.sonic.lldp.enable = lib.mkDefault true;
    services.sonic.sai.profile = lib.mkIf (config.hardware.as7760.ledProgram != null) {
      SAI_OPENBCM_RC_FILE = "${config.hardware.as7760.ledProgram}";
    };
    system.build.as7760 = { sonicPlatform = platformPackage; inherit ledProgram; };
  };
}

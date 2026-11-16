# SKETCH, not imported anywhere: how the qemu-ipd experiment could sit on the
# CS8210 once it works (machine glue lives in nepeat/nix nixos/module/cs8210).
# The experiment itself runs `isai-qemu` by hand; no binfmt registration, so
# nothing system-wide changes and no other amd64 binary is ever run implicitly.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.cs8210.isaiQemu;
  q = import ./. { pkgs = pkgs.pkgsBuildBuild; };   # x86_64 build host
in {
  options.hardware.cs8210.isaiQemu = {
    enable = lib.mkEnableOption "Marvell's amd64 Teralynx SAI under qemu-ipd (experiment)";
    profile = lib.mkOption { type = lib.types.path; description = "sai.profile for isai"; };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ q.runner ];
    systemd.services.isai-smoke = {
      description = "isai smoke run under qemu-ipd";
      # one ASIC user at a time
      conflicts = [ "cs8210-switch.service" "sonic.target" ];
      serviceConfig = {
        Type = "oneshot";
        # ipd must be loaded with legacy_abi=0 for this SAI (0444 parameter)
        ExecStartPre = [
          "${pkgs.kmod}/bin/rmmod ipd"
          "${pkgs.kmod}/bin/modprobe ipd legacy_abi=0"
          # + the cage guard of hardware.cs8210.sonic (only allowed cages populated)
        ];
        ExecStart = "${q.runner}/bin/isai-qemu ${q.smoke}/bin/isai-smoke ${cfg.profile} 300";
        ExecStopPost = [ "-${pkgs.kmod}/bin/rmmod ipd" "${pkgs.kmod}/bin/modprobe ipd" ];
        Environment = [ "QEMU_IPD_TRACE=1" ];
      };
    };
    # The alternative, NOT wanted for the experiment:
    #   boot.binfmt.registrations.x86_64-ipd = { interpreter = "${q.qemu-ipd}/bin/qemu-x86_64";
    #     magicOrExtension = ...; mask = ...; preserveArgvZero = true; };
    # would need the -B 0 / -cpu / strict settings baked into a wrapper and makes every
    # amd64 ELF on the box run under it.
  };
}

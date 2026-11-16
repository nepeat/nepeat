# Our U-Boot in the DMC-12's boot SPI flash, installed from Linux: `dmc12-firmware` (see the script).
# The board DT exposes exactly three writable windows of CS0 for it (IPXE 0, IPXE 1, BOOT_MODE);
# everything else stays read-only. The payload is pinned: a rebuild that does not produce the
# rehearsed bytes fails the build instead of shipping an untested U-Boot (the tool and the pins live
# in packages/dmc12-firmware, also runnable standalone with nix run). The read-only rest of CS0 is pinned too (factory layout, factory
# U-Boot intact; identical on u01 and p01): the tool refuses to write on anything else.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.dmc12.firmware;
  fw = import ../../../../packages/dmc12-firmware {
    inherit pkgs lib;
    bootLoader = config.hardware.dmc12.bootLoader;
    inherit (cfg) ubootSha256;
  };
  inherit (fw) tool payloads;
in
{
  options.hardware.dmc12.firmware = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "dmc12-firmware and the boot flash's writable IPXE/BOOT_MODE windows in the Linux DT.";
    };
    ubootSha256 = lib.mkOption {
      type = lib.types.str;
      default = (import ../../../../packages/dmc12-firmware { inherit pkgs lib; }).ubootSha256;   # the rehearsed pin
      description = "sha256 of the rehearsed U-Boot IPXE object; the build fails on any other.";
    };
  };

  config = lib.mkIf cfg.enable {
    # After the env-window overlay (uboot-env), which already split CS0 into boot | env | tail:
    # boot (0..0xf90000) becomes spi0-boot | ipxe0 | boot-mode | spi0-mid | ipxe1 | spi0-high.
    hardware.deviceTree.overlays = lib.mkAfter [{
      name = "dmc12-firmware-windows";
      dtsText = ''
        /dts-v1/;
        /plugin/;
        / {
          compatible = "edgecore,dmc-12-48r-as0";
          fragment@0 {
            target-path = "/soc/io-bus@fc000000/spi@1882000/flash@0/partitions";
            __overlay__ {
              partition@0 { reg = <0x0 0x2c0000>; };
              partition@2c0000 { label = "ipxe0"; reg = <0x2c0000 0x80000>; };
              partition@340000 { label = "boot-mode"; reg = <0x340000 0x10000>; };
              partition@350000 { label = "spi0-mid"; reg = <0x350000 0x230000>; read-only; };
              partition@580000 { label = "ipxe1"; reg = <0x580000 0x80000>; };
              partition@600000 { label = "spi0-high"; reg = <0x600000 0x990000>; read-only; };
            };
          };
        };
      '';
    }];
    environment.systemPackages = [ tool ];
    # Record which slot booted our U-Boot (the proof install/switch require; see the script).
    systemd.services.dmc12-firmware-mark-booted = {
      description = "Record the boot of our U-Boot from its IPXE slot";
      wantedBy = [ "multi-user.target" ];
      after = [ "local-fs.target" ];
      serviceConfig = { Type = "oneshot"; ExecStart = "${tool}/bin/dmc12-firmware mark-booted"; };
    };
    system.build.dmc12FirmwarePayloads = payloads;
    # `nix run <flake>#nixosConfigurations.<host>.config.system.build.dmc12FirmwareTool -- status`
    system.build.dmc12FirmwareTool = tool;
  };
}

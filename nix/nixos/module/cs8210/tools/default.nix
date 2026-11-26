# services.cs8210-tools: hardware convenience commands for the CS8210-32X,
# the counterpart of dmc12-nixos' dmc12-tools.
#
#   cs8210-fan    status / set / quiet / max / watchdog (fan CPLDs)
#   cs8210-show   cpld / fan / temperature / eeprom / pci / version
#   onieeeprom    ONIE TlvInfo parser (unchanged from dmc12-nixos)
#   cs8210-pci-probe  identify the two FPGAs, the Innovium ASIC and (with
#                 --phy-id) the 16 Credo CMS50216 MACsec ports from userspace
# The cages (I2C FPGA) are cs8210-optics' (../optics), the one process that
# touches that FPGA.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.cs8210-tools;

  runtimePath = lib.makeBinPath [
    pkgs.bash
    pkgs.coreutils
    pkgs.gawk
    pkgs.gnugrep
    pkgs.i2c-tools
    pkgs.pciutils
    pkgs.ubootTools
  ];

  tools =
    pkgs.runCommand "cs8210-tools"
      {
        nativeBuildInputs = [ pkgs.makeWrapper ];
      }
      ''
        install -D -m 0644 ${./cs8210-lib.sh} $out/share/cs8210/cs8210-lib.sh
        install -D -m 0755 ${./cs8210-fan} $out/libexec/cs8210-fan
        install -D -m 0755 ${./cs8210-show} $out/libexec/cs8210-show
        install -D -m 0755 ${./onieeeprom} $out/bin/onieeeprom
        install -D -m 0755 ${./cs8210-pci-probe} $out/bin/cs8210-pci-probe
        sed -i '1s|.*|#!${pkgs.python3}/bin/python3|' $out/bin/onieeeprom $out/bin/cs8210-pci-probe
        patchShebangs $out/libexec
        for t in cs8210-fan cs8210-show; do
          makeWrapper $out/libexec/$t $out/bin/$t --prefix PATH : ${runtimePath}:$out/bin
        done
      '';
in
{
  options.services.cs8210-tools = {
    enable = lib.mkEnableOption "CS8210 hardware info and fan commands (cs8210-show, cs8210-fan, onieeeprom)";
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ tools ];
  };
}

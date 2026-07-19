{
  description = "ai_re — hardware reverse engineering devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          name = "ai_re";

          # Add tools here as RE tasks require them. Note the reason in the
          # commit that adds a package.
          packages = with pkgs; [
            # Terminal multiplexing for persistent serial sessions
            tmux

            # Serial console clients
            tio
            picocom
            minicom

            # Serial / USB inspection
            # (macOS uses /dev/tty.usbserial-* ; these help on Linux too)
            usbutils
            lsof

            # SPI/flash programming (CH341A, Pi, FT2232 via flashrom)
            flashrom
            # I2C 24Cxx EEPROM reader for the CH341A (flashrom is SPI-only)
            ch341eeprom

            # General RE / analysis helpers
            binwalk
            hexyl # friendly hex viewer
            file
            ripgrep
          ];

          shellHook = ''
            echo "ai_re devshell — hardware RE"
            echo "serial: tmux new-session -d -s serial 'tio /dev/<port> -b 115200'"
            echo "ports : ls /dev/tty.* (macOS) | ls /dev/ttyUSB* /dev/ttyACM* (linux)"
          '';
        };
      });
}

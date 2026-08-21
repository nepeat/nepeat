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
            # XGecu/TL866-family universal programmer driver (flashrom does NOT
            # support these smart programmers). Drives the XGecu Pro (T48/T56)
            # and TL866II+ over their proprietary USB proto; VCC/VPP follow the
            # `-p <chip>` profile, not a manual voltage knob.
            minipro

            # git-lfs — firmware dumps (ai_re/**/*.bin) are stored via LFS.
            # NOTE: LFS filters run on `git` at the repo root (outside this
            # devshell), so git-lfs must ALSO be on your global PATH
            # (`nix profile install nixpkgs#git-lfs` or brew). This entry keeps
            # it available in-shell and documents the dependency.
            git-lfs

            # General RE / analysis helpers
            binwalk
            squashfsTools # unsquashfs (xz) — extract Raritan PP/SquashFS rootfs
            dtc # device tree compiler — decompile firmware DTBs
            hexyl # friendly hex viewer
            file
            ripgrep

            # Password / hash cracking (Raritan PX3 root $5$ hash — see
            # devices/raritan-px3-5475v). john has OpenCL/Metal on Apple silicon.
            john

            # Android RE (devices/amzn-pinnacles — Amazon "yacht"/KFYAWI).
            # adb + fastboot; Amazon USB VID is 0x1949, pass `-i 0x1949`.
            android-tools
            scrcpy # mirror/control the screen over adb — locked-device triage
            jadx # dex -> java, for pulling apart Amazon system APKs
            apktool # apk resource/smali unpack
            # NOTE: abootimg is linux-only; use `binwalk` (above) or
            # android-tools' unpack_bootimg for boot.img on darwin.
            simg2img # android sparse image -> raw, for pulled partitions
            payload-dumper-go # extract partitions from OTA payload.bin

            # Thumb-2 disassembly for the Amazon LK bootloader. Ghidra decodes
            # that payload as garbage unless TMode is set, so capstone is the
            # quicker route for targeted reads. See devices/amzn-pinnacles.
            # unicorn: emulate LK functions directly (full-system QEMU is out —
            # there is no MT8183 machine model). Gives the instruction-level
            # visibility the locked device refuses to provide.
            (python3.withPackages (ps: with ps; [ capstone pyelftools unicorn ]))
          ];

          shellHook = ''
            echo "ai_re devshell — hardware RE"
            echo "serial: tmux new-session -d -s serial 'tio /dev/<port> -b 115200'"
            echo "ports : ls /dev/tty.* (macOS) | ls /dev/ttyUSB* /dev/ttyACM* (linux)"
          '';
        };
      });
}

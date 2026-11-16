{
  description = "ai_re — hardware reverse engineering devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # Systems exported from ./nix track nixos-unstable, pinned apart from the
    # devshell so a devshell bump never rebuilds a switch (and vice versa).
    nixpkgs-systems.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, nixpkgs-systems, flake-utils }:
    let
      # Shared packages / NixOS modules / system configs: see nix/README.md.
      # A git flake only sees tracked files: until ./nix is committed (or
      # "git add -N"ed) the exports are simply absent and the devshell still
      # works. "nix build path:./nix#..." works either way.
      shared = nixpkgs.lib.optionalAttrs (builtins.pathExists ./nix/default.nix) (
        let
          aiRe = import ./nix;
          # The monorepo's Rust tools (fand). Reachable here because this is a
          # git flake over the whole repository.
          rust = if builtins.pathExists ../rust/crates/fand/nix/module.nix
            then {
              nixosModules.fand = ../rust/crates/fand/nix/module.nix;
              nixosModules.consoled = ../rust/consoled/nix/module.nix;   # its own workspace
            } else null;
          systems = aiRe.systemsFor { nixpkgs = nixpkgs-systems; inherit rust; };
        in {
          inherit (aiRe) nixosModules;
          inherit (systems) nixosConfigurations;
          packages.x86_64-linux =
            aiRe.packagesFor (import nixpkgs-systems { system = "x86_64-linux"; }) // systems.packages;
          # On a DMC-12: nix run --refresh 'github:nepeat/nepeat?dir=ai_re#dmc12-firmware' -- status
          packages.aarch64-linux.dmc12-firmware = (import ./nix/packages/dmc12-firmware {
            pkgs = import nixpkgs-systems { system = "aarch64-linux"; };
          }).tool;
        });
    in
    nixpkgs.lib.recursiveUpdate shared (flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        # OpenWrt's host tools and downloaded cross-toolchains expect an FHS
        # layout. Keep that environment reproducible inside the RE devshell.
        openwrtBuildShell = pkgs.buildFHSEnv {
          name = "openwrt-build-shell";
          targetPkgs = p: with p; [
            bash coreutils findutils diffutils gnugrep gnused gawk getopt
            gcc gnumake binutils glibc glibc.dev zlib zlib.dev
            ncurses ncurses.dev openssl openssl.dev
            git curl wget cacert patch perl python3 python3Packages.setuptools
            rsync unzip zip gnutar gzip bzip2 xz zstd file which util-linux
            bison flex gettext pkg-config time cpio swig
          ];
          profile = ''
            export OPENWRT_FHS=1
            export OPENWRT_HOST_LTO_PLUGIN="${pkgs.gcc.cc}/libexec/gcc/${pkgs.stdenv.hostPlatform.config}/${pkgs.gcc.cc.version}/liblto_plugin.so"
          '';
          runScript = "bash";
        };

        # --- Ghidra MCP server (headless, stdio) ---------------------------
        # pyghidra-mcp (clearbluejar, Apache-2.0) + its one non-nixpkgs
        # dependency ghidrecomp, built against nixpkgs' pyghidra/chromadb/mcp
        # and wired to nixpkgs' Ghidra. Kept inline (not a tools/*.nix file)
        # because flakes only see git-tracked files. Pinned by tag + source hash.
        # Network surface: Ghidra's optional PDB symbol server (off via
        # --no-symbols in .mcp.json) and chromadb's ONNX-model download (code
        # index, patched off below); see tools/README.md.
        ghidrecomp = pkgs.python3Packages.buildPythonPackage rec {
          pname = "ghidrecomp";
          version = "0.5.9";
          pyproject = true;
          build-system = [ pkgs.python3Packages.setuptools ];
          src = pkgs.fetchFromGitHub {
            owner = "clearbluejar";
            repo = "ghidrecomp";
            tag = "v${version}";
            hash = "sha256-r8jMfK5oFvyd1eFgf4wJrDfhhUzGEUyPf+BmOrx2OSU=";
          };
          dependencies = [ pkgs.python3Packages.pyghidra ];
          doCheck = false;
        };
        pyghidraMcpUnwrapped = pkgs.python3Packages.buildPythonApplication rec {
          pname = "pyghidra-mcp";
          version = "0.3.0";
          pyproject = true;
          build-system = [ pkgs.python3Packages.hatchling ];
          src = pkgs.fetchFromGitHub {
            owner = "clearbluejar";
            repo = "pyghidra-mcp";
            tag = "v${version}";
            hash = "sha256-e5/tVFf4GCJ2mA4kxRko1LzpR6WP34qCgdnPY5ArH74=";
          };
          dependencies = with pkgs.python3Packages; [
            click click-option-group mcp pyghidra chromadb ghidrecomp
          ];
          doCheck = false;
          pythonRelaxDeps = true; # upstream lower bounds vs nixpkgs versions
          # chromadb's default embedder downloads an ONNX model from
          # chroma-onnx-models.s3.amazonaws.com the first time the code index is
          # filled, and upstream does that automatically at startup. Turn the
          # semantic code index off (search_code then reports "not indexed");
          # every other tool is unaffected and the server stays fully offline.
          postPatch = ''
            substituteInPlace src/pyghidra_mcp/indexing_mixin.py --replace-fail \
              "        code: bool = True,
                    strings: bool = True,
                ) -> bool:" \
              "        code: bool = False,
                    strings: bool = True,
                ) -> bool:"
          '';
        };
        # Pin Ghidra to the nixpkgs build so GHIDRA_INSTALL_DIR is never needed.
        pyghidraMcp = pkgs.symlinkJoin {
          name = "pyghidra-mcp-${pyghidraMcpUnwrapped.version}";
          paths = [ pyghidraMcpUnwrapped ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            wrapProgram $out/bin/pyghidra-mcp \
              --set-default GHIDRA_INSTALL_DIR ${pkgs.ghidra}/lib/ghidra
          '';
          meta.mainProgram = "pyghidra-mcp";
        };
        # python with pyghidra for tools/ghidra/*.py (project seeding scripts).
        ghidraPy = pkgs.writeShellApplication {
          name = "ghidra-py";
          runtimeInputs = [ (pkgs.python3.withPackages (ps: [ ps.pyghidra ])) ];
          text = ''
            export GHIDRA_INSTALL_DIR=${pkgs.ghidra}/lib/ghidra
            exec python3 "$@"
          '';
        };
      in
      {
        # `nix run .#pyghidra-mcp -- ...` (used by .mcp.json; no devshell shellHook
        # noise on stdout, which would corrupt the MCP stdio stream).
        packages = pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          pyghidra-mcp = pyghidraMcp;
        };

        devShells.default = pkgs.mkShell {
          name = "ai_re";

          # Add tools here as RE tasks require them. Note the reason in the
          # commit that adds a package.
          packages = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            openwrtBuildShell
            # Only verified on x86_64-linux; not evaluated on darwin.
            pyghidraMcp
            ghidraPy
          ] ++ (with pkgs; [
            # Lab metrics: single-node VictoriaMetrics as a user service with
            # drop-in scrape targets, plus Grafana with dashboards provisioned
            # from tools/grafana/dashboards (tools/metrics.py, tools/README.md).
            victoriametrics
            grafana
            (writeShellApplication {
              name = "ai-re-metrics";
              runtimeInputs = [ python3 victoriametrics grafana ];
              text = ''exec python3 ${./tools/metrics.py} "$@"'';
            })

            # Home Assistant outlet control; credentials stay in the environment.
            (writeShellApplication {
              name = "lab-power";
              runtimeInputs = [ python3 ];
              text = ''exec python3 ${./tools/lab_power.py} "$@"'';
            })

            # Read-only catalog of device dumps: tags, verify, inspect, dupes,
            # zstd-qcow2 export (tools/images.py, tools/README.md).
            (writeShellApplication {
              name = "ai-re-images";
              runtimeInputs = [ python3 zstd xz util-linux ubootTools qemu-utils file ];
              text = ''exec python3 ${./tools/images.py} "$@"'';
            })

            # Static analysis of the TN48M AP806 BootROM
            # (devices/delta-tn48m/bootrom.md). nixpkgs `ghidra` is the
            # source build and substitutes from cache.nixos.org (no compile).
            # pyghidraMcp: headless MCP server so Claude Code can drive it;
            # ghidraPy: python+pyghidra for tools/ghidra/ scripts.
            ghidra

            # Search hardware reference-manual PDF register definitions.
            poppler-utils

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
            # Edge-Core AS7760: inspect acquired FAT32 SD and JFFS2 NOR images.
            mtools
            jefferson
            dtc # device tree compiler — decompile firmware DTBs
            hexyl # friendly hex viewer
            file
            ripgrep
            atftp # TFTP client/server for read-only CFE firmware acquisition
            # CS8210-32X SSD acquisition and read-only ext filesystem analysis.
            zstd
            smartmontools
            e2fsprogs
            # CS8210 boot-image packaging and offline ARM64 smoke tests.
            ubootTools
            qemu
            # CS8210 I2C FPGA: simulate the netlist recovered from the
            # bitstream (verilator: the whole 52k-primitive design per cycle
            # is too slow in Python); yosys for a planned equivalence check of
            # the two channel templates (not run yet). See devices/cs8210-32x/fpga.
            verilator
            yosys

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

            # Amazon's official Fire HD 10 (11th gen) GPL drop contains the
            # exact MT8183 Linux 4.4.146 tree used by amzn-pinnacles.  These
            # are needed to prepare it and build small arm64 test modules.
            pkgsCross.aarch64-multiplatform.stdenv.cc
            gnumake
            bc
            bison
            flex
            openssl
            perl
          ]);

          shellHook = ''
            echo "ai_re devshell — hardware RE"
            echo "serial: tmux new-session -d -s serial 'tio /dev/<port> -b 115200'"
            echo "ports : ls /dev/tty.* (macOS) | ls /dev/ttyUSB* /dev/ttyACM* (linux)"
          '';
        };
      }));
}

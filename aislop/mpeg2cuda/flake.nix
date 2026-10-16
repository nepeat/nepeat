{
  description = "mpeg2_cuda: experimental CUDA MPEG-2 video encoder inside FFmpeg";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
    let
      pkgs = import nixpkgs {
        inherit system;
        config = {
          allowUnfree = true;
          cudaSupport = true;
        };
      };

      # Pin to the CUDA version the host driver supports (595.x -> 13.2);
      # newer nvcc emits PTX the driver refuses to JIT.
      cuda = pkgs.cudaPackages_13_2;

      # nvcc needs a host compiler it supports; backendStdenv pins one.
      mkShell = pkgs.mkShell.override { stdenv = cuda.backendStdenv; };

      lib = pkgs.lib;

      # FFmpeg = upstream release + patches/ (the encoder lives only in the
      # patch series; ffmpeg/ is a scratch tree made by scripts/ffmpeg_tree.sh)
      patches = map (f: ./patches + "/${f}")
        (lib.filter (lib.hasSuffix ".patch") (lib.attrNames (builtins.readDir ./patches)));

      ffmpeg = cuda.backendStdenv.mkDerivation {
        pname = "ffmpeg-mpeg2cuda";
        version = "9.0.2";
        src = pkgs.fetchurl {
          url = "https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz";
          hash = "sha256-jDhQKD6yX6AmSCB4oEBR4L4XNHsJ74GghJvsFaluAC4=";
        };
        inherit patches;

        # autoAddDriverRunpath: find the host libcuda/libnvcuvid at /run/opengl-driver
        nativeBuildInputs = [ cuda.cuda_nvcc pkgs.autoAddDriverRunpath pkgs.nasm pkgs.pkg-config pkgs.perl ];
        buildInputs = [ cuda.cuda_cudart cuda.cccl pkgs.nv-codec-headers-12 ];

        # one flag per line, some contain spaces (--nvccflags)
        __structuredAttrs = true;
        configureFlags = lib.filter (f: f != "")
          (lib.splitString "\n" (builtins.readFile ./scripts/ffmpeg_flags));
        configurePlatforms = [ ];
        setOutputFlags = false;
        dontAddDisableDepTrack = true;
        dontDisableStatic = true;     # FFmpeg builds static libs by default (m2c_bench links them)
        enableParallelBuilding = true;

        # nonfree (--enable-cuda-nvcc): local use only, not redistributable
        meta.license = lib.licenses.unfree;
      };
    in
    {
      packages.ffmpeg = ffmpeg;
      packages.default = ffmpeg;

      devShells.default = mkShell {
        name = "mpeg2cuda";

        packages = [
          # CUDA toolchain (nvcc + headers for device code)
          cuda.cuda_nvcc
          cuda.cuda_cudart
          cuda.cccl
          cuda.cuda_sanitizer_api   # compute-sanitizer for the memcheck test

          # FFmpeg build deps (minimal config, see scripts/configure.sh)
          pkgs.nv-codec-headers-12
          pkgs.nasm
          pkgs.pkg-config
          pkgs.gnumake
          pkgs.perl

          # test/benchmark helpers
          pkgs.libmpeg2           # mpeg2dec: independent decoder for cross-checks
          cuda.nsight_systems
          (pkgs.python3.withPackages (ps: [ ps.numpy ]))
          pkgs.sysstat
        ];

        shellHook = ''
          # libcuda.so / libnvcuvid.so come from the host NVIDIA driver
          export LD_LIBRARY_PATH=/run/opengl-driver/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
          export MPEG2CUDA_ROOT=${toString ./.}
        '';
      };
    });
}

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
    in
    {
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

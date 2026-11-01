{ pkgs, ... }:
{
  nix.settings = {
    substituters = [
      "https://cache.nixos-cuda.org"
    ];

    trusted-public-keys = [
      "cache.nixos-cuda.org:74DUi4Ye579gUqzH4ziL9IyiJBlDpMRn9MBN8oNan9M="
    ];
  };

  nixpkgs.config.cudaSupport = true;
  nixpkgs.overlays = [
    (_final: prev: let
      # patchelf 0.19.2 corrupts cuda-gdbserver's ELF layout, then asserts on
      # the driver RUNPATH pass. compute-sanitizer and tensorrt_player hit
      # the same assertion.
      # Use the working version only for these CUDA binary packages.
      cudaPatchelf = prev.patchelf.overrideAttrs {
        version = "0.15.2";
        src = prev.fetchurl {
          url = "https://github.com/NixOS/patchelf/releases/download/0.15.2/patchelf-0.15.2.tar.bz2";
          hash = "sha256-F3RfVkFZyOIo/EEtplogSLhGxLa0Igt3y/IkFuAvLXw=";
        };
      };
      withCudaPatchelf = package:
        package.overrideAttrs (old: {
          nativeBuildInputs = [ cudaPatchelf ] ++ (old.nativeBuildInputs or [ ]);
        });
    in {
      cudaPackages = prev.cudaPackages.overrideScope (_cudaFinal: cudaPrev: {
        cuda_gdb = withCudaPatchelf cudaPrev.cuda_gdb;
        cuda_sanitizer_api = withCudaPatchelf cudaPrev.cuda_sanitizer_api;
        tensorrt = withCudaPatchelf cudaPrev.tensorrt;
      });
      # The binary-tool workaround should not invalidate ONNX Runtime's
      # cached CUDA build. Its runtime uses the stock CUDA dependency set.
      onnxruntime = prev.onnxruntime.override { cudaPackages = prev.cudaPackages; };
    })
  ];
  hardware.graphics.enable = true;
  services.xserver.videoDrivers = [ "nvidia" ];
  hardware.nvidia.open = true; 
  environment.systemPackages = with pkgs; [
    cudatoolkit
  ];
  hardware.nvidia-container-toolkit.enable = true;
}

# al_eth in Rust (./rust: al_eth.rs + hal/ + knet/): the DMC-12 path (RGMII,
# fixed 1000/full, no PHY) of the C module in ./default.nix, which stays the
# default (hardware.alpineV2.ethDriver) and keeps every mode the Rust one
# refuses (CS8210: SGMII + PHY, 10G serial). hal is checked against the
# vendor C by ./rust.nix; knet's QEMU tests are in ../kernel-rust.
# Needs a kernel with CONFIG_RUST and the net bindings patch (linux-alpine
# rust = true + rustNet = true; NixOS: hardware.alpineV2.kernelRustNet).
#   kernelPackages.callPackage ./rust-module.nix { }                       # module "al_eth"
#   kernelPackages.callPackage ./rust-module.nix { moduleName = "al_eth_rs"; }
# "al_eth_rs" is the same code under another module and PCI driver name, to
# insmod on a system running the C al_eth (after rmmod al_eth) without
# shadowing it; never install it into a system's module tree (both would bind
# by the Ethernet class alias).
#   kernelPackages.callPackage ./rust-module.nix { moduleName = "al_eth_rs"; trace = true; }
# trace: every register/config access and delay recorded into debugfs
# al_eth_rs/trace (rust/trace.rs; ./trace/rs2txt.py -> the C trace's text, ./trace/parse.py).
{ lib, stdenv, kernel, buildPackages, moduleName ? "al_eth", trace ? false }:
assert lib.assertMsg (trace -> moduleName == "al_eth_rs")
  "al-eth-rs-module: the tracing build is the al_eth_rs module only";
assert lib.assertMsg (kernel.withRust or false)
  "al-eth-rs-module: kernel ${kernel.name} is built without CONFIG_RUST";
assert lib.assertMsg (lib.any (p: (p.name or "") == "rust-net-bindings-and-helpers") (kernel.kernelPatches or [ ]))
  "al-eth-rs-module: kernel ${kernel.name} lacks the rust net bindings patch (linux-alpine rustNet = true)";
assert lib.assertMsg (lib.elem moduleName [ "al_eth" "al_eth_rs" ])
  "al-eth-rs-module: moduleName is al_eth or al_eth_rs";
stdenv.mkDerivation {
  pname = "${moduleName}-rust${lib.optionalString trace "-trace"}";
  inherit (kernel) version;
  src = builtins.path {
    path = ./rust;
    name = "al-eth-rust-src";
    filter = p: t: let r = lib.removePrefix (toString ./rust + "/") (toString p); in
      r == "al_eth.rs" || r == "trace.rs" || r == "Kbuild" || r == "board_params.rs" || r == "hal" || r == "knet"
      || ((lib.hasPrefix "hal/" r || lib.hasPrefix "knet/" r) && lib.hasSuffix ".rs" r);
  };
  __structuredAttrs = true;
  strictDeps = true;
  hardeningDisable = [ "all" ];
  # rustc + bindgen come with a Rust-enabled kernel's moduleBuildDependencies.
  nativeBuildInputs = kernel.moduleBuildDependencies ++ [ buildPackages.llvm buildPackages.python3 ];
  stackCheck = ./stack-check.py;
  makeFlags = kernel.commonMakeFlags ++ [
    "-C" "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    # no kernel.dev store path in the module (panic locations)
    "KRUSTFLAGS=--remap-path-prefix=${kernel.dev}/=/kernel/ -Dwarnings"
    "AL_ETH_MODNAME=${moduleName}"
  ] ++ lib.optional trace "AL_ETH_TRACE=1";
  # Stack guard (kernel stacks are 16 KiB; 2026-11-23 the first build overflowed
  # it in probe with an 11.7 KiB frame): a second build of the same sources with
  # -Zemit-stack-sizes, then ./stack-check.py over the object: every frame
  # <= 1 KiB, every chain of module frames from a kernel entry point <= 4 KiB.
  # The installed module is the normal build (no .stack_sizes section).
  buildPhase = ''
    runHook preBuild
    make "''${makeFlags[@]}" M=$(pwd) modules
    sc=$TMPDIR/stackcheck
    mkdir -p $sc
    cp -r al_eth.rs trace.rs Kbuild board_params.rs hal knet $sc/
    make "''${makeFlags[@]}" M=$sc "KRUSTFLAGS=--remap-path-prefix=${kernel.dev}/=/kernel/ -Dwarnings -Zemit-stack-sizes" modules \
      2>&1 | grep -v -E "non-allocatable section '.stack_sizes'|Did you forget|Note that for example|section definitions for use" || true
    obj=$sc/al_eth.o
    python3 $stackCheck llvm-readobj llvm-objdump llvm-cxxfilt $obj --frame 1024 --path 4096 | tee stack-check.txt
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    make "''${makeFlags[@]}" M=$(pwd) INSTALL_MOD_PATH=$out modules_install
    install -Dm644 stack-check.txt $out/share/al-eth/stack-check.txt
    ${lib.optionalString trace "install -Dm755 ${./trace/rs2txt.py} $out/share/al-eth/rs2txt.py; install -Dm755 ${./trace/parse.py} $out/share/al-eth/parse.py"}
    runHook postInstall
  '';
  meta = {
    description = "Annapurna Labs al_eth, RGMII fixed-link path, in Rust (module ${moduleName})";
    license = lib.licenses.gpl2Only;
    platforms = [ "aarch64-linux" ];
  };
}

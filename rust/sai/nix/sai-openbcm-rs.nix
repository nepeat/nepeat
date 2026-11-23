# The SAI package of the AS7760 / AS7772 (hardware.openbcm.package, ai_re/nix/modules/switch/openbcm.nix):
# the Rust libsai and its switch tools under the names the C++ sai-openbcm had (deleted 2026-11-23),
# so the NixOS modules, units and scripts kept their paths (README "Drop-in package").
#   with the SDK (the switch; big-endian ppc64, links the Broadcom SDK: unfree, not redistributable):
#     ppc.callPackage ./sai-openbcm-rs.nix {
#       saiSrc = <sonic-sairedis source>;            # SAI headers (sonic.sources.sairedis)
#       libsdk = <packages/openbcm libsdk { }>;
#       glueSrc = <ai_re/nix/packages/sai-openbcm/src>;   # sdk_glue.c / sdk_glue.h (the glue stays C)
#     }
#   without (any platform: the VM tests, a build host's table checkers):
#     pkgs.callPackage ./sai-openbcm-rs.nix { saiSrc = ...; glueSrc = ...; }   (libsdk = null)
#   saiRs (optional): the workspace build (./package.nix) for the same pkgs, to share one that
#     exists already (ai_re/tests/nixtargets.py passes its own); default: ./package.nix { saiSrc;
#     glueSrc; } (natively its check phase runs the workspace's tests).
#
# Installs:
#   lib/libsai.so -> libsai.so.1 -> libsai.so.1.0.0   sai-ffi; with libsdk feature openbcm-real (the
#                                  openbcm backend on sdk_glue.c + libsdk), without openbcm-fake (the
#                                  virtual backend + the openbcm backend on sdk-glue's in-memory chip);
#                                  -z defs, soname libsai.so.1, the 17 exports of the virtual-backend
#                                  library: ./libsai-openbcm.nix's checks
#   bin/openbcm-diag               crate openbcm-diag (openbcm-diag-rs)
#   bin/openbcm-mdio-script        crate mdio-tool (openbcm-mdio-script-rs); with libsdk feature sdk-real;
#                                  without: diag socket and --check only
#   bin/openbcm-ext-phy, bin/openbcm-ext-macsec   crate phy-seq's checkers (openbcm-ext-*-rs): --check,
#                                  --render (ext-macsec)
#   bin/openbcm-asic-temp          crate openbcm-asic-temp, feature sdk-real: with libsdk only
#   bin/sai-openbcm-smoke          crates/sai-backend-openbcm/smoke (refuses without --sonic-is-stopped)
#   share/sai-openbcm/backends     "fake openbcm" with libsdk, "fake" without
#   share/sai-openbcm/phy-seq-engine, macsec-model   "rust"
#   share/sai-rs/libsai-openbcm.txt   the library's link report
# The native library also answers SAI_OPENBCM_BACKEND=openbcm (in-memory glue). The binaries are
# copies, so the result does not keep the build derivations alive.
# Cargo invocations: the workspace build (saiRs: diag, MDIO tool without the SDK, table checkers;
# the export reference), and one for the library: with libsdk together with the two SDK tools
# (sai-ffi's openbcm-real, sdk-real of mdio-tool and openbcm-asic-temp: all on the real glue, so they
# share one sdk-glue-sys; the fake glue's dev-dependencies are not built there).
{
  lib,
  stdenv,
  callPackage,
  saiSrc,
  glueSrc,
  libsdk ? null,
  saiRs ? callPackage ./package.nix { inherit saiSrc glueSrc; },
}:
let
  withSdk = libsdk != null;
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  # libsai with the openbcm backend and the smoke program (+ the SDK tools in the same build)
  base = callPackage ./libsai-openbcm.nix ({ inherit saiRs; } // lib.optionalAttrs withSdk {
    inherit libsdk glueSrc;
  });
  parts = if !withSdk then base else base.overrideAttrs (old: {
    # unqualified features: each goes to the selected packages that have it (sai-ffi: openbcm-real;
    # mdio-tool, openbcm-asic-temp: sdk-real); "pkg/feature" would not satisfy openbcm-asic-temp's
    # required-features, and its binary would silently not be built
    cargoBuildFlags = [
      "-p" "sai-ffi" "-p" "mdio-tool" "-p" "openbcm-asic-temp"
      "--features" "openbcm-real,sdk-real"
    ];
    # the tools find libsdk at run time (as ./mdio-tool.nix)
    env = old.env // { SDK_GLUE_RPATH = "${libsdk}/lib"; };
  });
  mdioTool = if withSdk then "${parts}/bin/openbcm-mdio-script-rs" else "${saiRs}/bin/openbcm-mdio-script-rs";
  tools = {
    openbcm-diag = "${saiRs}/bin/openbcm-diag-rs";
    openbcm-mdio-script = mdioTool;
    openbcm-ext-phy = "${saiRs}/bin/openbcm-ext-phy-rs";
    openbcm-ext-macsec = "${saiRs}/bin/openbcm-ext-macsec-rs";
    sai-openbcm-smoke = "${parts}/bin/sai-openbcm-smoke";
  } // lib.optionalAttrs withSdk { openbcm-asic-temp = "${parts}/bin/openbcm-asic-temp"; };
in
stdenv.mkDerivation {
  pname = if withSdk then "sai-openbcm-rs" else "sai-openbcm-rs-fake";
  inherit (saiRs) version;
  dontUnpack = true;
  dontConfigure = true;
  dontBuild = true;
  # copies of finished binaries: no strip, no RUNPATH shrinking (libsdk's entry must stay)
  dontFixup = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib $out/bin $out/share/sai-openbcm $out/share/sai-rs
    cp ${parts}/lib/libsai.so.1.0.0 $out/lib/
    ln -s libsai.so.1.0.0 $out/lib/libsai.so.1
    ln -s libsai.so.1 $out/lib/libsai.so
    # the report names the library by its build path: by this one's instead (no reference kept)
    sed "s|${parts}|$out|g" ${parts}/share/sai-rs/libsai-openbcm.txt > $out/share/sai-rs/libsai-openbcm.txt
    ${lib.concatStrings (lib.mapAttrsToList (name: from: ''
      install -m755 ${from} $out/bin/${name}
    '') tools)}
    echo ${if withSdk then "fake openbcm" else "fake"} > $out/share/sai-openbcm/backends
    echo rust > $out/share/sai-openbcm/phy-seq-engine
    echo rust > $out/share/sai-openbcm/macsec-model
    ${lib.optionalString withSdk ''
      # the SDK tools link libsdk and carry the glue inside (as libsai: ./libsai-openbcm.nix)
      for t in openbcm-asic-temp openbcm-mdio-script; do
        ${bin}readelf -d $out/bin/$t | grep -q 'NEEDED.*libsdk' || { echo "$t: libsdk is not NEEDED"; exit 1; }
        ${bin}readelf -d $out/bin/$t | grep -q "R.*PATH.*${libsdk}/lib" || { echo "$t: no run path to libsdk"; exit 1; }
        if ${bin}nm -D --undefined-only $out/bin/$t | grep -q ' ob_sdk_'; then echo "$t: ob_sdk_* symbols are undefined"; exit 1; fi
      done
    ''}
    runHook postInstall
  '';

  passthru = {
    inherit libsdk;
    inherit (saiRs) saiHeaders;
    tools = builtins.attrNames tools;
    parts = { inherit saiRs parts; };
  };

  meta = {
    description = "The Rust SAI (libsai.so) and its switch tools under sai-openbcm's names (${if withSdk then "Broadcom SDK" else "virtual backend + in-memory glue"})";
    license = if withSdk then [ lib.licenses.asl20 lib.licenses.unfreeRedistributable ] else lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}

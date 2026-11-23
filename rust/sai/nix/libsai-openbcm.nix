# libsai.so of the Rust SAI with the openbcm backend (crate sai-backend-openbcm) linked in,
# and the hardware smoke program. README "Hardware backend".
#   real glue (the switch; big-endian ppc64, links the Broadcom SDK: unfree, not redistributable):
#     ppc.callPackage ./libsai-openbcm.nix {
#       saiRs = <./package.nix for the same pkgs>;
#       libsdk = ppcOpenbcm.libsdk { };
#       glueSrc = <ai_re/nix/packages/sai-openbcm/src>;      # sdk_glue.c, sdk_glue.h
#     }
#   fake glue (any platform; the same code path over sdk-glue's Rust fake of the glue, no C, no chip):
#     pkgs.callPackage ./libsai-openbcm.nix { saiRs = <./package.nix>; }
#     its check phase runs the smoke program against the library it built.
#
# The real glue is sdk_glue.c compiled here exactly as sai-openbcm's Makefile compiles it (the
# SDK's own cflags), put in a static archive and linked into libsai.so together with libsdk;
# sdk-glue-sys checks its declarations against the same sdk_glue.h (required).
#
# Checked at build time, without running anything (results in $out/share/sai-rs/libsai-openbcm.txt):
#   - the link itself runs with -z defs: no undefined symbol
#   - soname libsai.so.1
#   - exported symbols: exactly those of the virtual-backend library of saiRs
#   - real glue: NEEDED has libsdk.so, and the glue's functions are inside (not imported)
{
  lib,
  stdenv,
  saiRs,
  libsdk ? null,
  glueSrc ? null,
}:
let
  real = libsdk != null;
  feature = if real then "openbcm-real" else "openbcm-fake";
  cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
in
assert real -> glueSrc != null;
saiRs.overrideAttrs (old: {
  pname = "sai-rs-libsai-${if real then "openbcm" else "openbcm-fake"}";

  # Only the library: the other workspace members (tools, tests) are package.nix's.
  cargoBuildFlags = [ "-p" "sai-ffi" "--features" feature ];

  env = (old.env or { }) // {
    # every link for the target, i.e. libsai.so: an unresolved symbol fails the build
    NIX_LDFLAGS = "-z defs";
  } // lib.optionalAttrs real {
    SDK_GLUE_INC = "${glueSrc}";
    SDK_GLUE_SYS_CHECK = "require";
  };

  preBuild = (old.preBuild or "") + lib.optionalString real ''
    # The SDK glue is C, compiled with exactly the SDK's own defines (sai-openbcm's Makefile).
    mkdir -p "$NIX_BUILD_TOP/glue"
    ${cc} -std=gnu11 -fPIC -Wall -Wno-unused-function -O2 -g \
      $(cat ${libsdk}/share/openbcm/cflags) \
      -I${libsdk}/include/openbcm/include -I${libsdk}/include/openbcm/bde \
      -I${libsdk}/include/openbcm/kmod -I${libsdk}/include/openbcm/phymod \
      -I${glueSrc} -c ${glueSrc}/sdk_glue.c -o "$NIX_BUILD_TOP/glue/sdk_glue.o"
    ${bin}ar rcs "$NIX_BUILD_TOP/glue/libob_sdk_glue.a" "$NIX_BUILD_TOP/glue/sdk_glue.o"
    export SDK_GLUE_LINK="$NIX_BUILD_TOP/glue/libob_sdk_glue.a -L${libsdk}/lib -lsdk -lpthread"
  '';

  # Fake glue on a host that can run it: the backend's and the glue wrapper's tests, with the
  # smoke program driving the library built above through dlopen.
  doCheck = !real && canRun;
  cargoTestFlags = [ "-p" "sai-backend-openbcm" "-p" "sdk-glue" ];
  preCheck = ''
    export SAI_SMOKE_LIBSAI="$PWD/target/${triple}/$cargoBuildType/libsai.so"
    test -f "$SAI_SMOKE_LIBSAI"
  '';

  postInstall = (old.postInstall or "") + ''
    # the smoke program (a C program against sai.h; loads the library with dlopen)
    mkdir -p $out/bin $out/share/sai-rs
    ${cc} -std=gnu11 -Wall -Wextra -Werror -O1 -g -I ${saiRs.saiHeaders} -I ${saiRs.saiHeaders}/../experimental \
      crates/sai-backend-openbcm/smoke/sai_smoke.c -o $out/bin/sai-openbcm-smoke -ldl -lpthread

    so=$out/lib/libsai.so.1.0.0
    report=$out/share/sai-rs/libsai-openbcm.txt
    exports() { ${bin}nm -D --defined-only "$1" | awk '{ print $3 }' | LC_ALL=C sort; }
    {
      echo "libsai.so with the openbcm backend, ${if real then "real glue (sdk_glue.c + libsdk)" else "FAKE glue (no chip)"}, ${stdenv.hostPlatform.config}"
      echo "size: $(stat -c %s $so) bytes"
      echo "== dynamic section"
      ${bin}readelf -d $so | grep -E 'SONAME|NEEDED|RUNPATH|RPATH'
      echo "== exported symbols"
      exports $so
      echo "== undefined symbols: $(${bin}nm -D --undefined-only $so | wc -l), by library version"
      ${bin}nm -D --undefined-only $so | sed -n 's/.*@\(.*\)/\1/p' | LC_ALL=C sort | uniq -c
    } > $report
    cat $report

    ${bin}readelf -d $so | grep -q 'SONAME.*\[libsai.so.1\]' || { echo "wrong soname"; exit 1; }
    # the same exports as the library without this backend: nothing of the glue or the SDK leaks
    exports $so > exports.openbcm
    exports ${saiRs}/lib/libsai.so > exports.virtual
    diff -u exports.virtual exports.openbcm || { echo "exported symbols differ from the virtual-backend library"; exit 1; }
    # the glue is inside, not imported
    if ${bin}nm -D --undefined-only $so | grep -q ' ob_sdk_'; then echo "ob_sdk_* symbols are undefined"; exit 1; fi
    ${lib.optionalString real ''
      ${bin}readelf -d $so | grep -q 'NEEDED.*libsdk' || { echo "libsdk is not NEEDED"; exit 1; }
    ''}
  '';

  passthru = (old.passthru or { }) // { inherit libsdk; glue = if real then "real" else "fake"; };

  meta = old.meta // {
    description = "Rust libsai.so with the openbcm backend (${if real then "Broadcom SDK" else "fake glue"}) and the hardware smoke program";
    license = if real then [ lib.licenses.asl20 lib.licenses.unfreeRedistributable ] else lib.licenses.asl20;
  };
})

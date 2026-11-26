# tl-frames: the five hardware test frame tools of the CS8210 SAI tests in Rust (crate tl-frames,
# README "tl-frames"), the ports of ai_re/nix/packages/sai-teralynx/tools/{trap,qos,raw,l3}_frames.c and
# sflow_probe.c (read only here):
#   native:  pkgs.callPackage ./tl-frames.nix { saiRs = <./package.nix>; }
#   aarch64: pkgs.pkgsCross.aarch64-multiplatform.callPackage ./tl-frames.nix { saiRs = <./package.nix for those pkgs>; }
#
# $out/bin: trap-frames qos-frames raw-frames l3-frames sflow-probe (same command lines, output and
# exit codes as the C tools; they are behind the crate's "cli" feature, so sai-rs itself does not carry them).
#
# Native checks (a cross build runs none): the crate's fmt, clippy -D warnings and tests, and the
# differential against the C tools compiled from the sources (crates/tl-frames/tests/differential.rs): every
# scenario runs both over veth pairs inside an unprivileged network namespace and compares exit status,
# output and the frames on the wire. It runs in the nix build sandbox (user namespaces work there); requireDiff
# (default true) makes the build fail instead of skipping where they do not.
{
  lib,
  stdenv,
  saiRs,
  iproute2,
  util-linux,
  teralynxSrc ? ../../../ai_re/nix/packages/sai-teralynx,
  requireDiff ? true,
}:
let
  # the C sources only (the package itself is not touched)
  cTools = builtins.path {
    path = teralynxSrc + "/tools";
    name = "sai-teralynx-frame-tools-src";
    filter = path: type: type == "directory" || lib.hasSuffix ".c" path;
  };
in
saiRs.overrideAttrs (old: {
  pname = "tl-frames";
  cargoBuildFlags = [ "-p" "tl-frames" "--features" "cli" ];
  cargoTestFlags = [ "-p" "tl-frames" "--features" "cli" ];
  # only what this crate needs: not the board tables or the frozen frontend the workspace goldens read
  env = builtins.removeAttrs old.env [ "PHY_SEQ_TABLES" "SAI_OPENBCM_SRC" ]
    // { TL_FRAMES_C_TOOLS = "${cTools}"; }
    // lib.optionalAttrs requireDiff { TL_FRAMES_REQUIRE_DIFF = "1"; };
  nativeCheckInputs = old.nativeCheckInputs ++ [ iproute2 util-linux ];
  preCheck = ''
    cargo fmt -p tl-frames --check
    cargo clippy --offline -p tl-frames --all-targets --features cli --profile "$cargoCheckType" -- -D warnings
  '';
  postCheck = "";
  postInstall = "";
  passthru = { inherit cTools; };
  meta = old.meta // { description = "Hardware test frame senders and counters of the CS8210 SAI tests, in Rust"; };
})

# tl-frames: the five hardware test frame tools of the CS8210 SAI tests in Rust (crate tl-frames,
# README "tl-frames"), the ports of the C tools {trap,qos,raw,l3}_frames.c and sflow_probe.c of the C++
# sai-teralynx (deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0):
#   native:  pkgs.callPackage ./tl-frames.nix { saiRs = <./package.nix>; }
#   aarch64: pkgs.pkgsCross.aarch64-multiplatform.callPackage ./tl-frames.nix { saiRs = <./package.nix for those pkgs>; }
#
# $out/bin: trap-frames qos-frames raw-frames l3-frames sflow-probe (same command lines, output and
# exit codes as the C tools; they are behind the crate's "cli" feature, so sai-rs itself does not carry them).
#
# Native checks (a cross build runs none): the crate's fmt, clippy -D warnings and tests, and the golden test
# (crates/tl-frames/tests/differential.rs against tests/golden/frames-cpp.txt, the C tools' recorded behaviour):
# every scenario runs the Rust tool over veth pairs inside an unprivileged network namespace and compares exit
# status, output and the frames on the wire with the record. It runs in the nix build sandbox (user namespaces
# work there) and never skips: where they do not, the build fails.
{
  saiRs,
  iproute2,
  util-linux,
}:
saiRs.overrideAttrs (old: {
  pname = "tl-frames";
  cargoBuildFlags = [ "-p" "tl-frames" "--features" "cli" ];
  cargoTestFlags = [ "-p" "tl-frames" "--features" "cli" ];
  # only what this crate needs: not the board tables the workspace goldens read
  env = builtins.removeAttrs old.env [ "PHY_SEQ_TABLES" ];
  nativeCheckInputs = old.nativeCheckInputs ++ [ iproute2 util-linux ];
  preCheck = ''
    cargo fmt -p tl-frames --check
    cargo clippy --offline -p tl-frames --all-targets --features cli --profile "$cargoCheckType" -- -D warnings
  '';
  postCheck = "";
  postInstall = "";
  meta = old.meta // { description = "Hardware test frame senders and counters of the CS8210 SAI tests, in Rust"; };
})

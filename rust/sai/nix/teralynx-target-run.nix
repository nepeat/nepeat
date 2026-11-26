# The Teralynx test bundle of a cross build (./libsai-teralynx.nix passthru.testBundle: the Rust
# libsai, the C shim, the mock libifcs, test_ifcs and the call-log driver, built for the target) run
# where the target executes: a derivation of the target's system, so on a build host with that
# system in extra-platforms (binfmt + qemu-user) it runs under emulation. The same comparison as the
# native check (crates/sai-backend-teralynx/tests/test-ifcs.sh, PREBUILT=1): every FAIL line of
# test_ifcs expected, and the create_switch / remove_switch IFCS calls equal to the C++ library's
# for that target (cppLibsai, optional) on the same mock. What it adds to the native run: the GOT
# redirects with the target's relocation types (R_AARCH64_JUMP_SLOT / _GLOB_DAT) on a real loader; the
# ifcs crate's ABI test (tests/abi.rs, the bundle's ifcs/: by-value byte twins on the target's calling
# convention, zero padding in the keys and attributes) with its C fixture.
#   import ./teralynx-target-run.nix { pkgs = import nixpkgs { system = "aarch64-linux"; }; bundle = ...; cppLibsai = ".../lib/libsai.so.1.0.0"; }
{
  pkgs,
  bundle,
  cppLibsai ? null,
}:
pkgs.runCommand "sai-rs-teralynx-target-run" { } ''
  cp -r ${bundle} work
  chmod -R u+w work
  mkdir -p $out
  rc=0
  PREBUILT=1 EXPECTED=$PWD/work/test_ifcs-expected.txt bash work/test-ifcs.sh $PWD/work - - - - \
    ${if cppLibsai == null then "" else cppLibsai} > $out/test_ifcs.txt 2>&1 || rc=$?
  cat $out/test_ifcs.txt
  if [ -x work/ifcs/abi ]; then
    IFCS_ABI_FIXTURE=$PWD/work/ifcs/libifcs_abi_fixture.so work/ifcs/abi > $out/ifcs-abi.txt 2>&1 || rc=1
    cat $out/ifcs-abi.txt
    grep -q '^test result: ok. 1 passed' $out/ifcs-abi.txt || rc=1
  fi
  for f in create_remove-rust.txt create_remove-rust.stderr create_remove.diff test_ifcs.stderr; do
    [ -f work/$f ] && cp work/$f $out/
  done
  uname -m > $out/machine
  exit $rc
''

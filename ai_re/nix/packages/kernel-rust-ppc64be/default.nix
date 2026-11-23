# Rust in the kernel on 64-bit big-endian PowerPC: a patch series against
# Linux 7.3.4 (upstream has Rust for ppc32 and ppc64le only) and the config a
# kernel recipe adds. Platform-generic: every ppc64 BE board's kernel recipe
# takes it (linux-qoriq-t2080: `rust = true`). README.md: what, why, proof.
#   import ./kernel-rust-ppc64be { inherit lib; }
#     .kernelPatches   [ { name, patch } ]  for linux_latest.override / boardPatches
#     .config          structuredExtraConfig fragment (RUST = yes)
#     .keepBindings    kernel -> kernel whose dev output also keeps
#                      rust/bindings_generated.rs (the layout check of ./tests.nix
#                      reads it) and rust/kernel/generated_arch_*.rs (warn_on!() and
#                      static branches in out-of-tree modules include them; nixpkgs
#                      drops them with the other build products)
#     .series          the patches with their scope
{ lib }:
let
  # Ordered; each entry's scope says who can take it:
  #   powerpc    every powerpc kernel with Rust (also ppc32 and ppc64le)
  #   ppc64      every 64-bit powerpc kernel (also little endian)
  #   ppc64-be   64-bit big-endian powerpc (Book3S and Book3E alike)
  #   rust       not powerpc-specific
  # No patch is board-only: a board's own patches stay with its kernel recipe.
  series = [
    { file = "0001-powerpc-asm-use-the-Clang-DS-form-constraint-wheneve.patch"; scope = "ppc64"; }
    { file = "0002-rust-bindgen-support-big-endian-64-bit-PowerPC.patch"; scope = "ppc64-be"; }
    { file = "0003-powerpc-rust-build-Rust-code-in-modules-with-the-lar.patch"; scope = "ppc64"; }
    { file = "0004-powerpc-bug-provide-ARCH_WARN_ASM-and-ARCH_WARN_REAC.patch"; scope = "powerpc"; }
    { file = "0005-powerpc-Enable-Rust-for-big-endian-ppc64-ELFv2.patch"; scope = "ppc64-be"; }
    { file = "0006-rust-bindings-add-linux-hwmon.h.patch"; scope = "rust"; }
  ];
  version = "7.3.4";
  onDisk = builtins.attrNames (lib.filterAttrs (n: _: lib.hasSuffix ".patch" n) (builtins.readDir ./patches));
  listed = map (p: p.file) series;
in
assert lib.assertMsg (lib.sort lib.lessThan onDisk == lib.sort lib.lessThan listed)
  "kernel-rust-ppc64be: series and patches/ differ";
{
  inherit series version;
  kernelPatches = map (p: {
    name = "rust-ppc64be-" + lib.removeSuffix ".patch" p.file;
    patch = ./patches + "/${p.file}";
  }) series;
  config = { RUST = lib.kernel.yes; };
  keepBindings = kernel:
    assert lib.assertMsg (kernel.version == version)
      "kernel-rust-ppc64be: series is for Linux ${version}, kernel is ${kernel.version}: rebase it";
    kernel.overrideAttrs (o: {
      postBuild = (o.postBuild or "") + ''
        mkdir -p "$NIX_BUILD_TOP/rust-keep"
        cp "$buildRoot/rust/bindings/bindings_generated.rs" "$NIX_BUILD_TOP/rust-keep/"
        cp "$buildRoot"/rust/kernel/generated_arch_*.rs "$NIX_BUILD_TOP/rust-keep/"
      '';
      postInstall = (o.postInstall or "") + ''
        d="$dev/lib/modules/${kernel.modDirVersion}/build/rust"
        install -Dm444 "$NIX_BUILD_TOP/rust-keep/bindings_generated.rs" "$d/bindings_generated.rs"
        install -Dm444 -t "$d/kernel" "$NIX_BUILD_TOP"/rust-keep/generated_arch_*.rs
      '';
    });
}

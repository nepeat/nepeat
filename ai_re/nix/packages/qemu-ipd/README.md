# qemu-ipd

qemu-user (x86_64 guests) patched to run Marvell's amd64 Teralynx SAI on the CS8210's arm64 CPU
against the real `ipd` driver, plus the amd64 pieces that run inside it. Why, what was checked,
and the hardware plan: [devices/cs8210-32x/sai-qemu.md](../../../devices/cs8210-32x/sai-qemu.md).

| attr | what |
|---|---|
| `qemu-ipd` | arm64 `qemu-x86_64` (cross-built) with `patches/` |
| `qemu-ipd-native` | the same patches on x86_64, for the tests |
| `runner` | arm64 `isai-qemu PROGRAM ARGS`: `-B 0 -cpu Nehalem`, `QEMU_IPD_STRICT=1`, guest `LD_PRELOAD` of devmem |
| `devmem` | amd64 `libipd-devmem.so` ([guest/devmem.c](guest/devmem.c)) |
| `ortools-stub` | amd64 `libortools.so.9` stand-in ([guest/ortools-stub.nix](guest/ortools-stub.nix)) |
| `isai`, `libsai` | [../isai-amd64](../isai-amd64/default.nix): isai 6.2.0 debs, unpacked, libsai with RUNPATHs |
| `smoke` | amd64 `isai-smoke` (../sai-teralynx/tools/sai_smoke.cpp), `isai-version`, `sai-bench` |
| `syncd` | the pinned SONiC sairedis/syncd (../sonic) linked against this libsai: **does not work** with isai (SAI 1.19.2 vs 1.16.1: `sai_attribute_t` 64 vs 48 bytes) |
| `syncd-202505` | sairedis of SONiC 202505 (SAI 1.16.1) on this libsai, doxygen 1.9.5 (`doxygen-1_9`), [capability patch](patches/sairedis-202505-capability-unknown-attr.patch) |
| `syncd-runner` | arm64 `bin/syncd` = isai-qemu + syncd-202505 (services.sonic.sai.package; [sonic-isai.nix](sonic-isai.nix)) |
| `bench-native`, `sai-teralynx` | [tools/sai_bench.cpp](tools/sai_bench.cpp) on our SAI (arm64); the amd64 build is in `smoke` |
| `guest` | the amd64 closure to copy to the box |
| `tests.{layout,passthrough,codegen,aarch64}` | offline tests, see [tests/default.nix](tests/default.nix) |

Environment of the patched qemu: `QEMU_IPD_TRACE=1` (log ipd ioctls/mmaps), `QEMU_IPD_STRICT=1`
(device-safe code generation), `QEMU_IPD_TEST_PATH` (tests only). [module-sketch.nix](module-sketch.nix)
is an unimported sketch of box integration; the experiment invokes `isai-qemu` by hand.
[sonic-isai.nix](sonic-isai.nix) (also unimported) turns nepeat/nix's cs8210 SONiC config into the
isai variant for `extendModules`.

```sh
# from ai_re/ (not in packages/default.nix yet)
nix build --impure --max-jobs 4 --expr 'let f = builtins.getFlake "path:./nix";
  pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; config.allowUnfree = true; };
  in (import ./nix/packages/qemu-ipd { inherit pkgs; }).tests.passthrough'
```

The vendor files are fetched from github.com/Marvell-switching/sonic-marvell-teralynx-binaries at a
pinned commit; they live in the Nix store only (unfree: `allowUnfree`).

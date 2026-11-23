# kernel-rust-ppc64be: Rust kernel modules on 64-bit big-endian PowerPC

State of 2026-11-23: Linux 7.3.4, nixpkgs rustc 1.99.1 (LLVM 23.1.3), bindgen
0.72.1 (libclang 21), GCC 16.2.0, binutils 2.46. Upstream 7.3 has Rust for ppc32
and ppc64le only (`arch/powerpc/Kconfig:288-289`). This directory carries the
big-endian enablement ahead of upstream, written to be sendable, plus its proof.
Platform-generic: any ppc64 BE board's kernel recipe takes it; the T2080 one does
with `import ../linux-qoriq-t2080 { rust = true; }`, which the T2080 boards use by
default since 2026-11-23 (`hardware.qoriqT2080.rust`).

## the series (`patches/`, scopes in `default.nix`)

| # | scope | what, why |
|---|---|---|
| 0001 | ppc64 | `DS_FORM_CONSTRAINT` keyed on `__clang__`: bindgen (libclang) parses a GCC build's headers and rejects GCC's `"YZ<>"` |
| 0002 | ppc64-be | bindgen target `powerpc64-linux-gnu` on BE (was hard-coded LE: every endian-dependent layout would be wrong), and libclang gets rustc's CPU instead of GCC's `-mcpu` (e6500 unknown to clang) |
| 0003 | ppc64 | `-Ccode-model=large` for Rust in modules, as `-mcmodel=large` for C (below) |
| 0004 | powerpc | `ARCH_WARN_ASM`/`ARCH_WARN_REACHABLE` for Rust's `warn_on!`: without them **the kernel crate does not build on any powerpc with CONFIG_BUG=y** (also ppc64le, ppc32) |
| 0005 | ppc64-be | `HAVE_RUST if PPC64 && CPU_BIG_ENDIAN && PPC64_ELF_ABI_V2 && !PPC_KERNEL_PCREL`; rustc target `powerpc64-unknown-linux-gnuelfv2`; `-Ctarget-cpu` from the CPU type; arch-support.rst |
| 0006 | rust | `linux/hwmon.h` in the bindings (for the Rust as7760-fan) |

Each patch has an upstream-style message with the reasoning and a
`Signed-off-by: OWNER NAME` placeholder.

## analysis (what the patches rest on)

- **ABI of the kernel today:** ELFv2. `linux-qoriq-t2080` sets
  `PPC64_BIG_ENDIAN_ELF_ABI_V2` (default y with GCC that has `-mabi=elfv2`,
  `arch/powerpc/Kconfig:648`); built .config: `CONFIG_PPC64_ELF_ABI_V2=y`, C
  flags `-mabi=elfv2 -mcmodel=medium -msoft-float -mno-altivec -mno-vsx -mcpu=e6500`.
  No ABI switch was needed.
- **rustc target:** `powerpc64-unknown-linux-gnuelfv2` (tier 3) is the LE target
  with the endianness flipped: `llvm-abiname elfv2`, data layout
  `E-m:e-Fn32-i64:64-i128:128-n32:64-S128` (i128 16-aligned like GCC's
  `__int128`), 64-bit atomics (`rustc --print target-spec-json`).
  `powerpc64-unknown-linux-gnu` is ELFv1 and is not used. A custom JSON target with
  `"rustc-abi": "softfloat"` is rejected (`rustc_abi is unused on PowerPC64`), so the
  built-in target + `-Ctarget-feature=-hard-float,...` (as upstream LE) is the only
  route; rustc warns that disabling `hard-float` "will become a hard error" (rust
  issue 116344): a future rustc may refuse to build, loudly.
- **flags:** float ABI soft (GPRs, as `-msoft-float`), no AltiVec/VSX/MMA;
  relocation model static (kernel), but rustc still makes external calls
  `bl sym; nop` (TOC restore slot the module loader patches) and reaches data
  through the TOC; frame/red zone are ELFv2's (288-byte red zone, the kernel
  reserves `KERNEL_REDZONE_SIZE` 288 on interrupt entry, `asm/ptrace.h:119`);
  r2 TOC, r13 never allocated. CPU: LLVM names (`pwr4..pwr9`, `970`,
  `e5500`); no e6500 model, e5500 is its integer subset; POWER10 capped at pwr9
  (pwr10 = prefixed + PC-relative code the TOC kernel cannot take).
- **code model (patch 0003):** with rustc's default (medium) a Rust module had
  184 `R_PPC64_TOC16_HA/LO` straight against `.data.rel.ro`, 5 against `.bss` and
  REL16 global entry points; `module_64.c` does not range-check
  `TOC16_HA/TOC16_LO`, so a module whose data lands >2 GB from its TOC would be
  relocated silently wrong. With large: every access via a `.toc` entry
  (ADDR64), entries via REL64. Measured on `abi-test` objects.
- **ELFv1** (function descriptors, `.opd`): not enabled; Kconfig excludes it, the
  commit says so. It needs the ELFv1 target, ELFv1 asm in the ABI test and a
  Book3S ELFv1 boot test; future work.
- **upstream:** "Rust support for powerpc" by Mukesh Kumar Chaurasiya (IBM),
  ppc32 by Link Mauve, V2..V17 2026-02..07 (lore mirrors, e.g.
  https://ratatoskr.run/linuxppc-dev/2026/02/3402220/t). BE ppc64 was left out
  with "ppc64be still has some missing things from the toolchain side" (V2
  reply); reviewers objected to the doc wording ("Experimental" is not a support
  level) and to the target-feature warnings ("warnings are never expected").

## proof (`tests.nix`, all in `just test` as `extra:kernel-rust-ppc64be.*`)

`just test-packages -k kernel-rust-ppc64be` (about 25 min cold on 32 cores: four kernels, three QEMU runs under TCG).

| output | what | result 2026-11-23 |
|---|---|---|
| `e6500`, `e6500-kunit`, `pseries` | kernels (rustc 1.99.1, bindgen 0.72.1, GCC 16.2.0): T2080 recipe + QEMU ppce500 platform + test drivers as modules (hardware candidate); + KUnit doctests + Rust debug assertions; Book3S POWER8 pseries BE | build |
| `abi-*` | `abi-test/`: Rust<->C both directions: 16 scalars incl. stack spill, u128 pairs, 24 struct types (1..48 bytes, packed, align(16), u128, bool, pointer, union) by value at three placements (registers, straddling r10/stack, stack) and returned, function pointers both ways, C varargs, layouts of the test types; **register captures** of rustc's and GCC's calls (narrow-int extension, u128 pairing, stack slots) and of narrow return values; atomics u8/u16 (sub-word, canaries), u32/u64, CAS, kernel `Atomic<i64>`, SpinLock and Mutex hammered by a kthread per CPU; KBox/KVec/VVec incl. 16-byte alignment; `pr_info!` formatting; `warn_on!` (patch 0004) | 338/338 |
| `layout-*` | `layout-check/`: every named struct of `bindings_generated.rs`: rustc's size/align/offsets == GCC's, compiled with the kernel's flags | e6500: 2416 structs, 14079 fields, 18911 values, 1 known mismatch (below) |
| `qemu-e6500` | ppce500, e6500, 4 vCPUs (MTTCG): ABI module load/unload twice, all 15 Rust samples load/unload, Rust as7760-fan vs C on i2c-stub (54 observations equal, twice) | pass |
| `qemu-e6500-kunit` | + KUnit: 365/365 kernel-crate doctests | pass |
| `qemu-pseries` | QEMU pseries, POWER8, BE ELFv2, 4 vCPUs: ABI module twice + 3 samples | pass (338/338) |
| `c-neutral` | the series with RUST off: T2080 vmlinux section by section equal to the kernel without it (only `.notes` build id differs; `.rodata` with store hashes masked), `.config` differs only by `CONFIG_HAVE_RUST=y` | pass |

The console checks (`check-console.sh`): no Oops/BUG/relocation/TOC errors, the
only WARNINGs are the module's own two at the right `file:line`, taint only O/W/N.

**Found by the layout check:** `struct mce_error_info` (`asm/mce.h`): bindgen
0.72.1 lays the bit-fields of its anonymous union out one after another, so
Rust sees size 20 where GCC and libclang say 16. Architecture-independent
(reproduced for x86_64 and ppc64le); bindgen's own layout tests would catch it
but the kernel generates with `--no-layout-tests`; nothing in `rust/` uses the
type. Listed in `KNOWN` (the check fails if it changes or disappears).

**Other findings:** nixpkgs' kernel dev output drops
`rust/kernel/generated_arch_*.rs`, so an out-of-tree module using `warn_on!`
does not build; `keepBindings` keeps them (also `bindings_generated.rs`).
GCC passes `__int128` in the next two GPRs without even-register alignment;
rustc does the same (captured, equal).

## as7760-fan in Rust (`../as7760-fan/rust`, `rust.nix`)

Same module/driver/hwmon name `as7760_fan`, OF id `accton,as7760-fan`, same
`pwm1`, `fan1..12_input/_fault/_label` (modes 0644/0444), values, errnos and
register sequence (0x33=0, 0x11=duty, read-back, full-speed fallback) as
the C driver it replaced (deleted 2026-11-23), so `fand` and `sonic_platform`
see no difference. Register logic in `logic.rs` (no kernel types, 3 host tests run in
the build, the write rounding checked over -1000..1000). The QEMU test compares
it with `../as7760-fan/qemu-expected.txt`, the C driver's behaviour in the same
scenario on i2c-stub (recorded from both drivers' identical runs, 47c0012).

7.3's kernel crate has I2C drivers, OF tables, Mutex and ARef, but no hwmon and
no SMBus API: hwmon types come from the bindings (patch 0006), registration is
the small `hwmon` module in `main.rs` (non-devm, unregistered in Drop before the
`Chip` its callbacks use is freed: field order), SMBus byte read/write are raw
calls. `i2c_check_functionality()` is a static inline without a Rust helper: kept
in C (`shim.c`, 1 function).

| file | lines | unsafe |
|---|---|---|
| `logic.rs` | 93 | none |
| `main.rs` | 310 | 11 blocks: SMBus read, SMBus write, functionality check, hwmon register (call site + call), unregister, drvdata to `&Chip`, 2 out-pointer writes, 2 callback `chip()` uses; 2 `unsafe fn` with Safety contracts (`Registration::new`, `chip`); 4 `unsafe extern "C"` hwmon callbacks; 3 `unsafe impl` (`Sync` for the static C tables, `Send`/`Sync` for the registration handle); 1 `extern` block (the C shim) |
| `shim.c` | 13 | C |

Not covered by the QEMU parity test: the verify-mismatch and SMBus-error paths
of a duty write (i2c-stub always stores what is written); they mirror the C
code line by line but have run in neither driver under test.

## on a board

NixOS: `hardware.qoriqT2080.rust` (platforms/qoriq-t2080.nix, **default true
since 2026-11-23**) builds the board kernel with the series; the fan driver
(thermal/as7760-fan.nix) is the Rust one and needs it. Against the board kernel
the config changes only by the `RUST*`/`RUSTC_*`/`BINDGEN_*` symbols; every
out-of-tree module (BDE/KNET, the fan driver) is rebuilt for it by NixOS. There
is no `MODVERSIONS` or `MODULE_SIG`: with and without the series the vermagic
is `7.3.4 SMP preempt mod_unload`, so a module built for the other kernel would
load without complaint. Load only modules built for the running kernel
(`tests.nix` `abiTestFor`/`layoutCheckFor` take a board kernel).

## proof on hardware (AS7772, 2026-11-23)

Evidence: [devices/edge-core-as7772-32x/evidence/rust-kernel/](../../../devices/edge-core-as7772-32x/evidence/rust-kernel/).
Base `631c5hx7` (47c0012, attr as7772, C fan driver) -> trial `na3yx1c2`
(same + `hardware.qoriqT2080.rust` + the Rust fan driver; closure diff: kernel,
its modules, BDE/KNET rebuilt, initrd/uImage, fan driver). Two warm boots on the
trial, console logged. Green:
- kernel with `CONFIG_RUST=y`, the Rust `as7760_fan` bound (4-0066), sysfs
  (device, every attribute, mode, label, fault) identical to the C driver's;
  pwm1 writes on the real CPLD: 200->204, 128/129->136, 254/255->255, 100/127/256/-1/abc
  EINVAL (= the C driver in QEMU); fand ran its curve throughout, never failsafe
- ABI module on the real e6500, 8 CPUs: 338/338, twice; kernel log: only the
  module's two deliberate warn_on; otherwise the base's boot-time iommu/PCIe
  warnings and nothing else
- MKA up on 4 ports by itself (14.8-15.3 s), sonic-traffic-test and
  macsec-traffic-test 0 lost, encrypted = accepted, 32/32 PHYs
- 2 h 3 min line-rate soak through SONiC with MACsec on (`loadtest/sonic-ring.sh`):
  1.30 x 10^11 frames per port per direction, 17.7 Mpps / 92.1 Gb/s
  (94.9 Gb/s with preamble and IPG) per port, 0 discards and 0 errors until the
  stop (then 740 frames dropped at ce0 and the other ring's copied to the CPU once, as designed), MACsec IN_PKTS_INVALID/NOT_VALID/LATE 0, 8 SAs open
  throughout, ASIC 45-48 C, fans 13,000-17,100 rpm on fand's curve
- left running the trial (profile generation 31); the base is generation 30 in
  the extlinux menu ("Enter choice:", 3 s)

Not proven on the AS7760 (same platform, same fan CPLD; powered off).

## what the tests cannot catch

Paths not executed (QEMU only runs what the tests call; TCG checks opcodes per
CPU model but not timing or cache/MMU behaviour of real e6500 SMT); struct
layouts of types bindgen never emits (macros, inline-only); ABI classes the
kernel's Rust never uses (floats, vectors); GCC-vs-rustc disagreements inside
`asm!` blocks; anything specific to ELFv1, LE or PCREL kernels.

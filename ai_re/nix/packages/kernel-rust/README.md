# kernel-rust: Rust in the kernels built here

State of 2026-11-23, Linux 7.3.4, nixpkgs rustc 1.99.1 / bindgen 0.72.1.
Paths without a prefix are in the kernel source.

## which boards can

| platform | CONFIG_RUST | why |
| --- | --- | --- |
| Alpine v2/v3 (DMC-12, CS8210), arm64 | yes | `arch/arm64/Kconfig:228` (`HAVE_RUST if RUSTC_SUPPORTS_ARM64`, little endian). nixpkgs turns it on for aarch64 from 6.12 (`common-config.nix`, `rustByDefault`), so linux-alpine's `trimmed`/`lean` variants have it already; `minimal` (allnoconfig, the DMC-12's default) gets it with `rust = true` / `hardware.alpineV2.kernelRust` (+0.98 MB Image) |
| Armada 7040 (AS5114), arm64 | yes | stock nixpkgs kernel: on already |
| QorIQ T2080 (AS7760, AS7772), ppc64 big endian | **no** | `arch/powerpc/Kconfig:288-289` selects `HAVE_RUST` only for `PPC32` and `PPC64 && CPU_LITTLE_ENDIAN`; `arch/powerpc/Makefile:65` and `rust/Makefile` (bindgen target) hard-code powerpc64le; powerpc is not in `Documentation/rust/arch-support.rst`. Also: rustc's `powerpc64-unknown-linux-gnu` is ELFv1, our kernel is ELFv2 (`powerpc64-unknown-linux-gnuelfv2` exists, tier 3), and bindgen's clang does not know `-mcpu=e6500`. An experimental enablement patch is separate work: `../linux-qoriq-t2080/experimental` (opt-in, never a board's default kernel) |

Out-of-tree Rust modules build with `make -C <kernel.dev>/build M=...` like C
ones; a Rust-enabled kernel's `moduleBuildDependencies` carry rustc and bindgen,
its `dev` output the `rust/*.rmeta` and proc macros. The allnoconfig recipe
composes: `RUST = yes` is the only addition.

## what the kernel crate covers (for our drivers)

| need | 7.3 | notes |
| --- | --- | --- |
| I2C driver, adapter by number | yes | `rust/kernel/i2c.rs`; needs `I2C=y` |
| SMBus / i2c_transfer | no | raw `bindings::i2c_smbus_xfer` (one unsafe call) |
| regmap, hwmon | no | not even in the bindings (`rust/bindings/bindings_helper.h` lacks the headers): a Rust hwmon driver would be hand-declared FFI throughout |
| reboot notifier / restart / sys-off handler | no | `linux/reboot.h` is bound only when the config happens to pull it in (not in our minimal kernel) |
| platform + OF, fwnode properties, devres, IRQ, PCI (BARs, MSI-X), MMIO + `register!` | yes | MMIO accessors are little endian only |
| coherent DMA, sg tables | yes | no `dma_map_single/page` wrapper |
| PHY drivers (phylib) | yes | `rust/kernel/net/phy.rs`, `RUST_PHYLIB_ABSTRACTIONS` |
| net_device, NAPI, sk_buff, ethtool ops, MDIO bus, phylink | no | `netdevice.h`/`etherdevice.h`/`phylink.h` are not in the bindings: an out-of-tree Rust NIC driver needs a C shim for every one of these calls |
| workqueue, hrtimer, mutex/spinlock, module parameters (integers, no sysfs file) | yes | no work cancel/flush, no `timer_list` |

## what is here

| output (`tests/nixtargets.py`: `extra:kernel-rust.*`, `extra:al-eth-rs`) | what |
| --- | --- |
| `kernel` | linux-alpine `minimal` + `rust = true` + QEMU virt UART, i2c-stub, trace events, Rust samples |
| `dmc12-cpld-reset-rs` | [../dmc12-cpld-reset/rust](../dmc12-cpld-reset/rust): the DMC-12 reboot module in Rust, same module name and `bus` parameter as the C one (which stays the default). NixOS: `hardware.dmc12.cpld.rebootDriver = "rust"`; whole system: `dmc12-rust-system` (git flake). **Not run on hardware yet** |
| `dmc12-cpld-reset-qemu-test` | boots that kernel in QEMU (TCG), loads the kernel's `rust_minimal`, then the module (load, unload, load) and reboots in four scenarios (device answers, no device, no bus, power-off). The module's messages and the SMBus writes seen on the bus (`smbus_write` trace events) must equal [qemu-expected](qemu-expected) for the C and the Rust module |
| `al-eth-rs` ([../al-eth/rust.nix](../al-eth/rust.nix)) | `board_params.rs`: al_eth's board-parameter registers, safe Rust, no_std; equal to the C HAL functions on 400k generated vectors. Not linked into al_eth |

```sh
just test-packages -k "kernel-rust or al-eth-rs"
```

## unsafe inventory

| file | lines | unsafe |
| --- | --- | --- |
| `dmc12-cpld-reset/rust/sequence.rs` (logic, host-tested) | 90 | none |
| `dmc12-cpld-reset/rust/main.rs` | 110 | 1 block: `i2c_smbus_xfer` (no SMBus API in the kernel crate) |
| `dmc12-cpld-reset/rust/reboot.rs` (notifier registration behind a safe interface) | 117 | 4 blocks (init the notifier_block, register, callback's container_of, unregister), the `unsafe extern "C"` callback, 2 `unsafe impl` (Send/Sync), and an `extern "C"` block declaring 2 functions by hand |
| `dmc12-cpld-reset/rust/abi_check.c` | 16 | C, no code: static asserts that those hand declarations match `linux/reboot.h` |
| `al-eth/rust/board_params.rs` | 459 | none (`forbid(unsafe_code)` build) |

Plainly: the reboot module is 83 lines of C against about 230 lines of Rust with
5 unsafe blocks, because none of the three kernel interfaces it uses (reboot
notifier, SMBus transfer, busy-wait) has a Rust API yet. It proves the pipeline
(kernel option, out-of-tree build, host tests, QEMU) and its `reboot.rs` is
reusable; it is not a reduction in memory-unsafe code. `board_params.rs` is.

## rules for a Rust module here

- Logic in files that use only `core`, unit-tested on the build host
  (`rustc --test`; kbuild has no test target for external modules).
- Kernel interfaces without a Rust API: one small module with a safe interface
  (as `reboot.rs`), every `unsafe` with a SAFETY comment, the count in the table
  above. C functions missing from the bindings: declare by hand **and** add a
  static assert in a C file of the same module (`abi_check.c`).
- Build with `-Dwarnings`; same module name and parameters as the C twin so the
  NixOS side does not change; C stays the default until the Rust one has run on
  the hardware.

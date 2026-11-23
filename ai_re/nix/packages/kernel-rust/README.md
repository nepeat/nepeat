# kernel-rust: Rust in the kernels built here

State of 2026-11-23, Linux 7.3.4, nixpkgs rustc 1.99.1 / bindgen 0.72.1.
Paths without a prefix are in the kernel source.

## which boards can

| platform | CONFIG_RUST | why |
| --- | --- | --- |
| Alpine v2/v3 (DMC-12, CS8210), arm64 | yes | `arch/arm64/Kconfig:228` (`HAVE_RUST if RUSTC_SUPPORTS_ARM64`, little endian). nixpkgs turns it on for aarch64 from 6.12 (`common-config.nix`, `rustByDefault`), so linux-alpine's `trimmed`/`lean` variants have it already; `minimal` (allnoconfig, the DMC-12's default) gets it with `rust = true` / `hardware.alpineV2.kernelRust` (+0.98 MB Image) |
| Armada 7040 (AS5114), arm64 | yes | stock nixpkgs kernel: on already |
| QorIQ T2080 (AS7760, AS7772), ppc64 big endian | yes (our series) | upstream 7.3 selects `HAVE_RUST` only for ppc32 and ppc64le (`arch/powerpc/Kconfig:288-289`). Our six-patch series for 64-bit big-endian ELFv2 (Book3E and Book3S) is in [../kernel-rust-ppc64be](../kernel-rust-ppc64be/README.md); the boards' kernel has it on (`hardware.qoriqT2080.rust`, default since 2026-11-23, proven on the AS7772); the fan driver is Rust |

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
| net_device, NAPI, sk_buff, ethtool ops, streaming DMA | no; **yes with `rustNet`** | not in the stock bindings. [0001-rust-net-bindings-and-helpers.patch](0001-rust-net-bindings-and-helpers.patch) (linux-alpine `rustNet = true`, minimal + rust only) adds netdevice.h/etherdevice.h/skbuff.h/if_vlan.h to `bindings_helper.h` and 58 one-line C helpers (static inlines and macros, 5 sk_buff bitfield/anonymous-union field accessors, dma_map_single & co.); the safe layer on top is [../al-eth/rust/knet](../al-eth/rust/knet) |
| MDIO bus, phylink | no | not needed on the DMC-12; CS8210 (SGMII + phylib) later: own the bus in a type, borrow the PHY from it |
| workqueue, hrtimer, mutex/spinlock, module parameters (integers, no sysfs file) | yes | no work cancel/flush, no `timer_list` |

## what is here

| output (`tests/nixtargets.py`: `extra:kernel-rust.*`, `extra:al-eth-rs`) | what |
| --- | --- |
| `kernel` | linux-alpine `minimal` + `rust = true` + QEMU virt UART, i2c-stub, trace events, Rust samples |
| `dmc12-cpld-reset-rs` | [../dmc12-cpld-reset/rust](../dmc12-cpld-reset/rust): the DMC-12 reboot module, used by `boards/dmc12/cpld` (which turns on `hardware.alpineV2.kernelRust`). Reset u01 through the CPLD in a trial boot on 2026-11-23 (`devices/dmc-12-48r-as0/trial-boot-20261123.txt`); the C module it replaced is deleted |
| `dmc12-cpld-reset-qemu-test` | boots that kernel in QEMU (TCG), loads the kernel's `rust_minimal`, then the module (load, unload, load) and reboots in four scenarios (device answers, no device, no bus, power-off). The module's messages and the SMBus writes seen on the bus (`smbus_write` trace events) must equal [qemu-expected](qemu-expected), recorded from the deleted C module |
| `net-kernel` | `kernel` + `rustNet` (net bindings patch) + `debug` (KFENCE, SLUB debug, lockdep, DEBUG_ATOMIC_SLEEP, DEBUG_OBJECTS, DMA_API_DEBUG, kmemleak, DEBUG_NET + netdev refcount tracker, Rust debug assertions; linux-alpine `debugConfig`) |
| `fake-nic`, `pktloop` | [../al-eth/rust/qemu](../al-eth/rust/qemu): a loopback NIC on QEMU's `edu` PCI device built on knet only (real MSI, real device DMA through edu's engine), and an AF_PACKET frame generator/checker |
| `fake-nic-qemu-test` | boots `net-kernel` (slub_debug=FZPU, kfence.sample_interval=5): traffic at MTU 1500 and 3800 (copy, build_skb and multi-buffer RX; TX head + page fragments), MTU/MAC/rx-mode ops, 200 up/down, 100 unload/reload, down and unload with DMA in flight; steps must equal [expected.txt](../al-eth/rust/qemu/expected.txt) and the log must hold no checker report, WARN or BUG; kmemleak 0, DMA-API errors 0, no W taint (2026-11-23: pass, ~100 s) |
| `fake-nic-qemu-detects-leak` | test of the test: `inject=1` leaks every TX packet with its mappings (safe code may leak); passes only if kmemleak *and* DMA-API debug report it (2026-11-23: kmemleak 6, DMA-API 3) |
| `al-eth-rs` ([../al-eth/rust.nix](../al-eth/rust.nix)) | `board_params.rs`: al_eth's board-parameter registers, safe Rust, no_std; equal to the C HAL functions on 400k generated vectors. Not linked into al_eth |

```sh
just test-packages -k "kernel-rust or al-eth-rs"
```

KASAN: Kconfig has `RUST depends on !KASAN || CC_IS_CLANG` (init/Kconfig:2268) and these kernels
are GCC builds, hence KFENCE + SLUB debug. A clang build (`stdenv = clangStdenv`, KASAN_GENERIC) of
`net-kernel` configures and compiles, but the nixpkgs kernel packaging then fails on an out <-> dev
reference cycle (2026-11-23, not pursued).

## unsafe inventory

| file | lines | unsafe |
| --- | --- | --- |
| `dmc12-cpld-reset/rust/sequence.rs` (logic, host-tested) | 90 | none |
| `dmc12-cpld-reset/rust/main.rs` | 110 | 1 block: `i2c_smbus_xfer` (no SMBus API in the kernel crate) |
| `dmc12-cpld-reset/rust/reboot.rs` (notifier registration behind a safe interface) | 117 | 4 blocks (init the notifier_block, register, callback's container_of, unregister), the `unsafe extern "C"` callback, 2 `unsafe impl` (Send/Sync), and an `extern "C"` block declaring 2 functions by hand |
| `dmc12-cpld-reset/rust/abi_check.c` | 16 | C, no code: static asserts that those hand declarations match `linux/reboot.h` |
| `al-eth/rust/board_params.rs` | 459 | none (`forbid(unsafe_code)` build) |
| `al-eth/rust/knet/netdev.rs` (net_device, ops vtable, TX queues + lock guard, TxLocked) | 758 | 64 blocks (one FFI call or field access each), 6 `unsafe impl` (Send/Sync of NetDev, TxLocked, Registration), 1 `unsafe fn` (from_raw), 9 `extern "C"` callbacks |
| `al-eth/rust/knet/napi.rs` (NAPI owning its ring state, NapiOff guard, NAPI+IRQ pair, GRO frags skb guard, Bh token) | 421 | 27 blocks, 4 `unsafe impl`, 2 `unsafe fn` (Napi::new, NapiIrq::new: must sit in the netdev's private data, never forgotten), 1 callback |
| `al-eth/rust/knet/skb.rs` (owned sk_buff) | 278 | 25 blocks, 1 `unsafe impl` (Send), 1 `unsafe fn` (from_raw) |
| `al-eth/rust/knet/dma.rs` (RX page frags, mapped RX/TX buffers, sync window, descriptor ring) | 484 | 23 blocks, 1 `unsafe impl`; the descriptor ring has none (kernel `io_read!/io_write!` on `Coherent`) |
| `al-eth/rust/knet/ethtool.rs` (drvinfo, link, coalesce, rings, -S), `ether.rs`, `mod.rs` | 304 | 16 blocks, 9 callbacks; ether.rs none |
| `al-eth/rust/al_eth.rs` (the driver glue) | 1469 | 15 blocks: DMA mask, the IRQ-vector self-borrow, 8 NapiIrq::new + 1 irq::Registration::new, pci_restore_msi_state after the FLR, get_random_u32, 2 for IRQ affinity |
| `al-eth/rust/qemu/fake_nic.rs` (test driver) | 639 | 3 blocks: DMA mask (kernel `unsafe fn`), the IRQ-vector self-borrow in probe, NapiIrq::new |
| `0001-rust-net-bindings-and-helpers.patch` | 422 | C: 58 one-line helpers, no logic |

knet in numbers (2026-11-23): ~2250 lines, 155 unsafe blocks, 12 `unsafe impl`, 4 `unsafe fn`, 19 C callbacks.
Nearly every block is a single FFI call with its precondition stated; the logic on top
(rings, state machines, the driver) is safe. That is more `unsafe` keywords than a
small C shim would show, and far less memory-unsafe code than the C glue it replaces
(al_eth_main.c, 5841 lines, all of it unchecked). Upstream net_device/NAPI/skb
abstractions in the kernel crate would move most of these blocks out of the driver.

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

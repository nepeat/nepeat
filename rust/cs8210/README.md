# rust/cs8210

The CS8210-32X platform tools, in safe Rust over a six-block OS rim:

| crate | what |
|---|---|
| `cs8210-sys` | the only unsafe code: a mapped PCI BAR (`mmio`, 32-bit volatile accesses) and i2c-dev SMBus byte transfers (`i2cdev`) |
| `cs8210-xcvr` | pluggable module memory maps as pure functions: CMIS 4.0/5.x and SFF-8636 identity and monitors, optoe flat offsets, the CMIS bring-up state machine (`bringup`), SFF-8024 code tables, a module simulator |
| `cs8210-optics` | `cs8210-optics`: the one owner of the I2C FPGA (PCI `1d0f:8571`) - daemon + CLI |
| `cs8210-leds` | `cs8210-leds`: LED CPLD test colours / stream mode, the system LED keeper |
| `cs8210-psu` | `cs8210-psu`: PSU status (system CPLD), PMBus identity, binds the kernel `pmbus` driver, `switch_psu_*` metrics |

Its own workspace and lock file (like `../consoled`, `../sai`), so nothing here rebuilds fand or
changes another board's closure. Nix: `nix/package.nix` (one package, every binary), used by
`nix/nixos/module/cs8210/optics` (cross-built x86_64 -> aarch64). Device notes:
`ai_re/devices/cs8210-32x/sonic.md` ("Optics", "LEDs").

## cs8210-optics

Layers: `regs` (the FPGA register map as types; the allow-list = exactly the accesses the
vendor driver makes, an unacknowledged MMIO read on this SoC being an SError) -> `bar` (the real
mapping or `sim`) -> `fpga` (sideband, ResetL/LPMode, channel probe, transfers, all-ones and
version-change stall detection) -> `cage` (per-cage state, page-select cache, optoe flat access)
-> `daemon` (poll loop, policy, socket, metrics). `pci` and `owner`: the root-port check (link,
DevSta, AER CmpltTO; the root port is on the SoC and never stalls), the dead marker
`/run/cs8210-qsfp.dead` (every process stops until a reboot) and the flock
`/run/lock/cs8210-qsfp.lock`, held for the daemon's life.

Policy: a declared cage (`--cage N[:PxG]`) with a module is probed (400 kHz), released from ResetL
and brought up: SFF-8636 high-power class enable; CMIS LowPwrRequestSW cleared, ModuleReady, then
on a paged module the application whose host lane count and rate match the cage's ports
(Deinit -> staged ApSel -> Apply_DataPathInit -> ConfigSuccess -> Deinit cleared + Tx on ->
DataPathActivated). A module already up (daemon restart) is adopted without a write. Every other
cage is held in reset. Socket protocol: one JSON object per line, `{"op": "sideband" | "status" |
"read" | "write" | "lpmode" | "reset" | "retry", ...}` (`daemon.rs` `Request`).
`--simulate DIR` runs it on the simulator with the fake-board files of
`ai_re/nix/modules/boards/cs8210/fake_board.py`.

## Build and test

```sh
cd rust/cs8210
P=$(nix eval --impure --raw --expr '(builtins.getFlake "path:'$PWD'/../../ai_re/nix").inputs.nixpkgs.outPath')
nix shell path:$P#cargo path:$P#rustc path:$P#clippy path:$P#rustfmt -c sh -c \
  'cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check'
```

Lints: clippy `all` + `pedantic`, `unwrap`/`expect`/`panic`/indexing warned outside tests
(`clippy.toml`), `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` denied.

## Unsafe

`#![forbid(unsafe_code)]` in every crate but `cs8210-sys` (`deny` at its root, `allow` in its
two modules). 6 blocks, one operation each, each with a `// SAFETY:` contract; no `static mut`,
no `unsafe impl` (counted 2026-11-25: `grep -rn "unsafe {" crates/*/src`):

| file | blocks | what |
|---|---|---|
| `cs8210-sys/src/mmio.rs` | 4 | `mmap` (shared, kernel-chosen address), `read_volatile` / `write_volatile` of an aligned, bounds-checked `u32`, `munmap` in Drop |
| `cs8210-sys/src/i2cdev.rs` | 2 | `ioctl(I2C_SLAVE)`, `ioctl(I2C_SMBUS)` with the kernel's `#[repr(C)]` layout |

## Code tables

`cs8210-xcvr/src/codes_gen.rs` is generated from sonic-platform-common's
`sonic_xcvr/codes/public/sff8024.py` (the tables SONiC's xcvrd decodes with), by
`ai_re/devices/cs8210-32x/scripts/gen_sff8024_codes.py <sff8024.py> > codes_gen.rs`.

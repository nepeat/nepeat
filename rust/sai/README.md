# sai-rs: the SAI and its switch tools in Rust

Rust ports of parts of the C++ `ai_re/nix/packages/sai-openbcm` (OCP SAI 1.19.2, the headers
sonic-sairedis is pinned to), state after "Rust migration phase 2" (2026-11-22/23). The C++
library stays the SAI the switches run; every Rust part is off by default.

Status, stated plainly:

- **On hardware (AS7772, G0, 2026-11-23):** the Rust external-PHY engine and MACsec model, inside the
  C++ libsai, programmed the MACsec PHYs under SONiC (bus traces equal to the C++; traffic, rekey, wrong
  CAK, reboot, cold boot pass), and the Rust MDIO tool read the PHYs through syncd's diag socket with the
  C tool's output. **No Rust SAI frontend or backend has driven the ASIC**, and the Rust MDIO tool has not
  done the boot-time PHY bring-up. The box runs the C++ library.
- The userspace test bundle on the AS7772 (big-endian ppc64, [Big-endian runs](#big-endian-runs)): 55 of
  55 suites, 291 Rust tests, C harness 371 checks on the release libsai (2026-11-23, G0).
- What that run found: no endianness defect. Every failure on the box was environmental
  (virtual / fake backends refusing next to `/dev/linux-*-bde`, build-tree temp paths, C
  compiled at test time) and is fixed.
- Everything else ran on x86_64 (cargo, Nix checks), in NixOS VMs ([Parity](#parity)) and
  partly on ppc64 under qemu-user.

## Migration status

"Bundle" = the AS7772 userspace run above. "On chip": see the status above (G0).

| component | C/C++ today (default on the switch) | Rust (crate) | parity evidence, where tested | on chip | selected by |
|---|---|---|---|---|---|
| SAI frontend | `sai_*.cpp` | `sai-ffi` + `sai-core` (+ `sai-meta`, `sai-sys`); typed code: SWITCH, PORT, VLAN, VLAN_MEMBER, BRIDGE, BRIDGE_PORT | C harness 371 checks; syncd + `sai_smoke.py` unchanged; SONiC VM + L3 pass, ACL fails ([Parity](#parity)); x86, VM, bundle | no | the `libsai.so` syncd links (`nix/package.nix`); no NixOS option |
| ... attribute bags | | `sai-core::bag`: 23 types (STP, VR, RIF, NH, NHG + member, hostif + trap group / trap / user trap / table entry, queue, PG, scheduler group, scheduler, policer, buffer pool / profile, QoS map, WRED, hash, LAG + member); FDB / neighbor / route entries | stored and checked by metadata; no C++ behaviour behind them | no | |
| ... missing | | ACL, mirror, samplepacket, MACsec objects in `libsai.so`, STP ports, bulk, dump, FDB events, warm boot | | | |
| virtual backend + netdevs | `fake_backend.cpp` | `sai-core::virt` + `tap-netdev` | SONiC VM tests; `vm-sai-rs-privileged` | never (refuses next to a chip) | `SAI_RS_BACKEND=virtual`, `SAI_RS_VIRTUAL_NETDEV=tap` |
| hardware backend | `openbcm_backend.cpp` | `sai-backend-openbcm` over `sdk-glue` / `sdk-glue-sys`: ports, VLANs, link events only | fake glue: 35 tests, smoke 99 checks (x86, qemu-ppc64); real glue: links against libsdk, never executed | no | cargo feature `openbcm-real` / `-fake` (`nix/libsai-openbcm.nix`) + `SAI_RS_BACKEND=openbcm` |
| SDK glue | `sdk_glue.c` | stays C; bound by `sdk-glue-sys`, wrapped by `sdk-glue` | declarations checked against `sdk_glue.h` by the C compiler (x86, ppc64) | the C file, inside the C++ library; the Rust bindings no | features `real` / `fake` |
| MDIO script interpreter | `mdio_script.c` | `mdio-script` | differential, 7253 comparisons; bundle | no | inside `mdio-tool` |
| MDIO report + boot-time tool | `mdio_report.c`, `mdio_script_main.c` (`openbcm-mdio-script`) | `mdio-report`, `mdio-tool` (`openbcm-mdio-script-rs`) | 159 differential runs vs the C tool; `vm-mdio-units-rs`; bundle (fake chip) | no (ppc64 real-glue binary built, not run) | a package only (`nix/mdio-tool.nix`); no NixOS option |
| external PHY / MACsec table parser + executor | `sai_extphy.cpp`, `sai_extmacsec.cpp` | `phy-seq` + `phy-seq-capi` | ~43,600 table comparisons; `test_sai` unchanged; seqcheck on five recordings; bundle (C++ comparisons skip there) | no | `make RUST_PHY_SEQ`, nix `rustPhySeq`, `hardware.openbcm.rustPhySeq` |
| external PHY runtime (link gate, debounce, counters, locks) | `sai_extphy.cpp` | not ported | | | |
| MACsec object model | `sai_macsec.cpp` | `sai-macsec` + `sai-macsec-phy` + `sai-macsec-capi`, inside the C++ libsai (`sai_macsec_rust.cpp`) | differential + e2e vs the C++; `test_sai` unchanged; bundle | no | `make RUST_MACSEC`, nix `rustMacsec`, `hardware.openbcm.rustMacsec` |
| diag socket | `sai_diag.cpp` | not ported | | | |
| temperatures (SAI attributes, `openbcm-asic-temp`) | `sai_temp.cpp`, `temp_files.c` | only `sdk-glue::temp` (for `mdio-tool --temp-out`) | `mdio-tool` tests on the fake glue | no | |

## Plan: replacing the C++ (accepted 2026-11-23)

Goal: our own C/C++ SAI (`ai_re/nix/packages/sai-openbcm/src`, later `sai-teralynx`) is replaced by this
workspace; what stays C is the boundary to third parties (`sdk_glue.c` for the OpenBCM SDK, the Teralynx shim).
The C++ stays the boot default and selectable until a board is proven on the Rust path on hardware.

**Target architecture.** `sai-ffi` (rim) over `sai-core`: the model plus the frontend semantics of
`sai_*.cpp`, one module per area (switch, port, l2, lag, l3, hostif, acl / copp, qos, mirror, hash, caps,
dump), over typed backend sub-traits that mirror the C++ `ob::Backend` op for op (121 ops; optional groups
default to "not supported", with the same capability probes). Backends: `virt` (parity with
`fake_backend.cpp`, records a call log), `sai-backend-openbcm` (the 72 ops of `openbcm_backend.cpp` over
`sdk_glue.h`, 75 functions), later `sai-backend-teralynx`. Beside the core, all `forbid(unsafe_code)`: the
external-PHY runtime (link gate, debounce, hold-off, admin sequences) over `phy-seq`, `sai-macsec` called
natively, the diag socket, temperatures. `phy-seq-capi` and `sai-macsec-capi` exist only to plug Rust into the
C++ and go away with it. The library reads the C++ profile key names (`SAI_OPENBCM_*`).

**Parity.** (1) `tests/test_sai.cpp` was white-box (3154 checks); since G1a it runs on a narrow test port
(`tests/test_port.h`), implemented over the C++ internals and, for this library, over a test-only C ABI
(cargo feature, never deployed), so one test source runs against both. (2) Differential: the same op scripts
into both libraries, comparing statuses, attributes and backend call logs. (3) Every fake-backend VM test of
`ai_re/nix/modules/switch/sonic/tests`. (4) The bundle on the big-endian target. (5) Hardware gates on the
AS7772 with the device's test scripts and bus-trace comparison.

| gate | content | status |
|---|---|---|
| G0 | hardware baseline with the phase-2 code: bundle, MDIO tool verify-only, MACsec on the C++ library with the Rust engine + model | done 2026-11-23 on the AS7772: bundle 55/55 suites (291 tests, harness 371); Rust MDIO tool `--verify-only` output equal to the C tool (32/32 PHYs, 160 reads); MACsec bring-up bus traces equal between C++, Rust engine, Rust engine + model (80 sequence runs / 22,560 bus operations per PHY); traffic, rekey, wrong CAK, reboot, cold boot, `kill -9` syncd pass on the Rust engine + model; box back on the C++ library. Evidence: `ai_re/devices/edge-core-as7772-32x/evidence/rust-g0/README.md` |
| G1a | trait split, area skeletons, test port in the C++ tests, recording backends, frontend driver | done 2026-11-23: `test_sai` 3014 + `test_units` 140 (was 3154), default `libsai.so` byte-identical; 121 backend ops typed (`sai-core::backend`), dispatch table, `Recording`, virt at fake parity; 314 Rust tests; unsafe unchanged (194) |
| G1 | frontend complete on the virtual backend (test port suite, differential, all fake VM tests incl. ACL + MACsec) | |
| G2 | hardware backend covers `openbcm_backend.cpp`; smoke on the AS7772 with SONiC stopped | |
| G3 | no C++ shims: Rust MDIO tool as the boot unit, ext-PHY runtime + MACsec native in `libsai.so` | |
| G4 | syncd on the Rust library on the AS7772 (opt-in): full feature set, device scripts, reboot, cold boot, rollback | |
| G5 | Rust the default on the AS7772, C++ selectable; soak (owner's approval) | |
| G6 | delete the sai-openbcm C++ except `sdk_glue.c`; the frozen C++ frontend moves into sai-teralynx | |

**Owner decisions.** The AS7760 is off the bench: the AS7772 proof counts for it (same ASIC, SDK, glue), so
G6 deletes the sai-openbcm C++ for both boards, after G4 and G5 on the AS7772. Before G6 the AS7760 system must
build and pass every offline tier on the Rust library (LACP PortChannels, VRF / ECMP, ingress ACL + counters,
CoPP, 25G break-out, the AS7712 LED program + port-order remap, serdes pre-emphasis keys), and the G6 report
lists which AS7760 behaviours never ran on silicon. sai-teralynx: offline track only, its C++ stays until the CS8210 is proven. MACsec: D1, Q7 and Q10 are
fixed in Rust (each with a test); every other Q item keeps the C++ behaviour. The clear-traffic window
(hold MACsec-profiled ports closed until the MACsec port exists; leave engines on at stack stop) is designed
as profile keys, off, not enabled without the owner. No bulk API and no warm boot (the C++ has neither).
`safe fn` declarations only for glue calls whose arguments are all scalars and whose C side checks ranges;
anything with pointers, buffers or callbacks keeps a small unsafe block with a SAFETY contract; both kinds are
counted. Bindings derived from vendor ifcs files are generated at build time, never committed.

## Where it lives, and why here

Its own Cargo workspace and lock file, like `rust/consoled`: in `rust/crates/` every edit
would rebuild fand (its source is that whole workspace) and every appliance's closure. Rust
code lives under `rust/` with its Nix next to it (`rust/README.md`). Each `nix/*.nix` takes what
it needs from ai_re (SAI headers, sai-openbcm sources, libsdk) as arguments, said in its
header; `ai_re/tests/nixtargets.py` wires them up ([Build and test](#build-and-test)).

## Crates

```
 syncd                                    C++ sai-openbcm libsai.so (optional Rust parts)
   | SAI C ABI (sai.h)                      | phy_seq.h                  | sai_macsec_rs.h
 sai-ffi       libsai.so: the rim         phy-seq-capi  <-- re-export -- sai-macsec-capi   (static libs)
   |                                        |                            |
 sai-core      model, store, bags         phy-seq  <---- sai-macsec-phy ---- sai-macsec  (MACsec model)
   | Backend    caps, events                | Dev, MdioBus
   +-- virt (module) -- tap-netdev        mdio-script
   +-- sai-backend-openbcm -- sdk-glue -- sdk-glue-sys --> sdk_glue.c (C) + libsdk
 sai-meta, sai-sys   attribute tables, bindgen of the SAI headers (under everything that sees SAI types)

 openbcm-mdio-script-rs: mdio-tool -- mdio-report -- mdio-script;  feature sdk: -- sdk-glue
```

Each crate's `src/lib.rs` documents it in detail; this section says what it is for.

### sai-sys: the C ABI is the header's

- `build.rs` runs bindgen over `sai.h` from `$SAI_INC` (+ `../experimental`). In the Nix
  build that is `SAI/inc` of the pinned sonic-sairedis source: the headers syncd is compiled
  with. Enums become integer constants; status codes (function-like macros) are wrapped in a
  C enum so the C front end computes them. No number is typed by hand.
- **Layout check 1** (every target, compile time): bindgen's size / alignment / offset
  assertions. **Layout check 2** (every build, native and cross): the same numbers and every
  enumerator as 7132 `_Static_assert`s compiled with the target's C compiler
  (`SAI_SYS_ABI_CC`; `SAI_SYS_ABI_CHECK=require` in Nix). Verified for `x86_64-unknown-linux-gnu`
  and `powerpc64-unknown-linux-gnuelfv2`.
- Exported functions are checked against the header's prototypes at compile time
  (`prototypes_match_the_headers`); method tables list every field, so a header that grows
  an entry stops compiling.

### sai-meta: attribute metadata without the C metadata library

SAI's own metadata only exists as C tables built by sairedis (`libsaimetadata.so`). The
headers carry its source as annotations (`@type`, `@flags`, `@objects`, `@allownull`,
`@condition`, `@default`); `sai-meta`'s build script turns them into Rust tables: 2054
attributes of 125 object types, with defaults, enumerators and counter ids. They decide the
union member an attribute travels in, create / set / read-only rules, mandatory attributes,
allowed object types, `@default` answers. Limits: 221 attributes have a type the rim does not
decode (176 ACL field / action data; 27 on implemented types), answered "not supported";
conditions are only "is conditional". A test pins known attributes; a header change that
confuses the scanner fails the build or that test.

### sai-core: the safe part

- **Typed ids** (`oid.rs`, 29 types; the C++ 64-bit layout), decoded once; below the entry
  points only typed ids. A typed id proves the type, not existence.
- **Store** (`store.rs`): an object is stored only when everything it refers to exists; a
  referenced object cannot be removed (`OBJECT_IN_USE`); indices are never reused.
- **Attribute checks** (`attrs.rs`) by the metadata before object code: unknown, read-only,
  duplicate, missing mandatory, object ids of a wrong type or null.
- **Bags and entries** (`bag.rs`): types whose whole state is their attributes, one
  metadata-driven implementation (references counted, `@default` on get, read-only values
  computed). FDB, neighbor, route entries the same. **Default objects** at `create_switch`:
  VR, STP, trap group, hashes, per port 8 queues / 8 scheduler groups / 8 PGs; VLAN 1 with
  every port. **Capabilities** (`caps.rs`), gated by `Backend::programs(object_type)`: what a
  backend does not program is not advertised (tested with a refusing backend). The 22 API
  tables have no null entry (`NOT_IMPLEMENTED` stubs, `sai-ffi/src/tables.rs`).
- **Failure rule**: backend first, store second; a failed backend call leaves model and chip
  as they were (property test). `create_switch` brings the backend up, disables every port.
- **`Backend`** (`backend.rs`): the first slice's 15 required operations plus 14 with default
  bodies, so a backend implements only what it programs: `port_queues`,
  `port_priority_groups`, `limits`, `programs`, `object_create` / `_set` / `_remove`,
  `hostif_create`, `entry_create` / `_set` / `_remove`, `stats_supported` / `_get` / `_clear`.
  `impl Backend for Box<dyn Backend>` lets the rim choose at run time. Link changes go on an
  `EventSink`; backends never call into the SAI layer.

### sai-ffi: the rim

One shape for every entry point (`generic.rs`): check pointers, decode by metadata, call the
core under the lock, encode, all inside `guard`. Every output pointer is checked before
anything is created, and written last. The backend is chosen at switch creation by
`SAI_RS_BACKEND` (`backends.rs`): `virtual` always, `openbcm` only in a library built with
the feature `openbcm-real` (real glue + libsdk) or `openbcm-fake` (in-memory glue); never
implicitly. Unsafe inventory: [Unsafe](#unsafe).

The helpers and what the C caller must guarantee (what cannot be checked):

| helper | checks itself | contract |
|---|---|---|
| `in_ref(p)` | null, alignment | non-null `p` points to an initialised value, unwritten during the call |
| `in_slice(p, n)` | `n == 0` gives `[]` whatever `p` is; null with `n > 0` is an error; alignment | `n` initialised elements, unwritten during the call |
| `inout_slice(p, n)` | as `in_slice` | `n` elements valid for read and write, not accessed otherwise |
| `out(p)` gives `Out` | null, alignment | valid for one write; `Out` can be written once and never read |
| `write_list(&mut count, list, items)` | SAI's list convention: count in = capacity, out = needed; too small or null buffer gives `BUFFER_OVERFLOW` with nothing written; empty result needs no buffer | buffer valid for `count` writes |
| `convert::decode` / `encode` | list pointers through the helpers above; `bool` read as a byte | the union member SAI defines for the attribute is initialised |

An attribute a call may not carry (read-only, create-only in a set) is refused before its
value is looked at.

### Panics

`guard` wraps every exported function and method-table entry in `catch_unwind`: a panic
becomes `SAI_STATUS_FAILURE`. The state is one `Mutex<Option<Api>>`; a panic while it is
held poisons it, and then **every** call fails until `sai_api_uninitialize` discards the
state: a half-applied change is never reported as success or built upon (test:
`a_panic_under_the_lock_fails_every_call_until_uninitialize`). `phy-seq-capi` and
`sai-macsec-capi` follow the same rule (`PHY_SEQ_E_PANIC`; the MACsec state is poisoned
until `ob_rs_macsec_detach`). Needs `panic = "unwind"` (set in the workspace profiles); with
`abort` a panic ends syncd instead.

### Threads and notifications

- Every SAI call takes the one mutex while the core runs. No async runtime. Backends queue
  events (`mpsc`); one notifier thread per switch applies each to the model under the lock,
  **releases the lock**, then calls the user's callback, which may call back into the
  library (the C harness does). Profile callbacks are also called without the lock.
- `remove_switch` / `sai_api_uninitialize` close the queue and join the thread before
  returning: no callback after they return. A switch-generation number keeps an event of a
  removed switch from being applied to the next one.
- Only `PORT_STATE_CHANGE_NOTIFY` is ever called; other callbacks are stored and returned.

### Profile keys

A value that does not parse is an error, not the default. The virtual keys are the C++
fake's (`SAI_OPENBCM_FAKE_*`), renamed.

| key | values (default) | read by |
|---|---|---|
| `SAI_RS_BACKEND` | `virtual` / `openbcm`; required, no default | `sai-ffi` |
| `SAI_RS_VIRTUAL_PORTS`, `_LANES`, `_SPEED` | 1..256 (32), 1..8 (4), Mb/s (100000) | virtual |
| `SAI_RS_VIRTUAL_LINK` | `admin`: link follows admin state / `none` (admin) | virtual |
| `SAI_RS_VIRTUAL_ON_HARDWARE` | `1`: start although `/dev/linux-user-bde` or `-kernel-bde` exists (also from the environment) | virtual |
| `SAI_RS_VIRTUAL_NETDEV` | `none` / `tap`: a TAP netdev per host interface (none); `tap` is refused on a host with a chip, whatever the key above says | virtual |
| `SAI_RS_VIRTUAL_WIRE` | `<netif>:<ifname>,...`: frames carried to a host interface while the port is up (tap only) | virtual |
| `SAI_INIT_CONFIG_FILE`, `SAI_OPENBCM_RC_FILE`, `SAI_OPENBCM_UNIT`, `SAI_OPENBCM_PORT_*` | the C++ backend's keys, same meaning (`sai-backend-openbcm/src/lib.rs`) | openbcm |

### tap-netdev

TAP netdevs (`IFF_TAP | IFF_NO_PI`, carrier switched with the port) and `AF_PACKET` wires for
the virtual backend, as the C++ fake makes them; needs `CAP_NET_ADMIN`. Its tests that create
netdevs run only in `vm-sai-rs-privileged` (`SAI_RS_PRIVILEGED_TESTS=1`); elsewhere they
skip. Byte order and ioctl numbers per architecture come from `libc` (checked on the
target: `ioctl_numbers_are_the_architectures`; TUNSETIFF = 0x800454ca on ppc64, as reported).

### Hardware backend: sai-backend-openbcm, sdk-glue, sdk-glue-sys

- `sdk-glue-sys`: declarations of `sdk_glue.h` (26 functions, one struct), generated from a
  table and checked against the header by the target's C compiler. `sdk_glue.c` stays C (the
  only file with SDK headers). Feature `real` links it with libsdk (`SDK_GLUE_LINK`), `fake`
  links `fake/fake_glue.c` (in-memory registers, scripted faults); one per linked program.
- `sdk-glue`: the only caller. The glue's rules (no MDIO before misc init, one bring-up per
  process, BCM calls only after "init all") are types (`Sdk`, `Switch`): table in its `lib.rs`.
- `sai-backend-openbcm`: `Backend` for the slice: bring-up (**resets the chip**), ports,
  VLANs, link events. Differences from `openbcm_backend.cpp`: `OB_E_MEMORY` /
  `OB_E_PORT` give `INSUFFICIENT_RESOURCES` / `INVALID_PARAMETER` (no Rust variants;
  `tests/parity.rs`); `SAI_OPENBCM_UNIT` / `_IFG` parsed strictly; a port that cannot be
  disabled or enumerated fails the bring-up; stderr, not syslog. Not done: L2 callback, L3
  special egress, temperatures, diag socket, ext-PHY admin sequences, MACsec reset, KNET.
- `nix/libsai-openbcm.nix`: the library with this backend plus `sai-openbcm-smoke`
  (`crates/sai-backend-openbcm/smoke/sai_smoke.c`: switch, ports, a VLAN, teardown; refuses
  without `--sonic-is-stopped`). Real glue (ppc64): `-z defs`, soname `libsai.so.1`, the same
  17 exports as the virtual build; never executed.

### mdio-script

Port of `mdio_script.{c,h}`: `Script::parse` gives typed operations, `Script::run` executes
them over `trait MdioBus`; clause 22 cannot be expressed (`Dev` is 1..31) and a `Script`
proves the whole file parsed. Differences from C: crate docs. Differential against the C
interpreter (`MDIO_SCRIPT_C_DIR` / `MDIO_SCRIPT_C_DRIVER`): return code, statistics,
messages, failure record, bus trace; 7253 comparisons, no disagreement (first slice).

### mdio-tool

`openbcm-mdio-script-rs` (`mdio-tool` over `mdio-report`, the port of
`mdio_script_main.c` / `mdio_report.c`): the same command line, messages, status files and
exit codes as the C tool, so it is a drop-in for the systemd unit
(`passthru.asOpenbcmMdioScript` installs it under the C name). Two buses: syncd's diag socket
(read-only, any build) and the SDK through `sdk-glue` (feature `sdk-real`, ppc64; `sdk-fake`
for tests, refused without `MDIO_TOOL_FAKE_SDK=1`). Differences from C (UTC in
`status.txt`, stricter diag answer check, script read once, ...) and what the types rule out:
`mdio-tool/src/lib.rs`, `mdio-report/src/lib.rs`. Not done: like the C tool, it calls no
`ob_sdk_shutdown` after `init_mdio`.

### phy-seq, phy-seq-capi

`phy-seq`: the external PHY tables (`ext-phy 1`, `ext-macsec 1`): parser, typed
parameters (wiped on drop, `zeroize`), executor over a bus trait, and the checkers
`openbcm-ext-macsec-rs` / `openbcm-ext-phy-rs`. Differences from the C++ engines (`u16`
values, no partial results after a failure, ...): its `lib.rs`. `phy-seq-capi`: `phy_seq.h`
+ `libphy_seq_capi.a`, seven exported functions, for the C++ libsai, which keeps its own
structs. Only the parser and executor are ported; the runtime around them (link gate,
debounce, counters, locks) is C++.

### sai-macsec, sai-macsec-phy, sai-macsec-capi

- `sai-macsec`: the object model of `sai_macsec.cpp` (MACSEC, MACSEC_PORT, FLOW, SC, SA, the
  MACsec-stage ACL glue) over a `SeqRunner` trait. Its `lib.rs` lists the intentional
  differences **D1-D8** and the questionable C++ behaviours kept, **Q1-Q19**.
- `sai-macsec-phy`: `SeqRunner` over `phy-seq`; with the above, the whole Rust path from a
  SAI object to MDIO words (`tests/e2e.rs` holds it to the C++ path access by access).
- `sai-macsec-capi`: `sai_macsec_rs.h` + `libsai_macsec_capi.a` (18 exports, ABI 1) behind
  `sai_macsec_rust.cpp`; re-exports `phy-seq-capi` (one Rust static library per C++ link).
  Visible in the library: D1 fixed; `NO_MEMORY` / `INVALID_PORT_NUMBER` become `FAILURE`; a
  failed get writes no earlier attributes; `extmacsec run` prints no words after a failure.

## Wiring into the C++ libsai

| switch | replaces | marker |
|---|---|---|
| `make RUST_PHY_SEQ=<prefix>`, nix `rustPhySeq` (`nix/phy-seq.nix`), NixOS `hardware.openbcm.rustPhySeq` | ext-phy / ext-macsec parsers and the sequence executor (`-DOB_RUST_PHY_SEQ`) | `share/sai-openbcm/phy-seq-engine` = `rust` |
| `make RUST_MACSEC=<prefix>`, nix `rustMacsec` (`nix/sai-macsec-capi.nix`), NixOS `hardware.openbcm.rustMacsec` | the MACsec object model: `sai_macsec_rust.cpp` instead of `sai_macsec.cpp` | `share/sai-openbcm/macsec-model` = `rust` |

Independent; all four combinations build and pass `test_sai` (3154 checks, 0 failures, as
reported 2026-11-22). With both, only `rustMacsec`'s library is linked. The options default
to `null` (C++), take the path of the `.nix` file, and are honoured by `boards/as7772`
(syncd's libsai swapped). ppc64 libsai: 1.49 MB, 2.22 MB with the Rust engine.

Trace hook, in both engines: `openbcm-diag "extmacsec trace [on [all] | off | dump | clear]"`
records the bus operations of sequence runs (key words `****`, read values not shown, last
50,000 lines), off by default (`tests/test_extmacsec_trace.cpp`, 29 checks). It exists to
compare the engines on hardware.

Stated plainly: this changed the **default** sai-openbcm sources (trace hook, `#ifdef`s, new
files `sai_macsec_rust.cpp`, `test_extmacsec_trace.cpp`, Makefile), so every system with
sai-openbcm (as7760, as7772, the sai-teralynx build) has a new drvPath. The default
`libsai.so` and `test_sai` were byte-identical with and without the switch plumbing; the
trace hook is the one change in the built library (off unless asked for).

## Big-endian runs

`nix/target-tests.nix`: every test executable of the workspace built for the target (test
profile), the debug and release `libsai.so`, the C harness, the C MDIO driver, the helpers
crates prebuilt (C MDIO tool, C++ MACsec drivers incl. the C++ library with the Rust model,
smoke program), the workspace's binaries, the AS7772 MACsec table, a sysroot (loader, libc,
libgcc_s, libstdc++). `nix/target-tests-run.sh` runs it: one line per suite, the harness on
the release library, what the suites report. From `ai_re/`: `just sai-rs-test-on
root@<host> sudo ssh` (a `mktemp` dir under `/tmp`, removed afterwards).

Variables the tests honour (`run.sh` sets them): `SAI_RS_TEST_TMPDIR` (scratch instead of the
build tree's temp dir), `SAI_RS_TEST_BIN` (prebuilt helpers), `SAI_RS_TEST_SUMMARY` (one line
per test about what it compared or skipped), `SAI_RS_VIRTUAL_ON_HARDWARE=1`, `SAI_FFI_HARNESS`
/ `SAI_FFI_LIBSAI`, `MDIO_SCRIPT_C_DRIVER`, `SAI_MACSEC_E2E_TABLE`.

Rule for new tests: no compiler at run time. A C / C++ helper is prebuilt by the crate's
`build.rs` into `OUT_DIR/target-tests` and found through `SAI_RS_TEST_BIN`; it must be
harmless when started without arguments (`run.sh` probes each so). A passing test's stderr
is captured, so a skip must also go to `SAI_RS_TEST_SUMMARY`.

Does not run there: the netdev tests (skip without `SAI_RS_PRIVILEGED_TESTS`); `phy-seq`'s
comparisons with the C++ `openbcm-ext-*` tools (not in the bundle; their skip does not reach
the summary); `sai-backend-openbcm`'s `parity.rs` (reads `openbcm_backend.cpp`) and its fake
smoke run (`SAI_SMOKE_LIBSAI`); anything touching the chip, the SDK or a service. About 5
minutes on the T2080, suites one after another.

## Parity

As reported 2026-11-22/23; targets in `ai_re/tests/nixtargets.py`.

- **syncd** (`vm-sai-rs-syncd`, `nix/test-syncd.nix`): syncd on this `libsai.so` runs
  `nix/sai_slice_smoke.py` through libsairedis: passes.
- **sai-openbcm's smoke test unchanged** (`vm-sai-rs-syncd-parity`): `sai_smoke.py` passes
  through syncd on the Rust libsai; 873 objects discovered; `sai_api_query` fails for 57 APIs
  (was 75). `nix/test-syncd-parity.nix` is a labelled copy of the C++ `test-syncd.nix`; with two
  parameters added to the original, `profileText` and `logExpect`, it becomes an import.
- **Full SONiC on the virtual backend** (`vm-sai-rs-sonic`): the C++ fake's test, check for
  check: 86 PASS, 0 failed, the same 3 N/A (LACP selects both wires, ping across the
  PortChannel). **L3** (`vm-sai-rs-sonic-l3`): passes. **ACL** (`vm-sai-rs-sonic-acl`): fails
  at `create_acl_table` (INGRESS, PORT + LAG: `NOT_IMPLEMENTED`), and two CoPP checks grep C++
  log lines; in nixtargets' `SKIP` (fails by design).
- All six (`just test-vm -k "sai-rs or mdio-units-rs"`: syncd, syncd-parity, sonic, sonic-l3,
  privileged, mdio-units-rs) passed from the working tree on 2026-11-23 01:59Z, with sairedis
  built against the current libsai; the ACL one is skipped.
- **MDIO tool**: 159 differential runs against the C tool (44 fake chip, 75 without a chip, 40
  `--verify-only --diag-socket`): exit code, stdout, stderr, status files, bus trace equal.
  `vm-mdio-units-rs`: the units VM test, unmodified, passes 9 subtests on the Rust binary.
- **phy-seq**: 26,285 generated ext-macsec + 16,000 ext-phy + 1,307 corpus / board table
  comparisons with the C++ tools, no disagreement. **test_sai** (3154 checks) unchanged and
  passing on the Rust engine, the Rust model and both; the trace test 29 / 0.
- **sai-macsec** against the unmodified `sai_macsec.cpp` on the fake backend: 16 backbone +
  272 failure-injection + 300 random scripts per run, a 20,000-script soak: no disagreement
  but D1. **End to end** at MDIO level on the current AS7772 table (sha256 1ef74ead...): 4
  backbones + 48 random scripts, and the C++ library with the Rust model against the C++ one
  through the SAI API: equal. On the AS7772: the status block.
- **seqcheck** on the five real recordings with the new table (`0x5000` ingress write): C++
  tool, `openbcm-ext-macsec-rs`, the C++ library on the Rust engine and the build with both
  agree (s1-s3: 40 sequences / 4602 operations; s4-s5: 4 / 1952).

## Unsafe

Policy: `#![forbid(unsafe_code)]` wherever there is no FFI. The rims deny it at the root and
allow it in named modules only. Every block has a `// SAFETY:` comment and one unsafe
operation (clippy `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block` are errors).
No `static mut` and no `unsafe impl` in hand-written code.

Counted 2026-11-22: `grep -c 'unsafe {'` per file of `crates/*/src`, lines before the
file's `#[cfg(test)]` module, comment lines excluded (the in-file tests add 71 more in
`sai-ffi`, 10 in `phy-seq-capi`).

| crate | attribute | blocks | exports | what |
|---|---|---|---|---|
| `sai-ffi` | (workspace lints) | 68: generic 24, convert 13, tables 12, global 9, ptr 6, state 2, notify 2 | 11 `no_mangle` | the SAI C ABI; `convert` / `tables` blocks sit in macros |
| `sai-macsec-capi` | deny; allowed in ptr, attr, state, api | 60: attr 28 (union reads), api 20, ptr 6, state 6 | 18 `no_mangle` | `sai_macsec_rs.h`, C++ callbacks |
| `sdk-glue` | none | 29: port 11, vlan 5, error 2, unit 2, mdio 2, switch 2, link 2, fake 2, temp 1 | | one block per glue call |
| `phy-seq-capi` | deny; allowed in ptr, api | 28: api 24, ptr 4 | 7 `no_mangle` | `phy_seq.h` |
| `tap-netdev` | deny; allowed in sys | 9 | | ioctls (TUNSETIFF, TUNSETCARRIER, SIOCSIFHWADDR), if_nametoindex, socket, bind, setsockopt, poll |
| `sai-sys`, `sdk-glue-sys` | none | 0 | generated `unsafe extern "C"` blocks | declarations only |
| the other nine | forbid | 0 | | |

Total 194 blocks in five crates. First slice: 29 blocks and 8 `no_mangle`, all in `sai-ffi`.

### Miri

`cargo miri test -p sai-ffi --lib --test abi` (rustup nightly, `-Zmiri-disable-isolation`):
14 + 1 tests pass (2026-11-22, before the netdev / LAG additions; not re-run since),
including the lifecycle test with the notifier thread and the callback. It found one defect
in the first slice, fixed: a read-only list attribute passed to set had its list pointer
read before the call was refused. Limit: the callback pointer is kept in the safe core as an
address, so two integer-to-pointer casts remain ("might miss pointer bugs"). **Not run under
Miri**: `phy-seq-capi`, `sai-macsec-capi`, `tap-netdev` (syscalls), `sdk-glue` (calls C), the
C harness. The pinned-nixpkgs toolchain has no Miri.

## Dependencies

Direct dependencies, all of them (the rest of `Cargo.lock` is theirs):

- `bindgen` (build only): C headers to Rust (SAI, `phy_seq.h`, `sai_macsec_rs.h`), with
  `prettyplease` so the output does not depend on a rustfmt binary.
- `proptest` (tests only): property tests.
- `zeroize`: wipes key material (phy-seq's `Params`, sai-macsec's secrets); no dependencies.
- `aes` (RustCrypto): only `sai-macsec`'s `derive_hkey` / `AuthKeyPolicy::Verify` (off by
  default; the C++ does no AES). 8 crates; removable with that check.
- `signal-hook`: `mdio-tool` only; SIGTERM / SIGINT set a flag (std has no signal handling).
- `libc`: `tap-netdev` only; ioctl numbers, struct layouts, syscalls per architecture.

Lints: `[workspace.lints]` (clippy `all` + `pedantic`; no `unwrap` / `expect` / `panic` /
indexing in library code).

## Found by the port

In the C++, not fixed there:

- **D1** `sai_macsec.cpp` finds an installed SA through `sc.hw` (set :241, used :960 / :977
  remove, :379 poll): after a flow moves to another port `remove_macsec_sa` runs nothing and
  returns success; **the key stays in the first PHY**. Fixed only in the Rust model (test
  `where_a_flow_moved_the_cpp_leaves_the_key_in_the_phy`; the e2e flow-move script).
- **Q1-Q19**: questionable C++ behaviours ported as they are, with line numbers, in
  `crates/sai-macsec/src/lib.rs` (e.g. Q2 detach leaves engines on, Q7 a flow switch leaves
  the old SC's SAs installed, Q10 a cipher change under waiting SAs installs the first 16 key
  bytes, Q11 AUTH_KEY trusted).

In Rust, fixed: `sdk-glue`'s `Claim::take` (eager `then_some`) let a refused second
bring-up release the live handle's claim; the first slice's MDIO label round trip.

## Not done

- No Rust SAI frontend or backend on the ASIC; no boot-time PHY bring-up by the Rust MDIO tool.
- Hardware backend: ports, VLANs, link events only (no PHY admin sequences, MACsec reset,
  temperatures, diag socket, KNET netdevs, L2 / L3 / ACL).
- Frontend: ACL, mirror, samplepacket, MACsec objects in `libsai.so`, STP ports, bulk, dump,
  FDB events, warm boot; L3 / QoS bags have no C++ behaviour; one switch, no breakout. Not
  answered like the C++: `DEFAULT_OVERRIDE_VIRTUAL_ROUTER_ID`, CPU buffer pools,
  `VXLAN_DEFAULT_ROUTER_MAC`, `MAX_STP_INSTANCE`. 221 undecoded attributes; the capability
  table is gated by `Backend::programs` but is not the C++'s. Logs to stderr.
- External PHY runtime, diag socket, temperatures: C++ only. No fuzz target.
- Deploy path: the root git flake with the Rust options is unbuilt; the new files are
  untracked, so a git flake does not see them (`ai_re/tools/nix-untracked.sh`). Only `path:`
  builds of `as7772-system-rust-phy-seq` / `-both` exist.

## Hardware runbooks

Run on the AS7772 (G0, 2026-11-23; evidence in `ai_re/devices/edge-core-as7772-32x/evidence/rust-g0/`): the
bundle, the MDIO tool `--verify-only` through the diag socket against the C tool, MACsec on the C++ library
with the Rust engine (`rustPhySeq`) and with engine + model (`rustMacsec`). Not traced there: the probe /
reset at syncd start, ext-PHY sequences, rekeys (covered by the rekey test). **Not run yet**: a cold boot with
the Rust MDIO tool as the boot unit, and the openbcm smoke of the Rust `libsai.so`
(`sai-openbcm-smoke --sonic-is-stopped ...`, SONiC stopped; it resets the chip): no Rust SAI frontend or
backend has driven the ASIC.

## Build and test

From `ai_re/` (what is in the working tree):

```sh
just test-eval                    # every target evaluates
just test-packages -k 'sai-rs or phy-seq or sai-macsec or rust or mdio-tool'
                                  # native: fmt, clippy -D warnings, tests, harness, differentials; ppc64: cross builds
just test-systems -k rust         # as7772-system-rust-phy-seq, as7772-system-rust-both (path: flake)
just test-vm -k 'sai-rs or mdio-units-rs'   # syncd, parity, SONiC, L3, privileged, MDIO units (kvm)
just sai-rs-test-on root@<host> sudo ssh    # the bundle on a switch (Big-endian runs)
```

By hand, in the package's own build environment:

```sh
cd rust/sai
E='let f = builtins.getFlake "path:'$PWD'/../../ai_re/nix"; p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
   sonic = import (f.outPath + "/packages/sonic") { pkgs = p; };
   in p.callPackage ./nix/package.nix { saiSrc = sonic.sources.sairedis; mdioScriptC = f.outPath + "/packages/sai-openbcm/src"; }'
nix develop --impure --expr "$E" -c cargo test --workspace
nix develop --impure --expr "$E" -c cargo clippy --workspace --all-targets -- -D warnings
nix develop --impure --expr "$E" -c cargo fmt --all --check
```

`sdk-glue-sys`'s `real` and `fake` exclude each other in one link: a crate built with `real`
(`mdio-tool --features sdk-real`, `sai-ffi --features openbcm-real`) cannot be tested in the
same cargo invocation as dev-dependencies that enable `fake`; `nix/mdio-tool.nix` and
`nix/libsai-openbcm.nix` build them alone. Outside Nix: `SAI_INC=<SAI/inc> cargo test
--workspace` with libclang and a C compiler (`LIBCLANG_PATH`, `CC`); differentials skip
without `MDIO_SCRIPT_C_DIR` / `SAI_OPENBCM_SRC`.

### Updating the numbers in this file

Unsafe counts: the method under [Unsafe](#unsafe). Test, suite and comparison counts: the end
of `run.sh`'s output (the Nix checks print the same lines). Change a number with its date.

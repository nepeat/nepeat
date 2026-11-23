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
- **Frontend (G1, 2026-11-23, x86_64):** every object type of the C++ frontend is ported into
  `sai-core` (one area module per `sai_*.cpp`, MACsec through the model natively, the external-PHY
  runtime, the diag socket, temperatures, dump), on the virtual backend at C++-fake parity. The
  C++ suite `test_sai.cpp` runs unmodified on this library: 3014 of 3014 checks pass; the op-script
  differential against the C++ frontend: 455 of 455 operations identical (11 scripts, MACsec and
  external PHY included), allow-list empty ([Parity](#parity)).
- Everything else ran on x86_64 (cargo, Nix checks), in NixOS VMs ([Parity](#parity)) and
  partly on ppc64 under qemu-user.

## Migration status

"Bundle" = the AS7772 userspace run above. "On chip": see the status above (G0).

| component | C/C++ today (default on the switch) | Rust (crate) | parity evidence, where tested | on chip | selected by |
|---|---|---|---|---|---|
| SAI frontend | `sai_*.cpp` | `sai-ffi` (rim) + `sai-core` (+ `sai-meta`, `sai-sys`): switch, port, L2, LAG, hostif, L3, ACL / policer / CoPP, QoS, mirror / samplepacket, hash, MACsec objects, capabilities, dump, log, temperatures; refusals answered as each `sai_*.cpp` | `test_sai` 3014 / 3014 through the test port; differential 455 / 455 ops; C harness 374 checks; SONiC VM tests ([Parity](#parity)); x86 | no | the `libsai.so` syncd links (`nix/package.nix`); no NixOS option |
| ... not in the C++ either | | bulk operations, warm boot, STP ports, tunnels, packet I/O through the SAI | | | |
| virtual backend + netdevs | `fake_backend.cpp` | `sai-core::virt` + `tap-netdev`: the fake's models (QoS, mirror, L2 extras behind its toggles), numbering and answers under the C++ keys | the suites above; `vm-sai-rs-privileged` | never (refuses next to a chip) | `SAI_OPENBCM_BACKEND=fake` (or `SAI_RS_BACKEND=virtual`) |
| hardware backend | `openbcm_backend.cpp` | `sai-backend-openbcm` over `sdk-glue` / `sdk-glue-sys`: ports, VLANs, link events, board quirks; the other 50 required typed operations answer `NotSupported` (`src/g2.rs`, gate G2) | fake glue: its tests, smoke (x86, qemu-ppc64); real glue: links against libsdk, never executed | no | cargo feature `openbcm-real` / `-fake` (`nix/libsai-openbcm.nix`) + `SAI_OPENBCM_BACKEND=openbcm` |
| SDK glue | `sdk_glue.c` | stays C; bound by `sdk-glue-sys`, wrapped by `sdk-glue` | declarations checked against `sdk_glue.h` by the C compiler (x86, ppc64) | the C file, inside the C++ library; the Rust bindings no | features `real` / `fake` |
| MDIO script interpreter | `mdio_script.c` | `mdio-script` | differential, 7253 comparisons; bundle | no | inside `mdio-tool` |
| MDIO report + boot-time tool | `mdio_report.c`, `mdio_script_main.c` (`openbcm-mdio-script`) | `mdio-report`, `mdio-tool` (`openbcm-mdio-script-rs`) | 159 differential runs vs the C tool; `vm-mdio-units-rs`; bundle (fake chip) | no (ppc64 real-glue binary built, not run) | a package only (`nix/mdio-tool.nix`); no NixOS option |
| external PHY / MACsec table parser + executor | `sai_extphy.cpp`, `sai_extmacsec.cpp` | `phy-seq` (+ `phy-seq-capi` for the C++) | ~43,600 table comparisons; `test_sai` unchanged; seqcheck on five recordings; bundle | no | in `libsai.so` always; C++: `make RUST_PHY_SEQ`, nix `rustPhySeq`, `hardware.openbcm.rustPhySeq` |
| external PHY runtime (link gate, debounce, counters, locks) | `sai_extphy.cpp` | `sai-extphy` + `sai-core::extphy` | `test_sai` `test_ext_phy` (118 checks); differential `10-ext-phy` | no | `SAI_OPENBCM_EXT_PHY_FILE` |
| MACsec object model | `sai_macsec.cpp` | `sai-macsec` + `sai-macsec-phy`, natively behind `sai-core::macsec`; for the C++ libsai also `sai-macsec-capi` (`sai_macsec_rust.cpp`) | `test_sai` `test_macsec` (175 checks); differential `11-macsec` (93 ops); differential + e2e vs the C++; bundle | no (G0: inside the C++ libsai) | `SAI_OPENBCM_EXT_MACSEC_FILE`; C++: `make RUST_MACSEC`, nix `rustMacsec`, `hardware.openbcm.rustMacsec` |
| diag socket | `sai_diag.cpp`, `openbcm_diag.c` | `sai-diag` (server, under the SAI lock) + `openbcm-diag` (client) | `test_sai` `test_diag`; client differential vs the C tool | no | `SAI_OPENBCM_DIAG_SOCKET` |
| temperatures (SAI attributes, publisher) | `sai_temp.cpp`, `temp_files.c` | `sai-core::temp` (publisher thread, files); `sdk-glue::temp` | `test_sai` `test_temperature` | no | `SAI_OPENBCM_TEMP_FILE`, `_PROM`, `_INTERVAL` |

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
| G1 | frontend complete on the virtual backend (test port suite, differential, all fake VM tests incl. ACL + MACsec) | green 2026-11-23 (x86_64): `test_sai` 3014 / 3014 on the Rust library (strict, also `nix/parity-harness.nix`); differential 455 / 455 ops, allow-list empty; `cargo test --workspace` 525 / 0; fmt, clippy `-D warnings`; Miri on the rim's lib and C-free tests; the SONiC VM tests incl. ACL pass. no fake-backend MACsec VM test exists (`vm-macsec.nix` runs SONiC's own virtual SAI, not libsai) |
| G2 | hardware backend covers `openbcm_backend.cpp`; smoke on the AS7772 with SONiC stopped | |
| G3 | no C++ shims: Rust MDIO tool as the boot unit, ext-PHY runtime + MACsec native in `libsai.so` | |
| G4 | syncd on the Rust library on the AS7772 (opt-in): full feature set, device scripts, reboot, cold boot, rollback | |
| G5 | Rust the default on the AS7772 as soon as G4 passes | |
| G6 | delete the sai-openbcm C++ except `sdk_glue.c`; the frozen C++ frontend moves into sai-teralynx | |

**Owner decisions.** "Push as it comes, remove the C code as things get ported": once a Rust replacement is
ported and proven (parity tests green and, for what runs on hardware on the bench, run on that hardware), the
C/C++ it replaces is deleted in the same gate, with no permanent C fallback (rollback during the proof = the
previous generation / commit). So G5 needs no separate approval: when G4 passes, Rust becomes the AS7772
default and G6 follows; components proven earlier (the C MDIO tools, the C++ engines + the two capi crates)
go earlier. The AS7760 is off the bench: the AS7772 proof counts for it (same ASIC, SDK, glue), so
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
   |   (+ sai-test-port: test-only ABI)     |                            |
 sai-core      model, areas, store        phy-seq  <---- sai-macsec-phy ---- sai-macsec  (MACsec model)
   | Backend   caps, events, refusals       | Dev, MdioBus       ^ (natively, sai-core::macsec)
   | (16 typed sub-traits)                sai-extphy (ext-PHY runtime)   sai-diag (diag socket server)
   +-- virt (module) -- tap-netdev        mdio-script
   +-- sai-backend-openbcm -- sdk-glue -- sdk-glue-sys --> sdk_glue.c (C) + libsdk
 sai-meta, sai-sys   attribute tables, bindgen of the SAI headers (under everything that sees SAI types)

 openbcm-mdio-script-rs: mdio-tool -- mdio-report -- mdio-script;  feature sdk: -- sdk-glue
 openbcm-diag: the diag socket client (openbcm_diag.c);  sai-driver: op-script driver (differential)
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
allowed object types, `@default` answers. Limits: 42 attributes have a type the rim does not
decode (ACL field / action data and the `MACsec` keys are decoded), answered "not supported";
conditions are only "is conditional". A test pins known attributes; a header change that
confuses the scanner fails the build or that test.

### sai-core: the safe part

- **Typed ids** (`oid.rs`, 37 types + 3 entry types; the `MACsec` ones are the model's; the C++
  64-bit layout), decoded once; below the entry
  points only typed ids. A typed id proves the type, not existence.
- **Store** (`store.rs`): an object is stored only when everything it refers to exists; a
  referenced object cannot be removed (`OBJECT_IN_USE`); indices are never reused.
- **Attribute checks** (`attrs.rs`) by the metadata before object code: duplicate, missing
  mandatory, values of the wrong kind, object ids of a wrong type or null (but where the C++ takes
  null as a default: a trap's `TRAP_GROUP`). An attribute a call may not carry (unknown,
  read-only, create-only in a set) gets the status the type's C++ file gives it: `dispatch.rs`
  `refused_create` / `refused_get` and each area's `Area::refused_set` (after the object lookup, as
  the C++); the metadata's `INVALID_ATTRIBUTE` / `UNKNOWN_ATTRIBUTE` only where the C++ defines
  nothing. An id of another type is `INVALID_OBJECT_ID` for every type (the C++ looks it up).
- **Areas** (`dispatch.rs` `OWNERS`): one module per C++ file (`l2`, `lag`, `hostif`, `l3`,
  `acl` + `policer` + `copp`, `qos`, `mirror`, `hash`; switch and port in `sai.rs` / `switch.rs` /
  `port.rs`), each implementing `Area` for its types; the `MACsec` API types go to the model
  (`macsec.rs`). **Default objects** at `create_switch`: VR, STP, trap group, per port 8 queues /
  8 PGs (scheduler groups and hashes with the backend's QoS / hash support, as the C++); VLAN 1
  with every port. **Capabilities** (`caps.rs`), gated by `Backend::programs(object_type)`. The
  API tables have no null entry (`NOT_IMPLEMENTED` stubs, `sai-ffi/src/tables.rs`).
- **Failure rule**: backend first, store second; a failed backend call leaves model and chip
  as they were (property test). `create_switch` brings the backend up, disables every port.
- **`Backend`** (`backend/`): the C++ `ob::Backend` op for op. The supertrait keeps the first
  slice's 15 operations (`LEGACY_CPP_OPS`) and the queries `port_queues`, `port_priority_groups`,
  `limits`, `programs`, `temperature_reader`, `mdio_bus`, `stats_*`; the other 106 are typed
  sub-traits (`ops.rs`: `Base`, `Shell`, `Temp`, `Mdio`, `PortOps`, `Lag`, `L2`, `Hostif`, `L3`,
  `Field`, `Policer`, `Qos`, `Mirror`, `Sample`, `Hash`, `Storm`), written once in a macro that
  also writes the `Box<B>` forwarding and the `Recording` decorator (the C++ recording backend's
  call-log lines). The 50 operations the frontend calls that are pure virtual in the C++ are
  `required` (no default); the optional ones answer `NotSupported` / `false` by default.
  `Refusing<B>` is a chip with the first slice only (tests). Link and FDB events go on an
  `EventSink`; backends never call into the SAI layer.

### sai-ffi: the rim

One shape for every entry point (`generic.rs`), the `MACsec` API included: check pointers, decode
by metadata, call the core under the lock, encode, all inside `guard`. Every output pointer is
checked before anything is created, and written last; an unusable input pointer is answered after
the object lookup, as the C++ orders it. `MACsec` attributes are decoded without the metadata's
flag check (the model answers in the C++'s order); key material goes straight into a `Secret`
(`convert.rs` `decode_secret`). The backend is chosen at switch creation by `SAI_OPENBCM_BACKEND`
(or `SAI_RS_BACKEND`; `backends.rs`, `sai_core::keys`): `fake` / `virtual` always, `openbcm` only
in a library built with the feature `openbcm-real` (real glue + libsdk) or `openbcm-fake`
(in-memory glue); no key is `openbcm` where the library has it and an error where it does not:
the in-memory chip is never chosen implicitly. Unsafe inventory: [Unsafe](#unsafe).

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
- `PORT_STATE_CHANGE_NOTIFY` and `FDB_EVENT_NOTIFY` (one call per event, the C++'s attributes)
  are called; other callbacks are stored and returned.

### Profile keys

The C++ names (`SAI_OPENBCM_*`), so a NixOS module's `sai.profile` works for either library
(`sai_core::keys`); the Rust names are aliases (`SAI_RS_*`, until the C++ is gone) and win when
both are set. A value that does not parse is an error, not the default.

| key (alias) | values (default) | read by |
|---|---|---|
| `SAI_OPENBCM_BACKEND` (`SAI_RS_BACKEND`) | `openbcm` / `fake` (`virtual`); none: `openbcm` where built in, else an error | `sai-ffi` |
| `SAI_OPENBCM_FAKE_PORTS`, `_LANES`, `_SPEED` (`SAI_RS_VIRTUAL_*`) | 1..256 (32), 1..8 (4), Mb/s (100000) | virtual |
| `SAI_OPENBCM_FAKE_LINK` | `admin`: link follows admin state / `none` (admin) | virtual |
| `SAI_OPENBCM_FAKE_ON_HARDWARE` | `1`: start although `/dev/linux-user-bde` or `-kernel-bde` exists (also `SAI_RS_VIRTUAL_ON_HARDWARE` from the environment) | virtual |
| `SAI_OPENBCM_FAKE_NETDEV` | `none` / `tap`: a TAP netdev per host interface (none); `tap` is refused on a host with a chip | virtual |
| `SAI_OPENBCM_FAKE_WIRE` | `<netif>:<ifname>,...`: frames carried to a host interface while the port is up (tap only) | virtual |
| `SAI_RS_VIRTUAL_COMPAT` | `cpp` / `rust` (`cpp` when chosen by `SAI_OPENBCM_BACKEND=fake`, else `rust`): the C++ fake's port numbers and answers, or the Rust chip's (`virt.rs`) | virtual |
| `SAI_INIT_CONFIG_FILE`, `SAI_OPENBCM_RC_FILE`, `_UNIT`, `_KNET`, `_PORT_*` | the C++ backend's keys, same meaning (`sai-backend-openbcm/src/lib.rs`, `sai-core::port` board quirks) | openbcm, core |
| `SAI_OPENBCM_RIF_VLAN_BASE`, `_TEMP_FILE`, `_TEMP_PROM`, `_TEMP_INTERVAL`, `_LOG_STDERR` | as the C++ (`sai_l3.cpp`, `sai_temp.cpp`, `sai_api.cpp`) | core |
| `SAI_OPENBCM_DIAG_SOCKET` | the diag socket's path (none: no server) | `sai-diag` |
| `SAI_OPENBCM_EXT_PHY_FILE`, `SAI_OPENBCM_EXT_MACSEC_FILE` | the external PHY / `MACsec` tables (none) | `sai-extphy`, `sai-core::macsec` |
| `SAI_OPENBCM_MACSEC_HOLD_CLOSED` | `all` / first lanes `1,5,...`: those ports' link stays down until they have a `MACSEC_PORT` (off) | designed, **off**: the owner's decision on the clear-traffic window |
| `SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP` | `1`: removing a `MACSEC_PORT` leaves its engine on, data dropped (`0`) | designed, **off**: the same decision |

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
  VLANs, link events, board quirks; every other required typed operation answers `NotSupported`
  explicitly (`src/g2.rs`, each marked `G2`). Differences from `openbcm_backend.cpp`: `OB_E_MEMORY` /
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
- In the Rust `libsai.so` the model runs natively (`sai-core::macsec`, no C between): the five
  `MACSEC` API types and the `MACsec`-stage ACL tables / entries go to it through the generic
  entry points. Three C++ behaviours are fixed in Rust (owner's decision 2026-11-23, each with a
  test): **D1** (an SA of a flow that moved stays in the first PHY), **D9 = Q7** (a flow switch
  leaves the old SC's SAs installed), **D10 = Q10** (a cipher change under waiting SAs installs
  the first 16 key bytes); every other Q item keeps the C++ behaviour.

### sai-extphy, sai-diag, openbcm-diag

- `sai-extphy`: the runtime of `sai_extphy.cpp` around `phy-seq`'s parser and executor: the link
  gate the backend's link scan asks, debounce (`up-polls`), hold-off after foreign MDIO, the admin
  sequences, counters and status text, behind its own lock (never the SAI lock). The SAI layer
  holds it per switch (`sai-core::extphy`).
- `sai-diag`: the diag socket server of `sai_diag.cpp` (one thread, one connection at a time,
  root only, the command under the SAI lock, output written after it is released; differences
  in its `lib.rs`). `openbcm-diag`: the client `openbcm_diag.c`, same command line, bytes and exit
  codes (differential against the C tool).

### sai-test-port, sai-driver (the parity harnesses)

- `sai-test-port`: the answering side of `sai_rs_test_query`, the one test-only export of a
  `libsai.so` built with `sai-ffi`'s feature `test-port` (never installed: `nix/package.nix`
  builds without it). Over it `tests/test_port_rust.cpp` (sai-openbcm) implements
  `tests/test_port.h`, so the unmodified `test_sai.cpp` runs on this library
  (`tests/test-port/run.sh`).
- `sai-driver`: the Rust half of the op-script differential (grammar and output of the C++
  `frontend_driver`, profile identical): sai-openbcm's `tests/driver-scripts/*.ops` and this
  crate's own (`tests/driver-scripts`: external PHY, MACsec with key material, on the tables of
  `tests/test_tables.h`) through both drivers; every differing operation must be in
  `tests/differential-allow.txt` (empty).

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

Targets in `ai_re/tests/nixtargets.py`. The C++ library is the reference: where a C++ status or
call is observed by a shared suite or the op scripts, this library gives the same.

- **`test_sai` on this library** (2026-11-23): the unmodified C++ suite through the test port,
  `tests/test-port/run.sh --strict` (`SAI_INC=...`, from `rust/sai`): 3014 checks, 0 failures; the
  C++ reference run of the same `test_sai.o` 3014 / 0. Also `nix/parity-harness.nix`.
- **Op-script differential** (`cargo test -p sai-driver --test differential`, with
  `SAI_OPENBCM_SRC` / `SAI_OPENBCM_TESTS` and a C++ compiler): 11 scripts, 455 operations, every
  one identical (status, values, every backend call, notifications); the allow-list is empty.
  Scripts 10 and 11 (this crate's) cover the external-PHY runtime and the MACsec objects with key
  material.
- **C harness** (`sai-ffi/tests/harness.c`, 374 checks through `dlopen`): every expectation is the
  C++ library's answer or a listed difference below, checked by running a copy of the harness
  (C++ profile keys) against the C++ `libsai.so`.
- **Deliberate differences from the C++ that remain** (each also at its code):
  - a failed get writes nothing (the C++ has written the attributes before the failing one, and
    a failed `get_port_stats` zeros);
  - every list of a get is encoded even after one did not fit, so one retry suffices;
  - argument checks the C++ does not make: an attribute given twice, the switch id of a create, an
    entry key's address family (`INVALID_PARAMETER`; a removed route of family none: C++
    `ITEM_NOT_FOUND`), null pointers before anything else in create (C++: after its pre-loop
    checks); a create's refused attribute is answered before the type's own checks;
  - no null method-table entry (`NOT_IMPLEMENTED` functions); `sai_api_query` of an API out of
    range: `INVALID_PARAMETER`;
  - the user-defined trap has set / get / remove (C++: null entries) and a create;
  - set of the create-only port `FULL_DUPLEX_MODE`: `ATTR_NOT_SUPPORTED_0` (C++ stores it);
  - the STP, next-hop and table-entry sets look the object up first (C++: no lookup, no lock);
  - MACsec: D1, D9 (Q7), D10 (Q10) fixed; `NO_MEMORY` / `INVALID_PORT_NUMBER` from the model
    become `FAILURE`;
  - the virtual chip's QoS, mirror and L2-extras models are off (as the C++ fake's toggles) and
    reachable only through the test port and the driver (`feature`).
- **syncd** (`vm-sai-rs-syncd`, `nix/test-syncd.nix`, Rust key names): syncd on this `libsai.so`
  runs `nix/sai_slice_smoke.py` through libsairedis. **sai-openbcm's smoke test unchanged**
  (`vm-sai-rs-syncd-parity`, the C++ profile): `sai_smoke.py` through syncd, the C++ test's ASIC_DB
  expectations; `nix/test-syncd-parity.nix` differs from the C++ `test-syncd.nix` only by its
  diagnostics and the C++'s "oper up" log line.
- **Full SONiC on the virtual backend**: `vm-sai-rs-sonic`, `-l3`, `-acl` are the C++ fake's tests
  with only libsai replaced (profile and scripts the originals'). Results of this tree (2026-11-23, `just test-vm -k "sai-rs or mdio-units-rs"` + the ACL one by its
  expression): `vm-sai-rs-sonic` 86 PASS, 0 failed, 3 N/A (the fake's breakout / lanemap checks);
  `-l3` passes; `-acl` 27 / 27 PASS (CoPP, ingress ACL + counter, the L3V6 table): it can leave
  nixtargets' `SKIP`. `vm-sai-rs-syncd` (`sai_api_query` fails for 53 APIs), `-syncd-parity`,
  `-privileged` and `vm-mdio-units-rs` pass.
- **MDIO tool**: 159 differential runs against the C tool (44 fake chip, 75 without a chip, 40
  `--verify-only --diag-socket`): exit code, stdout, stderr, status files, bus trace equal.
  `vm-mdio-units-rs`: the units VM test, unmodified, on the Rust binary.
- **phy-seq**: 26,285 generated ext-macsec + 16,000 ext-phy + 1,307 corpus / board table
  comparisons with the C++ tools, no disagreement. The C++ `test_sai` (3154 checks then) passes
  on the Rust engine, the Rust model and both; the trace test 29 / 0.
- **sai-macsec** against the unmodified `sai_macsec.cpp` on the fake backend: 16 backbone +
  272 failure-injection + 300 random scripts per run, a 20,000-script soak: no disagreement
  but D1 / D9 / D10. **End to end** at MDIO level on the current AS7772 table (sha256 1ef74ead...):
  4 backbones + 48 random scripts, and the C++ library with the Rust model against the C++ one
  through the SAI API: equal. On the AS7772: the status block.
- **seqcheck** on the five real recordings with the new table (`0x5000` ingress write): C++
  tool, `openbcm-ext-macsec-rs`, the C++ library on the Rust engine and the build with both
  agree (s1-s3: 40 sequences / 4602 operations; s4-s5: 4 / 1952).

## Unsafe

Policy: `#![forbid(unsafe_code)]` wherever there is no FFI. The rims deny it at the root and
allow it in named modules only. Every block has a `// SAFETY:` comment and one unsafe
operation (clippy `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block` are errors).
No `static mut` and no `unsafe impl` in hand-written code.

Counted 2026-11-23 with `tests/unsafe-count.sh` (from `rust/sai`): per file of `crates/*/src`, the
`unsafe {` blocks before the file's `#[cfg(test)]` module (comment lines excluded); after it and in
`tests/` they are counted apart. Exports: the `.dynsym` set `sai-ffi/tests/exports.rs` checks.
`safe fn` declarations: `sdk-glue-sys`'s generated bindings (26, all-scalar glue calls whose C side
checks ranges; its build prints the count).

| crate | attribute | blocks (src) | in-file tests / `tests/` | exports | what |
|---|---|---|---|---|---|
| `sai-ffi` | (workspace lints) | 82: generic 25, convert 20, tables 12, global 10, ptr 6, state 4, notify 4, diag 1; + test_port 3 (feature `test-port` only) | 83 / 273 | 17 `sai_*` (+ `sai_rs_test_query` with `test-port`) | the SAI C ABI; `convert` / `tables` blocks sit in macros |
| `sai-macsec-capi` | deny; allowed in ptr, attr, state, api | 60: attr 28 (union reads), api 20, ptr 6, state 6 | 0 / 35 | 18 | `sai_macsec_rs.h`, C++ callbacks |
| `sdk-glue` | none | 54: real 48, error 2, fake 2, link 2 (being reworked by the G2 backend work, not counted as G1's) | 0 / 1 | | one block per glue call |
| `phy-seq-capi` | deny; allowed in ptr, api | 28: api 24, ptr 4 | 10 / 26 | 7 | `phy_seq.h` |
| `tap-netdev` | deny; allowed in sys | 9 | | | ioctls (TUNSETIFF, TUNSETCARRIER, SIOCSIFHWADDR), if_nametoindex, socket, bind, setsockopt, poll |
| `sai-sys` (forbid), `sdk-glue-sys` (none) | | 0 | | generated `unsafe extern "C"` blocks; 26 `safe fn` (sdk-glue-sys) | declarations only |
| the other 14 | forbid | 0 | | | |

Total 233 blocks in five crates (+3 behind `test-port`); 194 on 2026-11-22. The delta: `sai-ffi` +14
(68 then): `convert` +7 (ACL field / action data, the three `MACsec` key members), `notify` +2
(the FDB event callback: the stored address as its function type, the call), `state` +2 (the
profile callbacks, split per call), `generic` +1 (`MACsec` decode), `global` +1
(`sai_dbg_generate_dump`'s path), `diag` +1 (`getsockopt(SO_PEERCRED)`: std's
`UnixStream::peer_cred` is unstable, `rustix` / `nix` are not dependencies); the `MACsec` API's
own 19 blocks (`sai-ffi/src/macsec.rs`) are gone: it uses the generic entry points, and only the
three key reads remain. `sdk-glue` +25 is the G2 rework in progress.

### Miri

`cargo miri test -p sai-ffi --lib`, `--test abi`, `--test global_a1`, `--test macsec_abi` (rustup
nightly 2026-11-21, `MIRIFLAGS=-Zmiri-disable-isolation`): 16 + 1 + 1 + 1 tests pass (2026-11-23),
including the lifecycle with the notifier thread and callbacks, and the `MACsec` API with key
material through the generic entry points. Under Miri the log does not open `/dev/log` (no AF_UNIX
there). Limit: callback pointers are kept in the safe core as addresses, so integer-to-pointer
casts remain ("might miss pointer bugs"). **Not run under Miri**: `tests/c_abi.rs` and
`exports.rs` (a C program, the built ELF), `tests/diag.rs` (unix sockets), `phy-seq-capi`,
`sai-macsec-capi`, `tap-netdev` (syscalls), `sdk-glue` (calls C). The pinned-nixpkgs toolchain has
no Miri.

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

In the C++, not fixed there (file:line of `ai_re/nix/packages/sai-openbcm/src`):

- **D1** `sai_macsec.cpp` finds an installed SA through `sc.hw` (set :241, used :960 / :977
  remove, :379 poll): after a flow moves to another port `remove_macsec_sa` runs nothing and
  returns success; **the key stays in the first PHY**. Fixed in the Rust model (test
  `where_a_flow_moved_the_cpp_leaves_the_key_in_the_phy`). **Q7** (a flow switch leaves the old
  SC's SAs installed) and **Q10** (a cipher change under waiting SAs installs the first 16 key
  bytes) are fixed as D9 / D10; Q1-Q19 with line numbers: `crates/sai-macsec/src/lib.rs`.
- Null method-table entries syncd may call unchecked: the bulk functions (`create_vlan_members`,
  `create_route_entries`, `create_lag_members`, ...), `switch_mdio_read`, `recv_hostif_packet`,
  and the user-defined trap's set / get / remove (`sai_hostif.cpp:518-539`). Rust: a function that
  answers `NOT_IMPLEMENTED` in every slot.
- `sai_api_query` of an API number out of range answers `NOT_IMPLEMENTED`, not
  `INVALID_PARAMETER` (`sai_api.cpp`).
- `create_switch` accepts an attribute given twice; `create_*` ignore the switch id (a port id as
  the switch creates a VLAN).
- Every get writes the attributes before the failing one; a failed `get_port_stats` writes zeros.
- A set of the create-only port `FULL_DUPLEX_MODE` is stored and answers `SUCCESS` (stub table,
  `sai_port.cpp:23`, `:275`); the read-only `LAG.PORT_LIST` answers `INVALID_ATTR_VALUE`
  (`sai_lag.cpp:187`); the create-and-set `BRIDGE_PORT.BRIDGE_ID` is refused as create-only
  (`sai_l2.cpp:512-514`).
- Sets that answer without the API lock or an object lookup: STP (`sai_l2.cpp:751`), next hop
  (`sai_l3.cpp:738`), host interface table entry (`sai_hostif.cpp:488`).
- A set on a `MACsec`-stage ACL table answers `INVALID_OBJECT_ID` (`sai_acl.cpp:640` knows only
  ASIC tables).
- `remove_route_entry` of a key whose address family is none answers `ITEM_NOT_FOUND`.
- A host interface's `VLAN_TAG` change passes -1 as the old filter (`sai_hostif.cpp:165`): the SDK
  glue never destroys the old RX filter.
- A trap's "full" programming is never cleared (`TrapObj::full`, `sai_hostif.cpp`).
- The diag socket file stays behind when its setup fails after `bind`; a client that never reads
  its answer holds the SAI lock, then `remove_switch`, for ever (no write timeout; `sai_diag.cpp`).

In Rust, fixed: `sdk-glue`'s `Claim::take` (eager `then_some`) let a refused second bring-up
release the live handle's claim; the first slice's MDIO label round trip; the removed
`sai-ffi/src/macsec.rs` decoded read-only `MACsec` attributes (list pointers included) before the
model refused them (now a placeholder, never read); the workspace build gave the installed
`libsai.so` sai-core's `testing` facade (`nix/package.nix` now builds without `sai-test-port`).

## Not done

- No Rust SAI frontend or backend on the ASIC; no boot-time PHY bring-up by the Rust MDIO tool.
- Hardware backend (gate G2): ports, VLANs, link events, board quirks; the other 50 required
  typed operations answer `NotSupported` (`sai-backend-openbcm/src/g2.rs`), the optional ones by
  default (MDIO, QoS, mirror, sampling, hash, storm control, KNET, field processor, L3, ...).
- Frontend: one switch, no breakout; 42 attributes of the headers have a member the rim does not
  decode (answered "not supported"); the capability table is gated by `Backend::programs`. Not
  ported because the C++ has none: bulk, warm boot, STP ports, tunnels, packet I/O.
- No MACsec VM test on a libsai: `vm-macsec.nix` runs SONiC's own virtual SAI; the MACsec path of
  this library is covered by `test_sai`, the differential and the model's tests.
- The openbcm backend applies the board quirks in its own bring-up too (first slice); the core
  applies them as the C++ frontend does, so the chip gets them twice (G2 drops the backend's).
- No fuzz target.
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

The parity harnesses by hand (from `rust/sai`, in that environment; the C++ sources are read from
`ai_re/nix/packages/sai-openbcm`):

```sh
SAI_INC=... bash tests/test-port/run.sh --strict     # test_sai on this library + the C++ reference
SAI_OPENBCM_SRC=../../ai_re/nix/packages/sai-openbcm/src SAI_OPENBCM_TESTS=../../ai_re/nix/packages/sai-openbcm/tests \
  cargo test -p sai-driver --test differential -- --nocapture
bash tests/unsafe-count.sh                           # the unsafe inventory ([Unsafe](#unsafe))
```

The socket tests need a short `SAI_RS_TEST_TMPDIR` (unix socket paths have 107 bytes).

### Updating the numbers in this file

Unsafe counts: the method under [Unsafe](#unsafe). Test, suite and comparison counts: the end
of `run.sh`'s output (the Nix checks print the same lines). Change a number with its date.

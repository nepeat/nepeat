# sai-rs: the SAI and its switch tools in Rust

The SAI (OCP SAI 1.19.2, the headers sonic-sairedis is pinned to) and the switch tools of the AS7760 and
AS7772, in Rust. Since 2026-11-23 (gates G5 / G6) it is what those boards build and run: the C++
`ai_re/nix/packages/sai-openbcm` it replaced is deleted except the C SDK glue (`sdk_glue.c`); its C++
frontend stays, frozen, in `ai_re/nix/packages/sai-teralynx/frontend` for the CS8210 until that board is
proven on Rust.

Status, stated plainly:

- **On hardware (AS7772, 2026-11-23, G2-G4):** syncd on this `libsai.so` drove the ASIC under SONiC: 32
  ports + KNET netdevs, LLDP, temperatures, the external-PHY link gate, MACsec (up-check, traffic, rekey,
  wrong CAK, PN wrap; bring-up bus trace equal to the C++), stack restart, `kill -9`, reboot, cold boot,
  rollback; the Rust MDIO tool did the boot-time PHY bring-up (one reboot, two cold boots, 32/32 PHYs).
  Evidence: `ai_re/devices/edge-core-as7772-32x/evidence/rust-g2/README.md`. Not run there: `loadtest/ring.sh`,
  a soak.
- **AS7760: never on silicon with this library** (off the bench; the owner counts the AS7772 proof for it:
  same SDK and glue). Offline only: its system builds, the fake-backend SONiC VM tests (LACP PortChannels,
  VRF / ECMP, ingress ACL + counters, CoPP); never run on its chip: the 4x25G break-out, the AS7712 LED
  program and port-order remap, the serdes pre-emphasis keys, its config.bcm under this library.
- The userspace test bundle on the AS7772 (big-endian ppc64, [Big-endian runs](#big-endian-runs)): 55 of
  55 suites, 291 Rust tests, C harness 371 checks (2026-11-23, G0, before G1-G6); no endianness defect.
- Parity with the C++ ([Parity](#parity)): live against the frozen C++ frontend where it still exists
  (`test_sai` 3014 / 3014 through the test port, the op-script differential, the MACsec differentials, the
  `openbcm-diag` client); golden files captured at commit 50a6aa7 from the deleted C / C++ (the MDIO
  interpreter and tool, `openbcm-asic-temp`, the table checkers, the glue-call differential, the error table).

## Migration status

"Deleted" = the C/C++ is gone from `sai-openbcm` (its lines: the G6 report); "frozen" = still compiled by
sai-teralynx (`frontend/`), not by any AS77xx build. "On chip" = the AS7772 (G2-G4).

| component | C/C++ | Rust (crate) | parity evidence | on chip | selected by |
|---|---|---|---|---|---|
| SAI frontend | `sai_*.cpp`: deleted 2026-11-23, frozen for the CS8210 | `sai-ffi` (rim) + `sai-core` (+ `sai-meta`, `sai-sys`): switch, port, L2, LAG, hostif, L3, ACL / policer / CoPP, QoS, mirror / samplepacket, hash, MACsec objects, capabilities, dump, log, temperatures; refusals answered as each `sai_*.cpp` | `test_sai` 3014 / 3014 through the test port (live); differential 455 / 455 ops (live); C harness 374 checks; SONiC VM tests | AS7772 G4 | default (`hardware.openbcm.package`) |
| ... not in the C++ either | | bulk operations, warm boot, STP ports, tunnels, packet I/O through the SAI | | | |
| virtual backend + netdevs | `fake_backend.cpp`: deleted, frozen | `sai-core::virt` + `tap-netdev` | the suites above; `vm-sai-rs-privileged` | never (refuses next to a chip) | `SAI_OPENBCM_BACKEND=fake` (or `SAI_RS_BACKEND=virtual`) |
| hardware backend | `openbcm_backend.cpp`: deleted 2026-11-23 | `sai-backend-openbcm` over `sdk-glue` / `sdk-glue-sys`: every operation of `openbcm_backend.cpp`; what it lacks stays `NotSupported` | glue-call golden 469 / 482 ops identical, 1008 / 1008 glue lines matched (1 justified entry); error-table golden | AS7772 G2 smoke + G4 | default; `SAI_OPENBCM_BACKEND=openbcm` |
| SDK glue | `sdk_glue.c`: **stays C** | bound by `sdk-glue-sys`, wrapped by `sdk-glue`; in-memory twin `sdk_glue::rust_fake` | declarations checked against `sdk_glue.h` by the C compiler (x86, ppc64) | AS7772 G2-G4 | feature `real`; `rust-fake` for tests |
| MDIO script interpreter | `mdio_script.c`: deleted 2026-11-23 | `mdio-script` | golden: 5053 corpus + 2200 generated comparisons | AS7772 G3 | inside `mdio-tool` |
| MDIO report + boot-time tool | `mdio_report.c`, `mdio_script_main.c`: deleted 2026-11-23 | `mdio-report`, `mdio-tool` (`openbcm-mdio-script`) | golden: 159 runs; the tests again through `sdk-glue`'s FFI module on `tests/c/mdio_stub.c`; `vm-openbcm-mdio-units` | AS7772 G3 (boot unit) | default |
| external PHY / MACsec table parser + executor | `sai_extphy.cpp`, `sai_extmacsec.cpp`: deleted, frozen; checkers `*_check.cpp` deleted | `phy-seq` | golden: 683 corpus + 42,770 generated runs + the AS7772 tables (631 + 1); `test_sai` (live) | AS7772 G4 | default |
| external PHY runtime | `sai_extphy.cpp`: deleted, frozen | `sai-extphy` + `sai-core::extphy` | `test_sai` `test_ext_phy`; differential `10-ext-phy` (live) | AS7772 G4 | `SAI_OPENBCM_EXT_PHY_FILE` |
| MACsec object model | `sai_macsec.cpp`: deleted, frozen; `sai_macsec_rust.cpp` deleted | `sai-macsec` + `sai-macsec-phy`, natively behind `sai-core::macsec` | `test_sai` `test_macsec`; differential `11-macsec`; model differential + e2e vs `sai_macsec.cpp` (live) | AS7772 G4 | `SAI_OPENBCM_EXT_MACSEC_FILE` |
| diag socket | `sai_diag.cpp`, `openbcm_diag.c`: deleted, frozen | `sai-diag` (server) + `openbcm-diag` (client) | `test_sai` `test_diag`; client differential vs `openbcm_diag.c` (live) | AS7772 G4 | `SAI_OPENBCM_DIAG_SOCKET` |
| temperatures | `sai_temp.cpp`, `temp_files.c`: deleted, frozen | `sai-core::temp`; `sdk-glue::temp` | `test_sai` `test_temperature` | AS7772 G4 | `SAI_OPENBCM_TEMP_FILE`, `_PROM`, `_INTERVAL` |
| attach-only temperature reader | `asic_temp.c`: deleted 2026-11-23 | `openbcm-asic-temp` | golden: 30 cases; golden tests | in the AS7772 G4 system (the unit runs while syncd is stopped) | default |
| the package | `ai_re/nix/packages/sai-openbcm` (now the glue + README) | `nix/sai-openbcm-rs.nix`, the names the C++ package had ([Drop-in package](#drop-in-package)) | builds native and ppc64 (libsdk); link checks | AS7772 | `hardware.openbcm.package` (default; `hardware.openbcm.rustSai`) |
| C++-only plugs | `phy-seq-capi`, `sai-macsec-capi` (Rust for the C++): deleted 2026-11-23 | | | (G0) | |

## Plan: replacing the C++ (accepted 2026-11-23)

Goal: our own C/C++ SAI (`ai_re/nix/packages/sai-openbcm/src`, later `sai-teralynx`) is replaced by this
workspace; what stays C is the boundary to third parties (`sdk_glue.c` for the OpenBCM SDK, the Teralynx shim).
The C++ stays the boot default and selectable until a board is proven on the Rust path on hardware (done for
the AS7772 / AS7760: G5, G6; the CS8210 is next).

**Target architecture.** `sai-ffi` (rim) over `sai-core`: the model plus the frontend semantics of
`sai_*.cpp`, one module per area (switch, port, l2, lag, l3, hostif, acl / copp, qos, mirror, hash, caps,
dump), over typed backend sub-traits that mirror the C++ `ob::Backend` op for op (121 ops; optional groups
default to "not supported", with the same capability probes). Backends: `virt` (parity with
`fake_backend.cpp`, records a call log), `sai-backend-openbcm` (the 72 ops of `openbcm_backend.cpp` over
`sdk_glue.h`, 75 functions), later `sai-backend-teralynx`. Beside the core, all `forbid(unsafe_code)`: the
external-PHY runtime (link gate, debounce, per-PHY MDIO arbitration, admin sequences) over `phy-seq`, `sai-macsec` called
natively, the diag socket, temperatures. (`phy-seq-capi` and `sai-macsec-capi` plugged Rust into the C++ for G0
and were deleted with it.) The library reads the C++ profile key names (`SAI_OPENBCM_*`).

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
| G2 | hardware backend covers `openbcm_backend.cpp`; smoke on the AS7772 with SONiC stopped | offline part green 2026-11-23 (x86_64): every `openbcm_backend.cpp` operation ported, status table equal to `map_rv` (19 codes, no difference); glue-call differential 469 / 482 ops identical, 1008 / 1008 Rust glue calls matched in order, allow-list 1 entry (the C++ frontend's second `ob_sdk_cpu_port`); `fake_glue.c` deleted (consumers on the Rust fake; the MDIO tool's 159 differential runs on a C stub); order enums in `sdk_glue.h` (`test_sai` 3014, `test_units` 140, `libsai.so` loaded sections byte-identical); `openbcm-asic-temp` in Rust (30 differential cases); smoke program extended to the G2 gate (23 steps, passes on the Rust fake); drop-in `nix/sai-openbcm-rs.nix` (native + ppc64); `cargo test --workspace` 560 / 0. **Open: the smoke on the AS7772** (`--no-ext-phy` first, then with the PHYs)  **On the AS7772 2026-11-23:** smoke with SONiC stopped, without and with the PHY tables: 21 PASS, 1 SKIP, 1 FAIL in both runs; the FAIL was the smoke's own temperature check (it compared two separate reads of a moving monitor; fixed to allow one degree, not re-run). Evidence: `ai_re/devices/edge-core-as7772-32x/evidence/rust-g2/README.md`. |
| G3 | no C++ shims: Rust MDIO tool as the boot unit, ext-PHY runtime + MACsec native in `libsai.so` | on the AS7772 2026-11-23: the Rust MDIO tool did the boot-time PHY bring-up on one reboot and two cold boots (32/32 PHYs, 0 values differed, 100.5-101.3 s); ext-PHY runtime + MACsec native: see G4. The C tools and the capi crates: deleted in G6 |
| G4 | syncd on the Rust library on the AS7772 (opt-in): full feature set, device scripts, reboot, cold boot, rollback | first session passed 2026-11-23 (`as7772-rust`): 32 ports + KNET netdevs, LLDP, temperatures, link gate, MACsec up-check / traffic / rekey / wrong CAK / PN wrap, MACsec bring-up bus trace equal to the C++ (80 runs / 22,560 operations), stack restart, `kill -9`, reboot, cold boot, rollback and forward. Not run: `loadtest/ring.sh`, a soak. The box was left on the Rust library |
| G5 | Rust the default on the AS7772 as soon as G4 passes | done 2026-11-23 (offline): `hardware.openbcm` has no implementation switch any more; the flake configs `as7772` / `as7760` and every image / netboot variant build `nix/sai-openbcm-rs.nix` (with `hardware.openbcm.rustSai`, set by `ai_re/nix/configs/default.nix` in the root git flake); the `*-rust` configurations folded into them. Not yet deployed as the default generation by this gate (the box was left on the Rust library by G4) |
| G6 | delete the sai-openbcm C++ except `sdk_glue.c`; the frozen C++ frontend moves into sai-teralynx | done 2026-11-23 (offline): sai-openbcm keeps `src/sdk_glue.{c,h}` and its README; the frontend is in `sai-teralynx/frontend` (its `libsai.so` byte-identical to before the move, test_sai 3012, test_units 140, test_ifcs 1047); `phy-seq-capi`, `sai-macsec-capi` deleted; the comparisons with deleted C / C++ became goldens of 50a6aa7 ([Parity](#parity)) |

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
it needs from ai_re (SAI headers, the SDK glue, the frozen C++ frontend, libsdk) as arguments, said in its
header; `ai_re/tests/nixtargets.py` wires them up ([Build and test](#build-and-test)).

## Crates

```
 syncd
   | SAI C ABI (sai.h)
 sai-ffi       libsai.so: the rim
   |   (+ sai-test-port: test-only ABI)
 sai-core      model, areas, store        phy-seq  <---- sai-macsec-phy ---- sai-macsec  (MACsec model)
   | Backend   caps, events, refusals       | Dev, MdioBus       ^ (natively, sai-core::macsec)
   | (16 typed sub-traits)                sai-extphy (ext-PHY runtime)   sai-diag (diag socket server)
   +-- virt (module) -- tap-netdev        mdio-script
   +-- sai-backend-openbcm -- sdk-glue -- sdk-glue-sys --> sdk_glue.c (C) + libsdk
 sai-meta, sai-sys   attribute tables, bindgen of the SAI headers (under everything that sees SAI types)

 openbcm-mdio-script-rs: mdio-tool -- mdio-report -- mdio-script;  feature sdk: -- sdk-glue
 openbcm-diag: the diag socket client (openbcm_diag.c);  sai-driver: op-script driver (differential)
 openbcm-asic-temp: attach-only temperature reader (asic_temp.c) -- sdk-glue (Attached) + sai-core::temp
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
`a_panic_under_the_lock_fails_every_call_until_uninitialize`). Needs `panic = "unwind"` (set in the workspace profiles); with
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

- `sdk-glue-sys`: bindgen over `sdk_glue.h` (75 functions, five structs, three callback types, the
  `OB_*` constants), checked against the header by the target's C compiler (246 declarations);
  26 `safe fn` by the owner's rule. The order contracts of the glue's int arguments (`PortStat`,
  `TrapKind`, `Fec`, `Loopback`, `Learn`) are C enums of `sdk_glue.h` since G2 (`ob_port_stat_t`,
  ...; `backend.h`'s enum classes `static_assert` against them, `sdk_glue.c` checks its counter
  and trap tables against the `_COUNT`s), generated into `sdk_glue_sys::order`. `sdk_glue.c`
  stays C. Feature `real` links it with libsdk (`SDK_GLUE_LINK`); nothing else is linked (the C
  fake `fake_glue.c` and the feature `fake` are gone).
- `sdk-glue`: the only caller. The glue's rules (no MDIO before misc init, one bring-up per
  process, BCM calls only after "init all") are types (`Attached`, `Sdk`, `Switch`): table in its
  `lib.rs`. Cloneable `MdioBus` / `TempReader` handles, callback trampolines over a keyed
  registry (no memory handed to C). `rust_fake` (feature `rust-fake`) is the in-memory chip every
  test uses, with a call recorder (`record.rs`: one canonical line per glue call, arguments and
  results, the callbacks it fires).
- `sai-backend-openbcm`: every operation `openbcm_backend.cpp` implements, with the same glue
  calls in the same order and arguments: init (config checks, `ob_sdk_init` = "init all", the
  LED rc via `rcload`, stale KNET clean-up, RX, linkscan inside the glue; one
  `ob_sdk_register_callbacks` for link + L2; the CPU and drop egress objects created and cached;
  `ob_sdk_chip_limits` logged), ports (queried live, as the C++), VLAN, trunks, L2 + learn / age
  events onto the `EventSink`, KNET netifs / filters, the carrier through `/proc/bcm/knet/link`,
  `trap_set`, L3 (refusing to destroy the cached egress objects, the route drop flag), ECMP, field
  groups / entries / counters with the C++ policer-clone fallback and bookkeeping, policers, port
  counters, table usage, chip limits, temperatures + `temperature_reader`, shell (+ to an fd),
  clause-45 MDIO + `mdio_bus`, per-port link gates. Ports are disabled and the board quirks applied
  by the core (`sai-core::switch`), as the C++ frontend does; the backend no longer does either.
  Status table: exactly `map_rv` (`tests/parity.rs`, against the golden of the C++ table: 19 codes, no
  difference). What `openbcm_backend.cpp` does not implement (QoS, mirror, sampling, genetlink,
  hash, storm / flood, `trap_program`, the L3 / FP extras, counters by id, PFC) answers
  `NotSupported` as there. **Differences from the C++** (the crate's `lib.rs` lists them all): a
  failed init after `ob_sdk_init` calls `ob_sdk_shutdown` (C++: leaves the SDK up); a link gate
  still registered at shutdown is removed before `ob_sdk_shutdown`; before init / after shutdown
  every operation is `UNINITIALIZED` without a glue call (C++: calls the glue, which answers
  `OB_E_INIT`), and the field / policer / egress state goes with the shutdown; a second init while
  up is refused; `sdk-glue` refuses before the glue what C would truncate or cast (numbers above
  `i32::MAX`, netif names over 15 bytes, shell commands over 1023, NUL bytes, ECMP groups of 0 or
  more than 256, trunks over 64) and drops L2 events with arguments out of range;
  `SAI_OPENBCM_UNIT` parsed strictly; stderr, not syslog, and `OB_E_UNAVAIL` not logged.
- Glue-call golden (`tests/glue_differential.rs`, goldens in `tests/golden/`): the 9 op scripts of the
  C++ frontend (`sai-teralynx/frontend/tests/driver-scripts`), sai-driver's two (external PHY, MACsec)
  and `tests/glue-scripts/12-glue-extras.ops` (board quirks, a neighbour MAC change) run through
  sai-driver's `Driver` on this backend over a recording Rust fake; the output is compared op block by op
  block with the golden of the C++ side: the C++ `frontend_driver.cpp` and library on the
  **unmodified** `openbcm_backend.cpp` over a stub of all 75 glue functions that answered by replaying
  the Rust run's recording, captured at commit 50a6aa7 (both C++ and stub deleted since; the golden
  holds the script texts and table files it was made from, and a changed script fails as "changed since
  the golden"). Allow-list `tests/glue-differential-allow.txt` (1 entry: the C++ frontend's second
  `ob_sdk_cpu_port`, `sai_switch.cpp:240` / `:243`); every Rust glue line must also equal a C++ line of
  its op, in order (also inside allowed ops). 2026-11-23: 12 scripts, 482 ops, 469 identical, 1008 /
  1008 glue lines matched; negative controls (a changed Rust glue line, a changed golden byte, a changed
  script) fail. What it proves: given the SDK answers the Rust fake gave at capture, the Rust stack
  makes the calls the C++ made. Never exercised: `attach`, `th_overtemp_shutdown`, `init_mdio`,
  `shell`, `shell_to`, `errmsg`, `hw_info`, `port_stat_clear` (12-glue-extras does not reach it),
  `knet_cleanup`, `unregister_callbacks` (65 of 75 functions are). `tests/parity.rs`: the status table
  against the golden of the C++ `map_rv` (19 codes, no difference). Both run in every workspace test run
  (also on the switch) and in `sai-rs-parity`.
- `real.rs` (the FFI calls): off-target through `sdk-glue/tests/real-stub.sh` (feature `real` linked
  against the C stub `sdk-glue/tests/c/glue_stub.c` + `real_stub_threads.c` instead of `sdk_glue.c` +
  libsdk: every wrapper, callbacks from C threads), run by `nix/package.nix`'s check as its own cargo
  invocation and again under valgrind's memcheck; also mdio-tool's tests with `--features sdk-real`
  against `tests/c/mdio_stub.c` (`nix/mdio-tool.nix`). Not in the ppc64 bundle (the test re-executes
  itself through `current_exe`, which is the loader when `run.sh` starts suites through it).
- The smoke program (`smoke/sai_smoke.c`, `bin/sai-openbcm-smoke`): the G2 hardware gate, the SAI
  C API only. Refuses without `--sonic-is-stopped` (it resets the chip) or while the SDK lock is
  held; `--dry` lists the 23 steps: load, profile (served through the SAI service table;
  `--no-ext-phy` drops the ext-PHY / MACsec tables), create-switch, ports (count, lanes, speed),
  admin-up of the `--cabled` lanes, oper-up through the ext-PHY gate, VLAN, FDB, LAG, hostif,
  hostif-netdev (`/sys/class/net`, carrier following oper status; `--no-kernel` = SKIP, only for
  the fake), L3 (RIF, neighbour, next hop, route), ACL (table, entry, counter read), policer, port
  counters, temperatures, a diag command through the socket, one read-only MDIO read through the
  socket (`phy raw c45 <phy> 0x1 <reg>`; the program itself never writes MDIO), teardown,
  admin-down, remove-switch, `--recreate` (a second "init all" in one process, default off,
  untested on hardware), uninitialize. PASS / FAIL / SKIP per step, exit 1 on any FAIL. On the Rust
  fake (`tests/smoke.rs`, x86 only: the ppc64 bundle has no `openbcm-fake` library, so there only
  the refusal / `--dry` test runs): the full run, `--recreate`, an ext-PHY run on a test table
  (only that table's writes on the bus, ordered around port enable), `--no-ext-phy` (no bus access
  but the smoke's read), failure paths. Commands for the AS7772 and what only hardware can show:
  [Hardware runbooks](#hardware-runbooks).
- `openbcm-asic-temp` (crate of that name): `asic_temp.c` in Rust, same command line, messages,
  exit codes (4 after 3 bad reads, 5 after the core-PLL power-down at >= 125 C on 2 reads), files
  (`sai-core::temp`'s writer). Golden (`tests/golden/differential.txt`, 50a6aa7): the C tool (the
  unmodified, since deleted `asic_temp.c` + `temp_files.c` on a C stub of the glue answering as the Rust
  fake) in 30 cases: the Rust binary on the Rust fake gives the same exit code, stdout, stderr, files. Differences: a failure to install a signal handler is said; SIGPIPE ignored;
  `POSIXLY_CORRECT` ignored.
- `nix/libsai-openbcm.nix`: the library with this backend plus the smoke program. Real glue
  (ppc64): `-z defs`, soname `libsai.so.1`, the same 17 exports as the virtual build; on the AS7772 since G2.

### Drop-in package

`nix/sai-openbcm-rs.nix`: `{ saiSrc, glueSrc, libsdk ? null, saiRs ? <package.nix> }`, the SAI package of
the AS7760 / AS7772 (`hardware.openbcm.package`) under the names the C++ `sai-openbcm` package had:
`lib/libsai.so` (`.so.1`, `.so.1.0.0`; ppc64 with libsdk: `openbcm-real`, NEEDED libsdk, `-z defs`, 17
exports; native: virtual backend + `openbcm-fake`), `bin/openbcm-diag`, `bin/openbcm-mdio-script`
(`sdk-real` with libsdk), `bin/openbcm-ext-phy`, `bin/openbcm-ext-macsec` (phy-seq's checkers; the boards
check their tables with the native build, `hardware.openbcm.nativePackage`), `bin/openbcm-asic-temp` (with
libsdk), `bin/sai-openbcm-smoke`, `share/sai-openbcm/backends` (`fake openbcm` / `fake`),
`share/sai-openbcm/phy-seq-engine` and `macsec-model` (`rust`). passthru: `libsdk`, `saiHeaders`, `tools`,
`parts`. pname `sai-openbcm-rs` with libsdk (unfree), `sai-openbcm-rs-fake` without. Targets:
`sai-openbcm-rs`, `as7760-sai-openbcm-rs`, the systems (`ai_re/tests/nixtargets.py`).

### mdio-script

Port of `mdio_script.{c,h}`: `Script::parse` gives typed operations, `Script::run` executes
them over `trait MdioBus`; clause 22 cannot be expressed (`Dev` is 1..31) and a `Script`
proves the whole file parsed. Differences from C: crate docs. Golden of the deleted C
interpreter (`tests/golden/`, 50a6aa7; `tests/differential.rs`): return code, statistics,
messages, failure record, bus trace of 85 corpus scripts (5053 comparisons) and 2200 generated
ones (fixed seed; inputs and outputs as FNV-1a digests); the live run before the capture agreed on all 7253.

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

The golden of the deleted C tool (`tests/differential.rs`, `tests/golden/differential.txt`, 50a6aa7; 159
runs: 40 through a diag socket, 44 on a fake chip, 75 command lines): the C tool on
`tests/c/mdio_stub.c` (the MDIO side of the glue, reading the same scenario directives as the Rust fake)
against the Rust tool on the Rust fake: exit code, stdout, stderr, status files, bus trace. `mdio_stub.c`
stays: `nix/mdio-tool.nix` runs the tool's tests a second time with
`--features sdk-real` linked against that stub, which executes `sdk-glue`'s FFI module for MDIO and
temperatures.

### phy-seq

`phy-seq`: the external PHY tables (`ext-phy 1`, `ext-macsec 1`): parser, typed
parameters (wiped on drop, `zeroize`), executor over a bus trait, and the checkers
`openbcm-ext-macsec-rs` / `openbcm-ext-phy-rs`. Differences from the C++ engines (`u16`
values, no partial results after a failure, ...): its `lib.rs`. The runtime around them (link
gate, debounce, counters, locks) is `sai-extphy`. Golden of the deleted C++ checkers
(`tests/differential.rs`, `tests/golden/`, 50a6aa7): `--check` and `--render` (no params, params,
params + recorded reads) of 683 corpus runs in full, 4000 + 16,000 generated tables (fixed seeds, 42,770
runs as digests), and the AS7772 tables (631 + 1 runs) when `PHY_SEQ_TABLES` names files with the
captured contents (else SKIPPED).

### sai-macsec, sai-macsec-phy

- `sai-macsec`: the object model of `sai_macsec.cpp` (MACSEC, MACSEC_PORT, FLOW, SC, SA, the
  MACsec-stage ACL glue) over a `SeqRunner` trait. Its `lib.rs` lists the intentional
  differences **D1-D10** and the questionable C++ behaviours kept, **Q1-Q19**.
- `sai-macsec-phy`: `SeqRunner` over `phy-seq`; with the above, the whole Rust path from a
  SAI object to MDIO words (`tests/e2e.rs` holds it to the C++ path access by access).
- In the Rust `libsai.so` the model runs natively (`sai-core::macsec`, no C between): the five
  `MACSEC` API types and the `MACsec`-stage ACL tables / entries go to it through the generic
  entry points. Three C++ behaviours are fixed in Rust (owner's decision 2026-11-23, each with a
  test): **D1** (an SA of a flow that moved stays in the first PHY), **D9 = Q7** (a flow switch
  leaves the old SC's SAs installed), **D10 = Q10** (a cipher change under waiting SAs installs
  the first 16 key bytes); every other Q item keeps the C++ behaviour. The differentials (live:
  `sai_macsec.cpp` and the C++ library around it are the frozen frontend's, which the CS8210's libsai
  compiles; `tests/cpp/driver.cpp` is the test's own driver) hold the
  Rust to the C++ with exactly the D9 / D10 edits (`sai-macsec/tests/cpp/listed_fixes.rs`; prebuilt
  as `sai-macsec-cpp[-chip]-driver-fixed` for the bundle): equal to it on every script, so no other
  difference passes and neither does a missing D9 / D10. Random scripts are deterministic per seed
  (`SAI_MACSEC_SEED`, default random and printed; `SAI_MACSEC_DIFF_CASES`, `SAI_MACSEC_E2E_CASES`).

### sai-extphy, sai-diag, openbcm-diag

- `sai-extphy`: the runtime of `sai_extphy.cpp` around `phy-seq`'s parser and executor: the link
  gate the backend's link scan asks, debounce (`up-polls`), the admin sequences, counters and
  status text, and the **MDIO arbitration** of every library user of the PHYs plus the diag
  socket's **transactions** (below). Never the SAI lock. The SAI layer holds it per switch
  (`sai-core::extphy`).
- `sai-diag`: the diag socket server of `sai_diag.cpp` (one thread, one connection at a time,
  root only, the command under the SAI lock, output written after it is released; differences
  in its `lib.rs`), except raw MDIO and `extphy txn`, which the switch's `sai_core::diag::DiagPort`
  serves **without** the SAI lock. `openbcm-diag`: the client `openbcm_diag.c`, same command line,
  bytes and exit codes (differential against the C tool).

#### MDIO serialisation and diag transactions (2026-11-23, replaces the hold-off of 9d64222)

The Evora PHYs' firmware-message and MACsec register interfaces are conversations (request words,
doorbell 1.8222, ack 1.8221 that clears on read, result words): a foreign access in the middle
breaks both parties (the AS7772's `pn.* stopped ... 1.8221 read 0x0000` and the tool's "FW Message
failure", `ai_re/devices/edge-core-as7772-32x/evidence/rust-g6/` f1, g2).

- **What the C++ did:** one global mutex for every sequence run (link poll, admin, MACsec); the gate
  took it blocking on the SDK link-scan thread; `phy raw` of the diag socket ran under it *and* the
  SAI lock; a global `holdoff-ms` hold-off after each `phy raw` paused the **link polls only**
  (`sai_extphy.cpp:506`); MACsec sequences never looked at it (`sai_macsec.cpp:153`).
- **Now:** a **slot per PHY** (MDIO address word): every user holds the PHYs its sequence addresses
  for the whole run; different PHYs interleave (the SDK serialises single frames). A global slot only
  for raw diag MDIO that names no single PHY (broadcast bit, clause 22, `sbus`, unparsed).
- **Transactions** (diag socket, Rust only): `extphy txn begin <phy[,phy...]|all> [ms]` ->
  `txn <n> begun: ...`; `extphy txn <n> phy raw c45 ...` (each access renews it); `extphy txn renew
  <n> [ms]`; `extphy txn end <n>`; `extphy txn [list]`. A lease: `LEASE_DEFAULT_MS` 1000 after each
  access unless asked, never more than `LEASE_MAX_MS` 2000 from its begin (a dead client blocks
  nothing longer). A request of a transaction that ran out is refused (`openbcm-diag: extphy txn <n>:
  expired ...: what the PHY did since is unknown`). A bare `phy raw` inside another client's
  transaction is refused; without one it gets an **implicit lease** of `holdoff-ms` on its PHY (the
  old hold-off, per PHY now) so today's tools keep working.
- **Who waits for what** while a lease holds a PHY: the link gate never waits (cached verdict,
  counted) but waits behind short library runs (<= ~18 ms) rather than skipping a pass; MACsec periodic
  reads (`pn.*`, `stats-sa.*`, `stats-flow.*` of a get) skip (the model's last read answers, counted,
  no error line), and a read that has not reached its PHY for `MACSEC_POLL_DEFER_MAX_MS` 10 s waits
  for the transaction to end (<= 2 s) and reads, never inside one; SONiC's MACsec changes (SA
  install / remove, gate, enable / disable, clears) and admin sequences wait for a **transaction**
  (<= 2 s, under the SAI lock), never dropped, and pass implicit leases (as in the C++); a diag
  `extmacsec run` is refused inside a transaction. A new transaction waits for library runs on its
  PHYs and for library users already waiting, so back-to-back transactions cannot starve them.
- **Lock order**, by construction: SAI lock, then a lease, then a PHY slot, then the MDIO master's
  own lock. The runtime's mutexes are leaves (`sai-extphy` `leaf`: debug builds assert no nesting,
  no logger and no bus access under them); slots are never held while anything waits; the gate
  closure and the `DiagPort` have no way to take the SAI lock; the diag server serves raw clause-45
  MDIO and transactions over the backend's MDIO handle (answer format the SDK shell's `\t0x%02x:
  0x%04x`) when a PHY table is attached and the backend's shell would answer it from that bus
  (`Backend::diag_mdio_native`; the virtual chip only in `ShellMdio::Answer`); everything else still
  goes through the SDK shell under the SAI lock, within the arbitration.
- **Visibility**: the `extphy` text keeps the C++ header (`PAUSED now` = some lease live, `N so far` =
  leases begun) and table, then four lines: transactions begun / ended / expired, hold-offs, busy /
  void refusals; link polls answered from a lease and gate waits (count, max, total us, measured from
  before the wait); MACsec polls skipped, forced reads (max wait), runs that waited for a transaction,
  `extmacsec run` refused; then one line per live lease. Log: a NOTICE per implicit lease begun
  (`external PHY phy 0x10: link polls and MACsec polls paused: raw MDIO on the diag socket without a
  transaction ...`), per expiry, per forced read, per MACsec run that waited; INFO per transaction
  begun / ended.
- **Not solved, documented:** MACsec counters clear on read; a diag read of them (transaction or not)
  takes those counts from SONiC's statistics. A client that dies in the middle of a conversation leaves
  the PHY's message interface in an unknown state for the next user after its lease runs out.
- Tools: `evora-rehost/mdio.py` (`Diag.transaction()`) and `phystat.py` (one transaction per register
  read, `--txn phy|all|off`). Tests: `sai-extphy/tests/arbitration.rs` (a stateful fake of the
  message interface, `sai_extphy::fwphy`, that breaks a conversation on any foreign access, as the
  hardware does), `sai-core/tests/macsec.rs` (through the SAI layer and the socket, real threads).

### sai-test-port, sai-driver (the parity harnesses)

- `sai-test-port`: the answering side of `sai_rs_test_query`, the one test-only export of a
  `libsai.so` built with `sai-ffi`'s feature `test-port` (never installed: `nix/package.nix`
  builds without it). Over it `tests/test-port/test_port_rust.cpp` implements the frozen frontend's
  `tests/test_port.h`, so the unmodified `test_sai.cpp` runs on this library
  (`tests/test-port/run.sh`).
- `sai-driver`: the Rust half of the op-script differential (grammar and output of the C++
  `frontend_driver`, profile identical): the frozen frontend's `tests/driver-scripts/*.ops` and this
  crate's own (`tests/driver-scripts`: external PHY, MACsec with key material, on the tables of
  `tests/test_tables.h`) through both drivers; every differing operation must be in
  `tests/differential-allow.txt` (empty).

## Wiring into the C++ libsai (G0, deleted)

For G0 the C++ libsai could take the Rust table engine (`make RUST_PHY_SEQ`, crate `phy-seq-capi`,
`hardware.openbcm.rustPhySeq`) and the Rust MACsec model (`make RUST_MACSEC`, `sai_macsec_rust.cpp` +
crate `sai-macsec-capi`, `hardware.openbcm.rustMacsec`); with both, the AS7772 ran MACsec under SONiC on
them. All of it was deleted on 2026-11-23 with the C++ (the options are removed with a message). The trace
hook it brought (`openbcm-diag "extmacsec trace [on [all] | off | dump | clear]"`: the bus operations of
sequence runs, key words `****`, last 50,000 lines, off by default) is the Rust library's too
(`sai-core::macsec`, tests in `sai-core/tests/macsec.rs`, `diag.rs`); G4 compared the bring-up traces with it.

## Big-endian runs

`nix/target-tests.nix`: every test executable of the workspace built for the target (test
profile; the goldens are compiled into them), the debug and release `libsai.so`, the C harness, the
helpers crates prebuilt (the frozen C++ frontend's drivers and `openbcm_diag.c` for the live comparisons,
the smoke program), the workspace's binaries, the AS7772 MACsec and PHY tables, a sysroot (loader, libc,
libgcc_s, libstdc++). `nix/target-tests-run.sh` runs it: one line per suite, the harness on
the release library, what the suites report. From `ai_re/`: `just sai-rs-test-on
root@<host> sudo ssh` (a `mktemp` dir under `/tmp`, removed afterwards).

Variables the tests honour (`run.sh` sets them): `SAI_RS_TEST_TMPDIR` (scratch instead of the
build tree's temp dir), `SAI_RS_TEST_BIN` (prebuilt helpers), `SAI_RS_TEST_SUMMARY` (one line
per test about what it compared or skipped), `SAI_RS_VIRTUAL_ON_HARDWARE=1`, `SAI_FFI_HARNESS`
/ `SAI_FFI_LIBSAI`, `SAI_MACSEC_E2E_TABLE`, `PHY_SEQ_TABLES`.

Rule for new tests: no compiler at run time. A C / C++ helper is prebuilt by the crate's
`build.rs` into `OUT_DIR/target-tests` and found through `SAI_RS_TEST_BIN`; it must be
harmless when started without arguments (`run.sh` probes each so). A passing test's stderr
is captured, so a skip must also go to `SAI_RS_TEST_SUMMARY`.

Does not run there: the netdev tests (skip without `SAI_RS_PRIVILEGED_TESTS`); `sai-backend-openbcm`'s
fake smoke run (`SAI_SMOKE_LIBSAI`); anything touching the chip, the SDK or a service. The goldens
(G6) run there; the bundle has not been run on the switch since G0. About 5 minutes on the T2080
(G0), suites one after another.

## Parity

Targets in `ai_re/tests/nixtargets.py`. The C++ library was the reference: where a C++ status or call is
observed by a shared suite or the op scripts, this library gives the same. Since G6 (2026-11-23) each
comparison is one of two kinds:

- **live**: against C / C++ that still exists, the frontend frozen in `ai_re/nix/packages/sai-teralynx/frontend`
  (`SAI_OPENBCM_SRC` = its `src`, `SAI_OPENBCM_TESTS` = its `tests`; the CS8210's libsai compiles the same
  files, so they must stay equal anyway);
- **golden**: against the output of deleted C / C++, captured at commit 50a6aa7 into `crates/*/tests/golden`
  (embedded with `include_str!`, so they also run in the bundle). Each golden's header names the C / C++
  files (sha256) and how it was captured; a deliberate change edits the golden by hand with a reason
  (never regenerated from the Rust output). Generated cases use fixed seeds and store their inputs (or
  digests of them), so a changed generator is reported as such.

| comparison | kind | reference | size (2026-11-23) |
|---|---|---|---|
| `test_sai` through the test port (`tests/test-port/run.sh --strict`, `nix/parity-harness.nix`) | live | frozen `test_sai.cpp` + `.inc`; the frozen C++ on its fake as the reference run | 3014 / 3014 |
| op-script differential (`sai-driver --test differential`) | live | frozen `frontend_driver.cpp` on the C++ fake | 11 scripts, 455 / 455 ops, allow-list empty |
| MACsec model differential (`sai-macsec --test differential`) | live | frozen `sai_macsec.cpp` (+ the D9 / D10 edit, `tests/cpp/listed_fixes.rs`) | 16 backbone + 272 failure-injection + 300 random scripts per run |
| MACsec end to end at MDIO level (`sai-macsec-phy --test e2e`) | live | frozen `sai_macsec.cpp` + `sai_extmacsec.cpp` | 4 backbones + 64 synthetic (+ 48 AS7772 with `SAI_MACSEC_E2E_TABLE`) per run |
| `openbcm-diag` client (`openbcm-diag --test differential`) | live | frozen `openbcm_diag.c` | 29 command lines |
| diag server (`sai-core/tests/diag.rs`, `SAI_DIAG_CPP_ORACLE`) | live, by hand | a program over the frozen `sai_diag.cpp` (not built by any check) | |
| glue calls (`sai-backend-openbcm --test glue_differential`) | golden | `openbcm_backend.cpp` + the C++ frontend over a glue stub replaying the Rust fake's answers | 12 scripts, 469 / 482 ops identical (1 entry), 1008 / 1008 glue lines |
| error table (`sai-backend-openbcm --test parity`) | golden | `map_rv` of `openbcm_backend.cpp` | 19 codes |
| MDIO interpreter (`mdio-script --test differential`) | golden | `mdio_script.c` | 5053 corpus + 2200 generated |
| MDIO tool (`mdio-tool --test differential`) | golden | `mdio_script_main.c`, `mdio_report.c` on `tests/c/mdio_stub.c` | 159 runs (44 fake chip, 75 without, 40 diag socket) |
| `openbcm-asic-temp` (`openbcm-asic-temp --test differential`) | golden | `asic_temp.c` + `temp_files.c` on a glue stub | 30 cases |
| table checkers (`phy-seq --test differential`) | golden | `ext_*_check.cpp` over `sai_extphy.cpp` / `sai_extmacsec.cpp` | 683 corpus + 42,770 generated runs; AS7772 tables 631 + 1 (`PHY_SEQ_TABLES`) |
| C harness (`sai-ffi/tests/harness.c`) | its expectations | the C++ library's answers, checked by hand against the C++ `libsai.so` before G6 | 374 checks |

The live counts before each capture equalled the golden counts above (C / C++ and Rust agreed on every
comparison at 50a6aa7). Also:

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
  - deliberate improvement (2026-11-23): MDIO arbitration per PHY with diag transactions (section
    "MDIO serialisation and diag transactions"): the MACsec PN / counter polls skip while a diag lease
    holds their PHY (the C++ held off its link polls only and ran every sequence under one lock, so a
    PN read could land inside a tool's firmware-message conversation), the gate never waits for a
    diag client, raw MDIO runs without the SAI lock; the status text gains its arbitration lines;
    `phy raw c45` on a switch with a PHY table is answered by the library over its MDIO handle (the
    SDK shell's format), not by the SDK shell;
  - the virtual chip's QoS, mirror and L2-extras models are off (as the C++ fake's toggles) and
    reachable only through the test port and the driver (`feature`).
- **syncd** (`vm-sai-rs-syncd`, `nix/test-syncd.nix`, Rust key names): syncd on this `libsai.so`
  runs `nix/sai_slice_smoke.py` through libsairedis. **The C++ package's syncd smoke test unchanged**
  (`vm-sai-rs-syncd-parity`, the C++ profile): `nix/sai_smoke.py` (moved here from sai-openbcm) through
  syncd, the C++ test's ASIC_DB expectations and its "oper up" log line; it differs from the deleted C++
  `test-syncd.nix` only by its diagnostics.
- **Full SONiC on the virtual backend**: `vm-sai-rs-sonic`, `-l3`, `-acl` (the workspace's library) and
  `vm-sonic-openbcm-fake`, `-l3`, `-acl` (the boards' package through `hardware.openbcm`) run the fake-backend
  SONiC tests of `ai_re/nix/modules/switch/sonic/tests`; `vm-openbcm-mdio-units` the boot unit's VM test on
  the Rust tool. Results: [Build and test](#build-and-test).
- **seqcheck** on the five real recordings (`0x5000` ingress write, before G6): C++ tool,
  `openbcm-ext-macsec-rs`, the C++ library on the Rust engine and the build with both agree (s1-s3: 40
  sequences / 4602 operations; s4-s5: 4 / 1952). Soaks before G6 (2026-11-23): sai-macsec 50,000 seeds
  equal to the C++ with D9 / D10 (255 reach D9, 472 D10, 3.67M operations); e2e 50,000 synthetic and 9,600
  AS7772 scripts. On the AS7772: the status block.

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
| `sdk-glue` | none | 52: real 48, error 2, link 2 | 0 / 1 | | one block per glue call (`real`), `ob_sdk_errmsg`'s pointer, the L2 callback's MAC read |
| `tap-netdev` | deny; allowed in sys | 9 | | | ioctls (TUNSETIFF, TUNSETCARRIER, SIOCSIFHWADDR), if_nametoindex, socket, bind, setsockopt, poll |
| `sai-sys` (forbid), `sdk-glue-sys` (none) | | 0 | | generated `unsafe extern "C"` blocks; 26 `safe fn` (sdk-glue-sys) | declarations only |
| the other 15 | forbid | 0 | | | |

Total 143 blocks in three crates (+3 behind `test-port`), 2026-11-23 after G6: -88 with the two crates
that plugged Rust into the C++ (`phy-seq-capi` 28 + 10 in-file tests + 26 in `tests/`, `sai-macsec-capi` 60
+ 35 in `tests/`), deleted with it; nothing else changed (`sdk-glue/tests` 1: a test added during G6 by
another agent). Before G6:

231 blocks in five crates (+3 behind `test-port`) after G2: `sdk-glue` -2 (its
C-fake hook module `fake.rs` deleted with `fake_glue.c`; its `tests/` -1: `before_init.rs`, which
called the C glue directly); the G2 backend, the call recorder, `openbcm-asic-temp` and the smoke
program added none (`sai-backend-openbcm` and `openbcm-asic-temp` forbid it). 233 at G1, 194 on
2026-11-22. The delta: `sai-ffi` +14
(68 then): `convert` +7 (ACL field / action data, the three `MACsec` key members), `notify` +2
(the FDB event callback: the stored address as its function type, the call), `state` +2 (the
profile callbacks, split per call), `generic` +1 (`MACsec` decode), `global` +1
(`sai_dbg_generate_dump`'s path), `diag` +1 (`getsockopt(SO_PEERCRED)`: std's
`UnixStream::peer_cred` is unstable, `rustix` / `nix` are not dependencies); the `MACsec` API's
own 19 blocks (`sai-ffi/src/macsec.rs`) are gone: it uses the generic entry points, and only the
three key reads remain. `sdk-glue` +25 (29 then) was the rework of its wrappers for G2 (all 75 glue
functions, the trampolines).

### Miri

`cargo miri test -p sai-ffi --lib`, `--test abi`, `--test global_a1`, `--test macsec_abi` (rustup
nightly 2026-11-21, `MIRIFLAGS=-Zmiri-disable-isolation`): 16 + 1 + 1 + 1 tests pass (2026-11-23),
including the lifecycle with the notifier thread and callbacks, and the `MACsec` API with key
material through the generic entry points. Under Miri the log does not open `/dev/log` (no AF_UNIX
there). Limit: callback pointers are kept in the safe core as addresses, so integer-to-pointer
casts remain ("might miss pointer bugs"). **Not run under Miri**: `tests/c_abi.rs` and
`exports.rs` (a C program, the built ELF), `tests/diag.rs` (unix sockets), `tap-netdev`
(syscalls), `sdk-glue` (calls C). The pinned-nixpkgs toolchain has
no Miri.

## Dependencies

Direct dependencies, all of them (the rest of `Cargo.lock` is theirs):

- `bindgen` (build only): C headers to Rust (SAI, `sdk_glue.h`), with
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

In the C++, not fixed there (file:line of `ai_re/nix/packages/sai-teralynx/frontend/src`, the frozen
verbatim copy of `sai-openbcm/src` at 50a6aa7, where the CS8210's libsai still has them):

- **D1** `sai_macsec.cpp` finds an installed SA through `sc.hw` (set :241, used :960 / :977
  remove, :379 poll): after a flow moves to another port `remove_macsec_sa` runs nothing and
  returns success; **the key stays in the first PHY**. Fixed in the Rust model (test
  `where_a_flow_moved_the_cpp_leaves_the_key_in_the_phy`). **Q7** (a flow switch leaves the old
  SC's SAs installed) and **Q10** (a cipher change under waiting SAs installs the first 16 key
  bytes) are fixed as D9 / D10; Q1-Q19 with line numbers: `crates/sai-macsec/src/lib.rs`.
  Q7 also in two steps (found by the random differential, 2026-11-23): after `eadmin e3 0` the
  SA stays (both), then `entry e1 t3 1 f5 0 0` gives the port a flow without an SC; the C++ opens
  the gate on the old SC's SA (`R 1f3`, `W 121=1`: data of f5 through c3), the Rust takes it out
  (`R 1f2`, `W 310=8001`) and keeps the gate closed. That is D9 (its rule is on the state: a bound
  flow, an installed SA of another flow's SC); the old classifier only took extra writes. Tests:
  `d9_another_entry_takes_over_after_the_entry_was_disabled` (differential, e2e),
  `d9_a_flow_switch_through_a_disabled_entry` (model). SONiC never reaches D9 (one flow per port
  and direction, `SCI_IN_INGRESS_MACSEC_ACL` false).
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

- The AS7760 has never run this library (off the bench): status block. Its SDK inputs (config.bcm,
  sai.profile, LED / rc program, port_config.ini, hwsku tree) are unchanged by the migration (the same
  store paths as the C++ system at 50a6aa7) and pinned in `boards/as7760/sdk-config-test.nix`. No VM test
  runs its 4x25G break-out on the virtual backend (`SAI_RS_VIRTUAL_PORT_MAP` exists for it).
- The CS8210 still runs the frozen C++ frontend (`sai-teralynx`); `sai-backend-teralynx` does not exist.
- `sdk-glue`'s FFI module `real.rs` runs off-target on the glue stub (x86_64, valgrind), not in the
  ppc64 bundle; with the real glue on a chip only (the AS7772 since G2). The glue-call
  golden holds the C++ to the Rust fake's answers (replay): it shows equal calls for equal answers, not
  the C++ on answers of its own; 10 of the 75 glue functions are not reached by its scripts.
- The hardware backend has what `openbcm_backend.cpp` had and no more: no QoS, mirror, sampling,
  hash, storm control, egress ACL, warm boot on the chip (`NotSupported`, as the C++).
- Frontend: one switch, no breakout; 42 attributes of the headers have a member the rim does not
  decode (answered "not supported"); the capability table is gated by `Backend::programs`. Not
  ported because the C++ has none: bulk, warm boot, STP ports, tunnels, packet I/O.
- No MACsec VM test on a libsai: `vm-macsec.nix` runs SONiC's own virtual SAI; the MACsec path of
  this library is covered by `test_sai`, the differentials and the model's tests (and G4 on the AS7772).
- No fuzz target. The test bundle has not been run on the switch since G0.
- Goldens are as good as the inputs they were captured on (the generators' seeds, the corpora, the glue
  scripts); what the deleted C / C++ would have answered elsewhere is unknown now.
- Build path: the AS7760 / AS7772 systems need rust/sai, which `path:./nix` cannot reach: they build from
  the root git flake (deploy.sh; new files need `git add -N`, `ai_re/tools/nix-untracked.sh`) and as
  `extra:as77*-system` from the working tree; their `path:` outputs are KNOWN_BROKEN in nixtargets.

## Hardware runbooks

Run on the AS7772: G0 (evidence `ai_re/devices/edge-core-as7772-32x/evidence/rust-g0/`: the bundle, the MDIO
tool `--verify-only`, MACsec on the C++ library with the Rust engine and model) and G2-G4 (`.../rust-g2/`: the
smoke below, the Rust MDIO tool as the boot unit over a reboot and two cold boots, syncd on this library with
the device scripts). The same smoke is the first step on another board with this SDK.

The G2 smoke on the AS7772 (for the hardware agent; root, from the ppc64 build of
`nix/sai-openbcm-rs.nix` with libsdk as `$S`). Stop the stack first: syncd's stop hook restarts
`openbcm-asic-temp` about 10 s after syncd stops, and the program refuses while anything holds the SDK
lock. The profile's temperature publisher writes `/run/asic/temperature` during the run and removes it at
`remove_switch` (fand goes to full speed after its grace period until SONiC is back).

```sh
systemctl stop sonic.target; sleep 15; systemctl stop openbcm-asic-temp
P=$(systemctl cat sonic-syncd | grep -o '/nix/store/[^ ]*-sai\.profile' | head -n1)
# run 1: the PHYs left alone (no ext-PHY / MACsec table served)
$S/bin/sai-openbcm-smoke --sonic-is-stopped --libsai $S/lib/libsai.so --profile $P \
  --cabled 49,53,9,13 --lag-port 57 --no-ext-phy --mdio-phy 0x0 --mdio-reg 0x8b00 --mdio-expect 0x2391 \
  --log-stderr 2>&1 | tee g2-smoke-noextphy.txt
# run 2: the same without --no-ext-phy (the board's tables: real admin sequences on the PHYs)
# optional run 3: --recreate (a second "init all" in one process, never tried on hardware)
systemctl start sonic.target
```

Lanes 49, 53, 9, 13 are cages 1, 2, 31, 32 (the cabled DAC pairs), 57 cage 3 (not cabled). `0x2391` for
1.8b00 of PHY 0x0 is read from `sonic.md` ("ID 2391/80a0"), not from the box. Never pass `--poke-diag`
on hardware (the fake's link-scan pass). Only hardware shows: the KNET netdev and its carrier, link
through the PHYs with and without the tables, the SDK's own diag output, the BCM56960 accepting the
ACL / policer / L3 objects, plausible temperatures, a second "init all" in one process.

## Build and test

From `ai_re/` (what is in the working tree):

```sh
just test-eval                                   # every target evaluates
just test-packages -k sai                        # the workspace (fmt, clippy -D warnings, tests, harness,
                                                 # live differentials, goldens), parity, packages, sai-teralynx; ppc64 builds
just test-systems -k 'as7772 or as7760'          # extra:as77*-system (the path: ones are KNOWN_BROKEN)
just test-deploy -k 'as7772 or as7760'           # the root git flake's systems and images (deploy.sh's form)
just test-vm -k 'openbcm or sai or mdio'         # syncd, parity, SONiC fake-backend tests, privileged, MDIO units (kvm)
just sai-rs-test-on root@<host> sudo ssh         # the bundle on a switch (Big-endian runs)
```

Results of this tree (G6, 2026-11-23, x86_64): test-eval 220 passed, 19 xfailed (KNOWN_BROKEN); test-packages
`-k sai` 16 passed, `-k 'sai-rs or mdio or phy-seq'` 8; test-systems 4 passed (11 KNOWN_BROKEN path: outputs
skipped); test-deploy 23; test-vm 10 (`vm-sonic-openbcm-fake`, `-l3`, `-acl`, `vm-openbcm-mdio-units`,
`vm-sai-rs-sonic`, `-l3`, `-acl`, `-syncd`, `-syncd-parity`, `-privileged`). `cargo test --workspace` in
`sai-rs`: 566 passed, 0 failed (117 suites, with the real-stub runs).

By hand, in the package's own build environment:

```sh
cd rust/sai
E='let f = builtins.getFlake "path:'$PWD'/../../ai_re/nix"; p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
   sonic = import (f.outPath + "/packages/sonic") { pkgs = p; };
   in p.callPackage ./nix/package.nix { saiSrc = sonic.sources.sairedis;
        glueSrc = '$PWD'/../../ai_re/nix/packages/sai-openbcm/src; cppFrontendSrc = '$PWD'/../../ai_re/nix/packages/sai-teralynx/frontend/src; }'
nix develop --impure --expr "$E" -c cargo test --workspace
nix develop --impure --expr "$E" -c cargo clippy --workspace --all-targets -- -D warnings
nix develop --impure --expr "$E" -c cargo fmt --all --check
```

`sdk-glue-sys` links C only with `real` (`SDK_GLUE_LINK`: `sdk_glue.c` + libsdk, or a test stub in
its place); tests use `sdk-glue`'s Rust fake. A build with both `real` and `rust-fake` gives the
plain bring-ups the real glue (how `nix/mdio-tool.nix` runs the tool's tests on the MDIO stub). The glue's
header: `SDK_GLUE_INC`, else the monorepo's `ai_re/nix/packages/sai-openbcm/src`. Outside Nix:
`SAI_INC=<SAI/inc> cargo test --workspace` with libclang and a C compiler (`LIBCLANG_PATH`, `CC`); the live
differentials skip without `SAI_OPENBCM_SRC` (the frozen frontend's `src`), the goldens always run.

The live parity harnesses by hand (from `rust/sai`, in that environment; the C++ is the frozen frontend):

```sh
FE=../../ai_re/nix/packages/sai-teralynx/frontend
SAI_INC=... bash tests/test-port/run.sh --strict     # test_sai on this library + the frozen C++ as reference
SAI_OPENBCM_SRC=$FE/src SAI_OPENBCM_TESTS=$FE/tests cargo test -p sai-driver --test differential -- --nocapture
cargo test -p sai-backend-openbcm --test glue_differential -- --nocapture   # the glue-call golden
# the smoke program on the Rust fake glue
cargo build -p sai-ffi --features openbcm-fake && \
  SAI_SMOKE_LIBSAI=$PWD/target/debug/libsai.so cargo test -p sai-backend-openbcm --test smoke
bash tests/unsafe-count.sh                           # the unsafe inventory ([Unsafe](#unsafe))
```

The socket tests need a short `SAI_RS_TEST_TMPDIR` (unix socket paths have 107 bytes).

### Updating the numbers in this file

Unsafe counts: the method under [Unsafe](#unsafe). Test, suite and comparison counts: the end
of `run.sh`'s output (the Nix checks print the same lines). Change a number with its date.

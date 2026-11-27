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
- **CS8210 (Teralynx 7): the Rust libsai is the box's default since H4 (2026-11-27).** The Rust Teralynx libsai and shim are
  at parity with the C++ `sai-teralynx` on the mock libifcs ([Parity](#parity)) and passed the chip's smoke (H1), feature
  matrix (H2) and SONiC under a production-like config with soak, reboots and rollback (H3); the C++ stays selectable
  (`saiImplementation = "cpp"`, or the previous generation) until H5.

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
| Teralynx backend (CS8210) | `sai-teralynx/src`: `ifcs_backend.cpp`, `tl_*.cpp`, `ifcs_config.cpp`: stay (the box's SAI) | `sai-backend-teralynx` (every area of the C++) under `sai-ffi` feature `teralynx` ([Teralynx backend](#teralynx-backend-cs8210-32x)) | `test_ifcs` 1047 / 1047; parity harness strict green ([Parity](#parity)) | never: H1 + H2 passed on the chip, H3 pending | not yet (`nix/libsai-teralynx.nix`: `sai-rs-teralynx`, aarch64 `cs8210-sai-rs-teralynx`) |
| libifcs loader + bindings (CS8210) | `ifcs_lib.cpp` (dlopen, 148 symbols, GOT redirects); `ifcs_api.h` from `tools/gen_ifcs_api.py`: stay | `ifcs` (rim), `elf-got` (GOT redirect), `ifcs-sys` (bindgen of the generated header; built, never committed) | layout check: 652 assertions by the target's C compiler (x86_64, aarch64); `test_ifcs` through it | never: H1 + H2 passed on the chip, H3 pending | with the backend |
| Teralynx shim (CS8210) | `sai-teralynx/src/shim.c`, `ifcs_guard.c`: stay (the C++ package's) | `ifcs-shim` (rim) + `ifcs-guard` | differential 416 scripts / 95,582 steps equal but the one deliberate difference (x86_64; aarch64 emulated: 416); `test_ifcs` 1047 / 1047 with the Rust shim (x86_64, aarch64 emulated) | never: H1 + H2 passed on the chip, H3 pending | not yet (`nix/ifcs-shim.nix` builds it) |
| Teralynx parity harness | (the C++ side: sai-teralynx's libsai, `test_ifcs`, `frontend_driver.cpp`, `oracle/sai_script.cpp`, unmodified) | `sai-teralynx-parity` | is the evidence of the rows above ([Parity](#parity)) | n/a (mock libifcs) | target `sai-teralynx-parity` |
| Teralynx tools (CS8210) | `tools/sai_smoke.cpp`, `trap_frames.c`, `qos_frames.c`, `raw_frames.c`, `l3_frames.c`, `sflow_probe.c`, `oracle/sai_script.cpp`; the header generator `gen_ifcs_api.py`: still in sai-teralynx until H5 | `sai-teralynx-smoke`, `tl-frames`, `sai-script` (over `sai-client`), `ifcs-gen` | smoke C++ form identical; tl-frames veth differential; sai-script `--cpp-names` identical on 27 scripts; ifcs-gen byte-equal headers | CS8210 H1 (smoke), H2 (sai-script + tl-frames, 37 scripts) | default in the drop-in `sai-teralynx-rs.nix` |
| Teralynx oracle (CS8210) | runner `oracle/sai_script.cpp`: kept until H5 as the parity reference (ported: `sai-script`, row above); tracer `trace_rt.c` / `gen_tracer.py`: stays (it traces the vendor isai) | | | | |
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
   +-- sai-backend-teralynx -- ifcs (+ ifcs-sys, elf-got) --> libifcs.so (vendor, dlopen'ed)
 sai-meta, sai-sys   attribute tables, bindgen of the SAI headers (under everything that sees SAI types)

 openbcm-mdio-script-rs: mdio-tool -- mdio-report -- mdio-script;  feature sdk: -- sdk-glue
 openbcm-diag: the diag socket client (openbcm_diag.c);  sai-driver: op-script driver (differential)
 sai-teralynx-parity: C++ vs Rust Teralynx libsai on the mock libifcs -- sai-driver (compare)
 libsai-teralynx-shim.so: ifcs-shim -- ifcs-guard (the learn-ring guard)
 tl-frames: trap-frames qos-frames raw-frames l3-frames sflow-probe (the hw:* scripts' frame tools; AF_PACKET, psample)
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

### Teralynx backend (CS8210-32X)

The Teralynx 7 of the CS8210-32X, on the vendor IFCS 0.12.41 SDK (`libifcs.so` `dlopen`ed at run time from
the switch; nothing vendor linked, built or committed). A port of the C++ IFCS backend of
`ai_re/nix/packages/sai-teralynx` (which stays the box's SAI until this one is proven on the chip).
Only for x86_64 (tests on the mock) and aarch64 (the CS8210); on other targets the four crates are empty.

- `ifcs-gen` (G5, `forbid(unsafe_code)`, no dependencies): the IFCS header generator, sai-teralynx's
  `tools/gen_ifcs_api.py` + `ifcs_ctypes_model.py` in Rust: a parser of Python expressions (the ctypesgen
  file is read as data, never run), the model's line matchers, ctypes layouts (Python 3.14's rules), the
  template directives, the overrides; our templates in `crates/ifcs-gen/templates` (copies of sai-teralynx's
  `src/` until H5). Byte-equal to the Python on the same input, proven live by `nix/ifcs-headers.nix`
  (`passthru.identity`: the headers = sai-teralynx `build/gen`, all 43,268 model facts equal, the synthetic
  fixtures' output and error lines equal) and kept by a sha256 golden; the program (feature `cli`,
  `nix/ifcs-gen.nix`) writes the headers for the C fixtures; no Python in the headers' derivation.
- `ifcs-sys`: bindgen over the GENERATED `ifcs_api.h` / `ifcs_api_qos.h`: its build script generates them
  with `ifcs-gen` from `IFCS_CTYPES` (Nix: the pinned ishell 2.0.1 ctypes; `IFCS_API_EXPECT`: equal to
  `nix/ifcs-headers.nix`'s) or takes `IFCS_API_INC`. Unions are byte blobs (reading a Rust union is unsafe); the
  `IFCS_SYMBOLS` list (148 entries) as `SYMBOLS` and `for_each_symbol!`. Every struct has a byte twin
  `<struct>_bytes` (`CBytes`: its size and alignment, `[u8; N]`), and the two by-value struct parameters
  (`ifcs_l2_entry_key_t_mac_l2vni_set`, `ifcs_route_entry_key_t_ip_dest_l3vni_set`: `BY_VALUE_ARGS`) are
  declared as their twins; the build refuses a by-value struct with a float member, an alignment above 8 or
  in a callback (the ABI argument: the crate docs, "By-value structs"). Layout check 2: 652 assertions
  (sizes, alignments, offsets, every constant, the symbol list) by the target's C compiler
  (`IFCS_SYS_ABI_CC`, `IFCS_SYS_ABI_CHECK=require` in Nix). `forbid(unsafe_code)`.
- `elf-got`: the GOT redirect of `ifcs_lib.cpp` (`redirect_cb`). The ELF is read from the object's FILE in
  safe code (`PT_DYNAMIC`, `DT_JMPREL` / `DT_RELA`, dynsym; `R_X86_64_` / `R_AARCH64_JUMP_SLOT` and
  `_GLOB_DAT`); a slot is written only if it is mapped, aligned and holds an address the symbol binds to (or
  the target already); a read-only page gets its protection back. Rim: 6 blocks (`dladdr`, `sysconf`,
  slot read / write, `mprotect`).
- `ifcs`: the only unsafe rim for libifcs. The loader step for step as `ifcs_lib_load` (absolute path, the
  shim `libsai-teralynx-shim.so` next to libsai first, libifcs `RTLD_NOW | RTLD_GLOBAL`, memcpy and the
  learn-ring guard's GOT redirects and log lines, `SAI_TERALYNX_IFCS_TRACE` with `ifcs_trace_bind`, every
  symbol or "missing symbol X (wrong libifcs version?)"); one safe method per IFCS function over one private
  wrapper per C signature (shape), for 147 of the 148 functions (all but `ifcs_status_to_string`, which nothing
  calls): `call`, `entry` (the L2 / route entry keys, IP prefix accessors and entry calls, generic over the key
  type; the `ifcs_l2_entry_get_all` walk, its `user` a walk number, never dereferenced; the walk's keys copied as bytes), `qos` (queue / BAM
  queue keys and counters); `Attrs` (the C++'s zeroed attributes, its three empty forms, `Attrs::bytes` for
  value structs), `Handle`, `Status` (`map_reason` exactly); callbacks over a keyed registry (`sdk-glue`'s
  pattern); `netdev` (`SIOCGIFFLAGS` / `SIOCSIFFLAGS` / `SIOCSIFNAME` / `SIOCSIFHWADDR`: the adopt mode's rename
  of the SDK's `inno_0_<sysport>` netdevs); `ipd` (the ipd ioctls on libifcs' `/dev/ipd`, found through
  `/proc/self/fd`, or the mock's `tl_ipd_ioctl`; layouts with compile-time size checks); libifcs' own
  `node_pci_reg_read32` / `_write32` for the board's register list. Every struct it hands libifcs (keys, the IP prefix,
  the by-value key members, attributes, queue keys, the QoS map info, the traffic-monitor policy, table usage) is its
  byte twin, zeroed and written field by field at the compiler's offsets (`bytes.rs`): no padding byte is undefined
  and copies keep every byte (H2-R1, 2026-11-26: libifcs compares raw key bytes; Rust struct values with undefined
  padding made the chip answer NOTFOUND to route / FDB deletes). `tests/abi.rs`: C callees with the real signatures
  (`tests/c/abi_fixture.c`) check every byte they get through the crate's call sites, after 64 KiB of 0xa5 on the
  stack; natively and in the aarch64 bundle. 59 blocks ([Unsafe](#unsafe)).
- `sai-backend-teralynx` (`forbid(unsafe_code)`): one module per C++ file, line by line: `core`, `config`,
  `keys` (every key of sai-teralynx's README table, C++ defaults and C parsing: `strtoul` base 0, `atoi`),
  `switch` (init / shutdown in the C++ order, init commands, callbacks, temperatures, table usage, chip limits,
  diag shell), `port`, `counters` (101 ids), `l2` (VLANs as L2VNI + flood group + per-member `mc_dest`, LAG
  members joining as their LAG, FDB add / delete / flush by walk, aging, flood control, L2 events), `lag`
  (members out of the IFCS LAG while egress and ingress are disabled, the hash objects), `l3` (VRFs,
  interfaces, stations, next hops in place, hosts, routes with the ip2me / glean traps, ECMP by repeats and
  `member_replace`, RIF counters, admin state), `hostif` (netdevs in create and adopt mode, `trap_program` with
  the trap -> `CoPP` rule table, IGMP / MLD / `NEIGHBOR_MISS`, system traps, CPU queue counters, diag `copp`),
  `acl` (policers, SAI ACL tables with ranges / egress / `OUT_PORTS` / REDIRECT / `SET_TC` / MIRROR, entry update
  in place and enable, storm control, diag `acl`), `qos` (maps + map groups shared per (DSCP, DOT1P) pair, the
  TC -> queue identity rule, PFC, default TC, schedulers / port shaper in bits/s, WRED curve groups with
  0.12.41's `WRED_ENABLE` / ECN refusals and the rollback, queue buffer thresholds with `RESERVED_SIZE`
  refused, queue / PG / PFC counters), `mirror` (SPAN / ERSPAN on one shared DCDP per monitor sysport,
  collector sets per direction, samplers, the `sFlow` collectors) and `sflow` (genetlink host interfaces over
  the ipd netlink ioctls, the SAMPLEPACKET trap as system trap `MIRROR_TO_CPU` and its CPU-queue meter with
  `SAI_TERALYNX_SFLOW_METER=on`, per-port rates). Across areas: `LagArea::dest_handle`,
  `L3Area::nexthop_handle` (the ACL redirect), `acl::MirrorSets` (the ACL MIRROR action).
  Locking: the SAI API lock is the C++ `Core::mu`; outside it only the temperature reader (own mutex) and
  libifcs' callback threads (no IFCS call, queue to the `EventSink`). Differences from the C++: the crate's
  `lib.rs`. On the mock: `test_ifcs` 1047 / 1047, and every `test_ifcs` section's IFCS calls, the driver
  scripts and the oracle scripts equal to the C++ library's ([Parity](#parity)).
- `sai-ffi` feature `teralynx` (not with `openbcm`: `compile_error!`): `SAI_OPENBCM_BACKEND=openbcm` (the
  CS8210 profile's), `teralynx` and no key select it; its keys are in `profile_keys()`.
- `SAI_TERALYNX_POST_INIT_REGS` (Rust only): a file of `offset mask value` lines (hex `0x` or decimal; `#`
  comments; up to 4096 entries, in order, an offset may repeat), BAR0 read-modify-writes applied after the
  init commands on every init through libifcs' own `node_pci_reg_read32` / `_write32` (never a second mapping
  of the BAR): read, `(old & !mask) | value`, read back under the mask. No meaning in the backend: board data
  (an LED stream setup, per-PIC LED overrides from 0x13700, ...). A malformed file refuses the switch before
  the SDK starts; a failed access or a read-back mismatch is an error line, counted, and the init goes on.
- `SAI_TERALYNX_DIAG_REGS` (Rust only): a read-only watch list for the diag shell, `offset` or `first-last`
  (every 4 bytes) per line, up to 1024 registers. The diag command `regs` (also while syncd runs, under the SAI
  lock like every IFCS call) prints the post-init list (mask, wanted value, the value read now, ok /
  DIFFERS) and the watch list's live values. Nothing from the socket names an offset: an MMIO read nothing
  answers can stall the SoC, so only the board's files decide what is read.
- `nix/libsai-teralynx.nix`: `libsai.so` (`-z defs`, soname `libsai.so.1`, the virtual build's exports).
  Native check (`crates/sai-backend-teralynx/tests/test-ifcs.sh`): sai-teralynx's `test_ifcs`, unmodified, on
  it with the C mock libifcs and the C shim, every FAIL line one of `tests/test_ifcs-expected.txt` (none
  since 2026-11-26: 1047 / 1047); the create / remove IFCS call log equal to the C++
  library's on the same mock; the post-init register list on the mock's registers. aarch64: cross build
  (`cs8210-sai-rs-teralynx`) and its test bundle run as aarch64-linux (`nix/teralynx-target-run.nix`).
- Areas (`crates/sai-backend-teralynx/tests/areas.txt`): which area (C++ `tl_<area>.cpp`, G2 group) owns
  each `test_ifcs` check (file / line range), section, driver op and oracle script. `test-ifcs.sh` prints the
  FAIL lines per area (now / expected); an expected line may end in `# needs <area>` (it fails because
  ANOTHER area is not ported yet); `REGEN_AREA=<area>[,...] bash tests/test-ifcs.sh ...` rewrites exactly
  those areas' lines from the run (marked lines stay while they fail), every other line untouched.

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

`nix/sai-teralynx-rs.nix`: `{ saiRs, cppTeralynx }`, the CS8210's (the `sai-teralynx` package's names):
`lib/libsai.so` (`.so.1`, `.so.1.0.0`: `libsai-teralynx.nix`), `lib/libsai-teralynx-shim.so` (`ifcs-shim.nix`),
`bin/openbcm-diag`, `bin/sai-teralynx-smoke`, `bin/sai-script` (the last two wrappers around `libexec/`, host libstdc++ on `LD_LIBRARY_PATH`; [Teralynx tools](#teralynx-tools-sai-client-sai-script-sai-teralynx-smoke)),
`share/sai-rs/` (the parts' reports, `drop-in.txt`). Checked: every file of the C++ package has a
counterpart, 17 exports (the C++'s 15 + `sai_bulk_get_attribute`, `sai_query_object_stage`). Targets:
`sai-teralynx-rs` (native, the mock checks), `cs8210-sai-teralynx-rs` (aarch64). The CS8210 system links it
by default (`nix/nixos/module/cs8210/sonic`, option `saiImplementation`, "rust" since H4).

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
- `sai-diag`: the diag socket server of `sai_diag.cpp` (root only, the command under the SAI lock,
  output written after it is released; differences in its `lib.rs`), except raw MDIO and `extphy
  txn`, which the switch's `sai_core::diag::DiagPort` serves **without** the SAI lock. An accepting
  thread and one thread per connection, at most `MAX_CONNECTIONS` (8) at once (a further one waits
  in the backlog, not refused); the C++ serves one connection at a time, which let a command queued
  on the SAI lock hold up a tool's next raw access (below). Stopping the server cuts the connected
  clients off and joins every connection's thread, so `remove_switch` returns with none left. `openbcm-diag`: the client `openbcm_diag.c`, same command line,
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
  counted); MACsec periodic reads (`pn.*`, `stats-sa.*`, `stats-flow.*` of a get) skip (the model's
  last read answers, counted, no error line), and a read that has not reached its PHY for
  `MACSEC_POLL_DEFER_MAX_MS` 10 s has its get wait for the transaction to end (<= 2 s, **outside the
  SAI lock**, below) and reads, never inside one; SONiC's MACsec changes (SA install / remove, gate,
  enable / disable, clears) and admin sequences wait for a **transaction** (<= 2 s, outside the SAI
  lock), never dropped, and pass implicit leases (as in the C++); a diag `extmacsec run` is refused
  inside a transaction. A new transaction waits for library runs on its PHYs, for library users
  already waiting and for SAI calls that claimed them, so back-to-back transactions cannot starve
  them.
- **No SAI-lock wait behind a lease** (Y1, 2026-11-23; the AS7772's two "txn expired (1001 ms, 0
  accesses)" in `evidence/rust-g7` d4: an SA install waited for a lease *holding* the SAI lock, the
  lease holder's next request sat behind another client's `extmacsec` that needed the lock, only the
  expiry broke it). Every SAI call of the rim runs in `sai_extphy::call` (`sai-ffi` `with_api_call`:
  create / remove / set / get / stats / clear) with the lock marked (`under_sai_lock`). A PHY change
  **claims** the PHYs it will run on before its first effect (port admin: the port's ext-PHY PHYs;
  MACsec: the model's `reach`, the PHYs of the one port the change runs on); a claim is taken at once
  when no transaction holds them, and then no transaction may begin on them until the call returns.
  When one does, the call returns at once with nothing done, the rim lets go of the SAI lock, waits for
  the transaction (<= `LEASE_MAX_MS`, the PHYs held claimed meanwhile so no new one begins) and runs
  the call again, which re-validates by claiming again (an object that moved claims its new port: one
  more bounded wait at most per PHY). A forced read defers its get the same way. So link
  notifications, port sets on other ports and diag status commands never wait behind a lease. A
  change on PHYs its call did not claim that meets a transaction under the SAI lock is a bug of the
  claims: asserted in debug builds, counted (`waits under the SAI lock`) and logged in release ones.
- **The link gate never stalls the link scan** (Y1): it answers its cached verdict behind *any*
  holder it cannot get at once (try only, bound 0): a lease (counted as before) or a library run
  (counted, `behind a library run`); its poll stays due and runs on the next pass, so link-down is
  seen at most one pass (~255 ms) later in the common case. The scan pass is 250 ms and a poll costs
  0.2-0.36 ms of bus, while a library run holds its PHY 1.3 ms (PN read), 14-18 ms (SA statistics) or
  80-90 ms (SA install) on the AS7772: before this change the gate waited up to 72-89 ms behind SA
  installs (G7, also right after boot with no tool running), holding every port's link processing
  up; X1's expectation (`max ~20000 us`, "one MACsec stats run") was wrong, it left SA installs out.
  Starvation bound: after `GATE_BUSY_MAX` (3) busy answers in a row the next due poll waits for that
  one run (it goes before every other library user of those PHYs), so a port is polled at the latest
  3 passes (~765 ms) plus one run (<= ~90 ms) after its poll was due.
- **Lock order** (one global order, by construction): a PHY claim (waited for outside the SAI lock
  only), then the SAI lock, then a PHY slot, then the MDIO master's own lock; a lease is a record,
  nobody holds anything while it lives. The runtime's mutexes are leaves (`sai-extphy` `leaf`: debug
  builds assert no nesting, no logger and no bus access under them); slots are never held while
  anything waits; a claim is never waited for under the SAI lock (asserted); the gate closure and the
  `DiagPort` have no way to take the SAI lock; the diag server serves raw clause-45 MDIO and
  transactions over the backend's MDIO handle (answer format the SDK shell's `\t0x%02x: 0x%04x`) when
  a PHY table is attached and the backend's shell would answer it from that bus
  (`Backend::diag_mdio_native`; the virtual chip only in `ShellMdio::Answer`); everything else still
  goes through the SDK shell under the SAI lock, within the arbitration.
- **Visibility**: the `extphy` text keeps the C++ header (`PAUSED now` = some lease live, `N so far` =
  leases begun) and table, then six lines: transactions begun / ended / expired, hold-offs, busy /
  void refusals; link polls answered from a lease and behind a library run, gate waits (count, max,
  total us, from before the wait; only after 3 busy in a row); the gate-time histogram per due poll
  (us: <=500, <=2000, <=10000, <=100000, more; max), lock wait and poll included; MACsec polls skipped,
  forced reads (max wait), runs and SAI calls that waited for a transaction, `extmacsec run` refused;
  SAI calls that waited for a transaction outside the SAI lock and waits under the SAI lock (0 unless
  the claims have a gap); then one line per live lease. Log: a NOTICE per implicit lease begun
  (`external PHY phy 0x10: link polls and MACsec polls paused: raw MDIO on the diag socket without a
  transaction ...`), per expiry, per forced read, per SAI call that waited (`external PHY phy 0x10: a
  SAI call waited N ms for diag transaction T, outside the SAI lock`); an ERROR per wait under the SAI
  lock; INFO per transaction begun / ended.
- **Not solved, documented:** MACsec counters clear on read; a diag read of them (transaction or not)
  takes those counts from SONiC's statistics. A client that dies in the middle of a conversation leaves
  the PHY's message interface in an unknown state for the next user after its lease runs out.
- Tools: `evora-rehost/mdio.py` (`Diag.transaction()`) and `phystat.py` (one transaction per register
  read, `--txn phy|all|off`). Tests: `sai-extphy/tests/arbitration.rs` (a stateful fake of the
  message interface, `sai_extphy::fwphy`, that breaks a conversation on any foreign access, as the
  hardware does; the gate behind slow runs, the starvation bound, link-down one pass late, a SAI
  call's deferral and the debug assertion), `sai-core/tests/macsec.rs` (through the SAI layer and the
  socket, real threads; `the_expired_lease_loop_of_the_as7772_cannot_form` sets up the G7 loop
  exactly), `sai-diag/tests/server.rs` (concurrent clients, the connection bound, stop with clients
  connected).

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
- `sai-teralynx-parity` (nix target `sai-teralynx-parity`): the C++ and the Rust
  Teralynx libsai, each on the SAME mock libifcs, under the same unmodified programs, in the same
  directories: `test_ifcs` (checks per section: "Rust passes N of 1047", every FAIL line with its area),
  its IFCS calls per section (`TEST_IFCS_DUMP=1` + `c/tap.c`, which streams every mock line past the
  tests' log clears; `c/sections.cpp` marks the areas through `-Wl,--wrap`), the frontend's driver-scripts
  01-09 through `frontend_driver.cpp` as a plain SAI client (`c/tl_port.cpp`: the test port over the
  mock's hooks, the op's IFCS lines as its `be` lines) and the oracle's JSON scripts through
  `sai_script.cpp` (`c/oracle_prep.cpp`: `exec` steps dropped, sleeps 0); host interfaces in
  `SAI_TERALYNX_NETDEV=create` mode (the build host has no netdevs to adopt). A difference is allowed
  (`tests/parity-allow.txt`: each entry with a reason; one that matches nothing is red), pending (its area still has lines in
  `sai-backend-teralynx/tests/test_ifcs-expected.txt`, or a `tests/parity-pending.txt` entry gives it to
  such an area), downstream of a pending one in the same run, or red; `--strict`: only allowed ones
  pass (the target reports it in `strict.txt`; required with `strict = true`). The C++ must pass 1047 / 1047 and repeat itself; a negative control (`--mutate`: one
  Rust call line changed) must be red. An area owner's run from `rust/sai`:
  `nix build --impure --expr ...sai-teralynx-parity` (`ai_re/tests/nixtargets.py`), then
  `result/bin/sai-teralynx-parity-run --rust-lib target/debug/libsai.so --parts sections,checks --only l2,lag`
  (report: `<work>/out/report.txt`, every differing unit in `details.txt`, both sides' outputs beside it);
  or without Nix: `build-fixtures.sh` (its header) and `cargo run -p sai-teralynx-parity --features cli`.

### Teralynx shim: ifcs-shim, ifcs-guard

`libsai-teralynx-shim.so` of the CS8210 in Rust, a drop-in for `ai_re/nix/packages/sai-teralynx/src/shim.c` +
`ifcs_guard.c` (which stay the C++ package's; not changed): libsai dlopens it from its own directory
(`RTLD_NOW | RTLD_GLOBAL`) before the vendor `libifcs.so` and points libifcs' GOT entries at it (the GOT
write stays in libsai's process: `ifcs_lib.cpp`, later crate `ifcs`).

- `ifcs-guard` (forbid unsafe): the learn-ring read guard (`ai_re/devices/cs8210-32x/learn-wedge.md` §2) as a
  state machine over an `Io` trait (the original accessors, clocks, the 3 ms sleep, the journal, the stats and
  inject files): validity rules, re-reads, substitution, degraded mode, all-ones counting of other offsets,
  the `im_malloc` limit, journal text and rate limit, the `.prom` text and its rewrite cadence, the inject
  hook (`fscanf("%x %u")` as glibc reads it), init / installed, with the C's buffer sizes and truncation
  (but the counters' text buffer: the deliberate difference below).
  No allocation; a valid guarded read takes no lock (as the C).
- `ifcs-shim` (cdylib, soname `libsai-teralynx-shim.so`; installed under that name by `nix/ifcs-shim.nix`):
  the rim: the C library's 26 exports and nothing else (`nm -D` compared), the 19 CPython stubs (the two
  variadic ones defined with their fixed parameter: sound on AAPCS64, SysV x86_64 and ELFv2, see the
  module's text), `tl_devmem_memcpy` (volatile accesses, the C's 8 / 4 / 1 width choice), the guard's
  entry points (`catch_unwind` with a named fallback in each), `tl_ifcs_im_malloc` as a naked tail jump
  that passes the caller's address (the C's `__builtin_return_address(0)`; x30 / `[rsp]`). stderr through
  `write(2)` (`std::io::stderr` allocates a thread handle on first use).
- Evidence (2026-11-25, numbers of 2026-11-26): `ifcs-guard` 27 tests (rules one by one, the C test's learn scenario, a property
  test: 0x4860 never answered with bits 13..31, the RMW registers never all-ones, at most 3 re-reads and
  none while degraded, the journal's window); the live differential `ifcs-shim/tests/differential.rs`
  (C harness `tests/c/guard_harness.c`; the C library compiled from the sources above unmodified; 16 corpus
  scripts + 400 generated, inputs digest pinned): 95,582 steps, every trace line equal but the 6 of the
  deliberate difference below (values, codes,
  original-accessor calls, sleeps, journal bytes, file versions, stubs, copies), per step the Rust allocates
  no more than the C (except init: the environment copy), no thread-local destructor registered; the same
  416 scripts equal on aarch64 under emulation (the same allow rule). Drop-in: sai-teralynx's own `test_ifcs` with the Rust
  library in place: 1047 / 1047 (x86_64; aarch64 emulated, C baseline 1047 too). `tl_devmem_memcpy`'s
  disassembly: single loads / stores of the chosen width, no SIMD, no pair but the frame record, no call.
- Deliberate difference (found by the port; fixed in Rust 2026-11-26, coordinator decision; the C keeps
  it until it goes at H5): with the 16 all-ones names in use the counters' text outgrows the C's 4096-byte
  buffer at a few hundred events per name (`render` returns -1): the C no longer rewrites the file and
  `tl_ifcs_guard_text` answers "", exactly in the dead-chip case the file is for. The Rust buffer
  (`TEXT_CAP`, 5120 bytes) holds the worst case, 4720 bytes (every counter at `u64::MAX`, the 16 names at
  0xffffffff, `installed` at `i32::MIN`, both timestamps at -2^63 s: test
  `the_worst_case_counters_fit_the_buffer`; `counters_beyond_the_c_buffer_are_written`). The differential
  allows exactly that and nothing else (`compare_allowing_text_cap`: a `prom` / `text` line of at least
  4096 bytes on the Rust side only, where the C's text is empty; corpus `allones-overflow.txt`: 6 lines; the
  emulated aarch64 replay in `nix/ifcs-shim.nix` applies the same rule).
- Differences from the C, none in the differential's observables: a write-back error at `close` of the
  stats file is not seen (std does not report it; the C unlinked then); files are opened `O_CLOEXEC`;
  init copies the inject variable; calls before `tl_ifcs_guard_init` answer -1 (the C called a null
  pointer); on a target other than aarch64 / x86_64 the im_malloc line says `caller (nil)`.
- Build and check: `just test-packages -k sai-teralynx-shim` (`sai-teralynx-shim-rs` native,
  `cs8210-sai-teralynx-shim-rs` aarch64 + emulated runs).

### tl-frames: the hardware test frame tools

`crates/tl-frames` ports `ai_re/nix/packages/sai-teralynx/tools/{trap,qos,raw,l3}_frames.c` and
`sflow_probe.c` (frame senders and counters the oracle's `hw:*` JSON scripts run as `exec` steps: 44 uses of
`trap-frames`, 59 of `l3-frames`, 14 of `qos-frames`, 47 of `sflow-probe`, `raw-frames` from store paths)
to one binary each, with the same command lines, output text and exit codes: `trap-frames TX RX KIND COUNT PPS
[DMAC] [DST_IP]`, `qos-frames TX RX[,RX..] COUNT PPS SIZE DMAC SRC DST DSCP[,DSCP..]` (`ECN=`, DMAC `pfc`),
`raw-frames TX RX HEX COUNT PPS`, `l3-frames key=value..`, `sflow-probe netdev|psample ..` (`HEX=`, `LISTEN=`).
Frame building, checksums, counting and the report text are safe functions on byte slices; the C library
behaviours the command lines depend on (`atoi`, `atof`, `strtoul`, `sscanf("%x:..")`, `strtok`, `perror`
text, `printf` of `nan`) are in `cstd`. `os` is the one unsafe module (12 blocks, each one call:
`socket`, `bind`, `setsockopt`, `sendto`, `recvfrom`, `send`, `recv`, `poll`, `if_nametoindex`,
`if_indextoname`, the interface-flag / MAC `ioctl`; generic netlink is a plain socket, the message parsing is
safe). `tap-netdev`'s wrappers did not fit (it makes the interface promiscuous and ignores outgoing frames,
and has no `sendto` / pkttype / generic netlink), so `os` is separate and `tap-netdev` is unchanged.
The binaries are behind the `cli` feature: `sai-rs` and the AS77xx outputs do not carry them.

Proof (`tests/differential.rs`, run by the nix build as well as `cargo test --features cli`): the C tools
are compiled from the read-only sources and every scenario runs both tools with the same arguments over
veth pairs inside `unshare -rn` (works in the nix build sandbox), comparing exit status, standard output,
standard error (program path masked) and every frame the tool put on the wire, captured on the veth
peer (time fields, run ids and the v6 UDP checksum, which covers the run id and is verified first, masked).
About 70 scenarios: every `trap-frames` kind (the TCP kinds with their checksums), `raw-frames` with
ICMP / TCP / ICMPv6 / IGMP / short frames, `qos-frames` (DSCP lists, size clamps, ECN CE / ECT, PFC, two
receive interfaces, injected GRE copies), `l3-frames` (v4, v6, options, TTL, TOS, flows, `dst_n` carries,
sizes, `smac`, pacing), `sflow-probe netdev` against a chip emulator (every tenth frame comes back as a
GRE 0x8818 sample frame: counts, 1-in-N interval, gaps, `META`, `CHIP`, `HEX`, `LISTEN`), and every
error path (usage, bad MAC / IP / hex / kind, missing interface). `sflow-probe psample` runs against a
`LD_PRELOAD` shim (`tests/fixtures/psample_shim.c`) that swaps the generic-netlink socket for a socket
pair, answers `GETFAMILY` and feeds psample messages (group filter, duplicates, two messages in one
datagram, control and error messages, a missing data attribute, a wrong original size, rate changes), the
same bytes to both tools; the kernel path without the `psample` module (exit 3) runs too.
Not covered: psample from a real kernel and real chip samples (no hardware, no `psample` module in the
namespace; the netlink request and reply layouts are checked through the shim only), `ENOBUFS`, the
`SO_RCVBUFFORCE` effects, timing and rates (masked), aarch64 execution (the aarch64 build compiles; it
is not run), and floating point on aarch64: the C compiler may fuse multiply-adds in `GAPS sd`
(`-ffp-contract`), Rust does not, so the last digit could differ there.

Deliberate differences: `qos-frames` with an empty DSCP list exits 2 with a message (the C divides by
zero); `l3-frames` with a negative `size=` sends 64-byte frames (the C crashes in `memset`);
`raw-frames` refuses a hex pair with a non-hex digit even where `sscanf %2x` would take one digit; the
run id of `l3-frames` comes from a different generator (random either way); a closed standard output is
ignored. Build and check: `just test-packages -k tl-frames` (`tl-frames`, `cs8210-tl-frames`).

### Teralynx tools: sai-client, sai-script, sai-teralynx-smoke

G4 part 1 of the CS-B plan: the CS8210's direct-libsai programs in Rust, SAI C API clients of either
Teralynx libsai (the C++ one stays the box's until the hardware proof). Bins need feature `cli` (the
workspace build installs nothing new); package: `nix/sai-teralynx-rs.nix` (below).

- `sai-client` (the rim; deny unsafe, allowed in `dl`, `ffi`, `raw`, `callback`, `tee`): `dlopen` of a
  `libsai.so` (`--libsai` / `$SAI_LIBSAI` / `lib/libsai.so.1` beside the program), `sai_api_initialize` with
  a profile served from process state, `sai_api_query` per API, create / remove / set / get / stats by
  object type (the C++ programs' dispatch, object type for object type), entry calls, `flush_fdb_entries`,
  `sai_query_stats_capability`; attributes as owned values, encoded into zeroed `sai_attribute_t` arrays
  (lists point into storage the call owns; empty lists NULL, as an empty `std::vector`), a get decoded from
  the bytes the library wrote; notifications as events to one sink; `tee`: stderr through a pipe (the
  loader's guard lines are stderr in both libsais).
- `sai-script`: `oracle/sai_script.cpp` (the README's "Script format"), its tables generated from the C++
  ones entry for entry, its outputs, markers, error texts (nlohmann's accessor messages) and exit codes.
  Fixed, each said on stderr as `sai-script: FIX <step>: ...`, all off with `--cpp-names`: a step on the
  null object of a failed create gets the created type (the C++ stopped the script: `acl`, `qos`,
  `phasec-recreate`); enumerator names by the attribute's own enum (`GENETLINK` of a host interface's
  `TYPE` is 2, not the table-entry channel's 5: `mirror`, `hostif-counters`); attribute and counter names of
  the headers the C++ tables lack. Exec steps unchanged (`/bin/sh -c`, `EXEC rc=<wait status>`);
  `--tool NAME=CMD` (or `SAI_SCRIPT_TOOLS="NAME=CMD;..."`) runs `CMD` for `$ORACLE/bin/NAME` (tl-frames).
  Other differences: the library is `dlopen`ed (a missing one: exit 1); a malformed script's parse
  error text; SAI 1.19 headers only.
- `sai-teralynx-smoke`: `tools/sai_smoke.cpp`'s form (`PROFILE SECONDS [FIRST_LANE[:IFNAME] ...]`, identical
  output and exit codes) and the H1 gate (`--dry`; `--sonic-is-stopped --profile P`; refuses without it or
  while `syncd` runs): 16 steps, PASS / FAIL / SKIP each, `SMOKE_OK` / `SMOKE_FAILED`, exit 0 / 1 / 2; the
  loops as devport pairs (`--pairs`, default `1-2,3-17,31-32`, mapped to SAI ports through the port map's
  first lanes), oper-up with a link event per end, `--hold S`, the guard's GOT counts (stderr) and
  `ifcs-guard.prom`, `regs` through the diag socket (the post-init list read back, the LED watch
  registers printed for the record; SKIP on the C++ library, which has no `regs`), temperatures, a diag
  command, admin down, `remove_switch`, `--recreate`.
- Checks (`just test-packages -k sai-teralynx-rs`, 2026-11-26): the runner's differential
  (`crates/sai-script/tests/differential.rs`) on the mock through the C++ and the Rust libsai: 27 scripts,
  `--cpp-names` identical to the C++ runner on all (stream, stdout, exit, script error); fixed: 22
  identical, 5 fixed (`tests/fixed-scripts.txt`), identical up to their first FIX step, the three the C++
  stopped now run to their end; the exec interface with `--tool`. The smoke program on the mock
  (`tests/mock-smoke.sh`, cables by `tests/c/mock_wire.c` over the mock's hooks): the full gate (16 PASS),
  a dark loop (FAIL oper-up naming it, teardown done), no libifcs, no shim (FAIL guard), guard off, the
  refusals, the gate on the C++ library, the C++ form identical to the C++ program on both libraries.
- H1 on the CS8210 (for the hardware agent, box granted; never run unasked): `systemctl stop sonic.target`,
  then `$P/bin/sai-teralynx-smoke --sonic-is-stopped --profile <syncd's sai.profile> --hold 30 2>&1 | tee h1.txt`
  (`$P` = `cs8210-sai-teralynx-rs`'s output; the LED defaults are the `regs:` lines when the profile names
  `SAI_TERALYNX_DIAG_REGS`, no post-init list on the first run), a second run with `--recreate`; the oracle's
  `hw:*` scripts with `$P/bin/sai-script --libsai $P/lib/libsai.so.1 <profile> <script>` (`--tool` for the
  tl-frames programs). `bin/sai-teralynx-smoke` and `bin/sai-script` are wrappers that put the host libstdc++ on `LD_LIBRARY_PATH` (the vendor libifcs.so needs it outside syncd; the real programs are in `libexec/`; the package checks the wrapper's path holds a `libstdc++.so.6` of the target machine). H1 and H2 passed 2026-11-26 (logs (h), (i) in the device PROGRESS); H3 untested.

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
| Teralynx libsai: C++ vs Rust on the mock libifcs (`nix/teralynx-parity.nix`, `crates/sai-teralynx-parity`) | live | `ai_re/nix/packages/sai-teralynx`: its C++ libsai, `test_ifcs`, the frozen frontend's `frontend_driver.cpp`, the oracle's `sai_script.cpp`, all unmodified, on the same mock | 2026-11-26, strict: green: `test_ifcs` Rust 1047 / 1047 (C++ 1047 / 1047); sections 17 / 17 equal; driver 9 scripts (01-09), 376 / 376 ops identical; oracle 27 scripts: 25 equal, 2 allowed (`crates/sai-teralynx-parity/tests/parity-allow.txt`, 3 steps: the runner resolves the scripts' genetlink TYPE to another enum's value, which sai-core's metadata check refuses otherwise than the C++); negative control (one Rust call line changed): red. aarch64 (target `cs8210-sai-teralynx-parity`: everything cross-built, run under binfmt / qemu-user): the same strict report as native, store paths masked. Byte tap (H2-R1, 2026-11-26): every IFCS call's struct and attribute arguments as ALL their bytes (`@B` lines, `c/bytes_rt.c` over the oracle tracer's wrappers, `SAI_TERALYNX_IFCS_TRACE`) in every part: equal on all (test_ifcs 3766 lines, driver 3999; the driver part counts 385 ops with it: the tap's profile line is one op per script); byte negative control (`IFCS_POISON_PADDING=1`: the key members' tail padding 0xa5): red on the setters' `@B` lines while test_ifcs stays 1047 / 1047; the library before the fix (581449b): red in 13 of 17 sections, 127 driver ops, 27 of 27 oracle scripts (attribute padding after `id`, the key members' and `ifcs_usage_t`'s padding) |
| Teralynx shim (`ifcs-shim --test differential`, `nix/ifcs-shim.nix`) | live | sai-teralynx's `shim.c` + `ifcs_guard.c`, compiled unmodified, on the same read sequences | 2026-11-26: 416 scripts (16 corpus + 400 generated), 95,582 steps, equal but the 6 lines of the one deliberate difference ([Teralynx shim](#teralynx-shim-ifcs-shim-ifcs-guard)); the same 416 on aarch64 emulated; `test_ifcs` 1047 / 1047 with the Rust shim |
| oracle runner: Rust `sai-script` vs `sai_script.cpp` (`crates/sai-script/tests/differential.rs`, in `nix/sai-teralynx-rs.nix`) | live | the oracle's `sai_script.cpp` on the mock, through the C++ and the Rust libsai | 2026-11-26: 27 scripts x 2 libsais: `--cpp-names` identical on all; fixed mode 22 identical, 5 deliberately fixed (`tests/fixed-scripts.txt`); exec / `--tool` identical |
| smoke: Rust `sai-teralynx-smoke` vs `tools/sai_smoke.cpp` (`crates/sai-teralynx-smoke/tests/mock-smoke.sh`) | live | the C++ program on the mock, through both libsais | 2026-11-26: 3 argument sets x 2 libsais identical (stdout, exit), usage |
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
- **syncd + SONiC on the Teralynx libsai** (`vm-sai-teralynx-syncd`, `nix/test-syncd-teralynx.nix`): the CS8210
  board layer unchanged (port map, generated IFCS yaml, sai.profile, cs8210-sai-files) over a stand-in SDK tree
  whose libifcs is the C mock, the Rust shim next to the Rust libsai, dummy `inno_0_<sysport>` netdevs adopted;
  the mock's call log (parity tap) checked for create_switch, 32 ports, admin / link up, a VLAN, RIF +
  neighbor + route, an ACL table and rule, CoPP. 2026-11-26: 38 checks pass (on eb9a34c's workspace).
  Refused as the C++ refuses: `PORT_SERDES_ID` gets, `VLAN_TAG` keep on an adopted netdev.
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

Counted 2026-11-26 (H2-R1 changed no count: the byte twins are safe code, the walk's key copy is the same one
read) with `tests/unsafe-count.sh` (from `rust/sai`): per file of `crates/*/src`, the
`unsafe {` blocks before the file's `#[cfg(test)]` module (comment lines excluded); after it and in
`tests/` they are counted apart. Exports: the `.dynsym` set `sai-ffi/tests/exports.rs` checks.
`safe fn` declarations: `sdk-glue-sys`'s generated bindings (26, all-scalar glue calls whose C side
checks ranges; its build prints the count).

| crate | attribute | blocks (src) | in-file tests / `tests/` | exports | what |
|---|---|---|---|---|---|
| `sai-ffi` | (workspace lints) | 82: generic 25, convert 20, tables 12, global 10, ptr 6, state 4, notify 4, diag 1; + test_port 3 (feature `test-port` only) | 83 / 273 | 17 `sai_*` (+ `sai_rs_test_query` with `test-port`) | the SAI C ABI; `convert` / `tables` blocks sit in macros |
| `sdk-glue` | none | 52: real 48, error 2, link 2 | 0 / 1 | | one block per glue call (`real`), `ob_sdk_errmsg`'s pointer, the L2 callback's MAC read |
| `tap-netdev` | deny; allowed in sys | 9 | | | ioctls (TUNSETIFF, TUNSETCARRIER, SIOCSIFHWADDR), if_nametoindex, socket, bind, setsockopt, poll |
| `ifcs-shim` | (workspace lints) | 17: the stored accessors as their function types + the calls 4, the copy loop 5, `val` 2, im_malloc (errno, calloc) 3, `write(2)` 1, `clock_gettime` 1, init's string 1; + 2 `#[unsafe(naked)]` (im_malloc, aarch64 / x86_64) | 0 / 0 | 26 (the C shim's) | `libsai-teralynx-shim.so` ([Teralynx shim](#teralynx-shim-ifcs-shim-ifcs-guard)); counted 2026-11-25 |
| `ifcs` | deny; allowed in named modules | 59: call 20, entry 10, shim 8, callback 7, dl 5, ipd 3, netdev 3, qos 2, table 1 | 0 / 0 | | libifcs (crate docs, Unsafe); counted 2026-11-26 after G2 package E (+5: `qos` 2, `ipd` 3) |
| `elf-got` | rim module | 6 | 0 / 4 | | the GOT redirect (`dladdr`, `sysconf`, slot read / write, `mprotect`) |
| `tl-frames` | deny; allowed in `os` | 12: socket 2, bind 1, setsockopt 1, sendto 1, recvfrom 1, send 1, recv 1, poll 1, if_nametoindex 1, if_indextoname 1, ioctl 1 | 0 / 0 | | the hardware test frame tools ([tl-frames](#tl-frames-the-hardware-test-frame-tools)); counted 2026-11-26 |
| `sai-client` | deny; allowed in named modules | 31: ffi 16 (one call site per C signature, the method table read), callback 6, dl 5, raw 3 (zeroed C structs, a value union's bytes, an FDB event's oid), tee 1 (`dup2`) | 0 / 0 | | the tools' side of the SAI C ABI ([Teralynx tools](#teralynx-tools-sai-client-sai-script-sai-teralynx-smoke)); counted 2026-11-26 |
| `ifcs-guard` | forbid | 0 | | | the guard's decisions |
| `sai-sys` (forbid), `sdk-glue-sys` (none) | | 0 | | generated `unsafe extern "C"` blocks; 26 `safe fn` (sdk-glue-sys) | declarations only |
| the other 20 | forbid | 0 | | | |

Total 225 blocks in six crates (+3 behind `test-port`), 2026-11-26 after the Teralynx backend reached
parity with the C++: +82 in the three Teralynx rims (`ifcs` 59, `ifcs-shim` 17, `elf-got` 6; 64 when the
scaffold landed, then `ifcs` +18: `entry` 10, `netdev` 3, `ipd` 3, `qos` 2); the AS77xx crates unchanged.
`sai-client` (G4 tools, 2026-11-26): +31, not in the total above (also `tl-frames`' 12).
143 blocks in three crates (+3 behind `test-port`), 2026-11-23 after G6: -88 with the two crates
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

In Rust, fixed (Teralynx, H2-R1, 2026-11-26): **R1**, found on the CS8210 (evidence
`ai_re/devices/cs8210-32x/evidence/rust-h2/06-ifcs-key-padding.txt`): the `ifcs` rim built the C structs it hands
libifcs as Rust struct values, whose padding is undefined; libifcs copies the by-value key members into its keys and
compares the raw bytes, so route / FDB deletes and replaces answered NOTFOUND and objects stayed busy. The mock
compares fields: invisible offline until the parity harness's byte tap. Now byte twins (Teralynx backend).

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
- The CS8210's default SAI is the Rust one since H4 (2026-11-27; `hardware.cs8210.sonic.saiImplementation`, "cpp" still
  selectable until H5). The Rust Teralynx libsai and shim passed H1 (smoke, a short SONiC run), H2 (the 37 feature scripts,
  Rust vs C++ equal step for step with the same tools, learn-ring guard injection equal; R1 found and fixed) and H3 (SONiC
  with BGP/BFD/LACP/ACL/sFlow, reboots, a cold cycle, a 128-minute soak, rollback) on the chip (device PROGRESS logs
  2026-11-26 (h)-(j), 2026-11-27 (a)). Not done: H5 (goldens from the C++, then its deletion), the LED panel replay (owner).
  Still unproven on the chip: the board's post-init register list beyond the register-level LED walk (the panel effect).
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
`sai-rs`: 566 passed, 0 failed (117 suites, with the real-stub runs). Y1 (MDIO claims, concurrent diag server,
non-waiting gate; 2026-11-23, x86_64): `cargo test --workspace` 586 passed, 0 failed (116 suites, the package's
develop environment); test-eval 219 passed, 19 xfailed; test-packages `-k 'sai or mdio or phy-seq'` 18 passed;
test-vm `-k 'openbcm or sai or mdio'` 10 passed; `sai-rs-parity` reproducible (`nix build --rebuild`).

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

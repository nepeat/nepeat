# sai-rs: a SAI in Rust

`libsai.so` (OCP SAI 1.19.2, the headers sonic-sairedis is pinned to) as a safe Rust
library that exports a C ABI. First slice, 2026-11-22: switch, ports, VLANs, VLAN members,
bridge ports on an in-memory ("virtual") backend. No hardware backend yet, nothing here
runs on a switch. The C++ `ai_re/nix/packages/sai-openbcm` stays the SAI the switches run.

What works, with the evidence (all on x86_64; commands under [Build and test](#build-and-test)):

- 48 Rust tests (unit, property, C-ABI lifecycle) and a C harness of 147 checks that loads
  the built `libsai.so` with `dlopen` and drives it with the real `sai.h`.
- SONiC's `syncd`, linked against this `libsai.so`, starts, creates the switch, discovers
  its objects and passes the slice through `libsairedis` in a NixOS VM ([syncd](#syncd)).
- Cross-builds for big-endian ppc64 (the AS7760 / AS7772 CPU); **not run there**.
- Miri (nightly): the rim's unit tests and the whole C-ABI lifecycle test, no undefined
  behaviour reported ([limits](#miri)).

## Where it lives, and why here

`rust/sai/`, its own Cargo workspace with its own lock file, like `rust/consoled`.

- Not in `rust/crates/`: that workspace shares one lock file and fand's package source is the
  whole workspace, so every edit here would rebuild fand and change every appliance's
  closure, and bindgen's dependency tree would land in fand's lock file.
- Not in `ai_re/nix/packages/`: Rust code of the monorepo lives under `rust/` with its Nix
  next to it (`rust/README.md`); `ai_re/nix` holds what is built for devices, and this is not
  built for one yet. `nix/package.nix` takes the SAI headers as an argument, so nothing here
  knows where SONiC is pinned. `ai_re/tests/nixtargets.py` wires the two together (targets
  `sai-rs`, `as7760-sai-rs`, `vm-sai-rs-syncd`). When a device uses it, the device's module
  imports `nix/package.nix` the way board modules import fand.

## Crates

```
 C caller (syncd)
   | C ABI: sai_api_initialize / sai_api_query / method tables
 sai-ffi   libsai.so. The only hand-written unsafe. Pointers, unions, callbacks, panics stop here.
   | Rust values (Value, typed ids, Error)
 sai-core  #![forbid(unsafe_code)]. Object ids, object store, attribute checks, the slice's
   |       objects, entry points, events -> notifications.
   | trait Backend (ports, VLANs: what a chip must do)
   +-- virt (in sai-core)   in-memory chip, fault injection
   +-- (later) a backend over the C SDK glue
 sai-meta  #![forbid(unsafe_code)]. Attribute tables generated from the SAI headers.
 sai-sys   bindgen output from the SAI headers + ABI checks. No hand-written code beyond two constants.
 mdio-script  #![forbid(unsafe_code)]. Port of sai-openbcm's MDIO script interpreter (first migration candidate).
```

The virtual backend is a module of `sai-core`, not a crate: it becomes one when a second
backend exists (the rule of `rust/README.md`: split when a second user needs it).

### sai-sys: the C ABI is the header's

- `build.rs` runs bindgen over `sai.h` from `$SAI_INC` (+ `../experimental`, which
  `saiobject.h` includes). In the Nix build `SAI_INC` is `SAI/inc` of the pinned
  sonic-sairedis source: the same headers syncd is compiled with, no network.
- Enums become integer constants (no Rust enum can hold an out-of-range value from C).
- Status codes are function-like macros bindgen cannot evaluate; the build script wraps them
  in a C enum so the C front end computes them. No number is typed by hand.
- **Layout check 1** (every target, at compile time): bindgen's size / alignment / offset
  assertions, clang's numbers against rustc's.
- **Layout check 2** (every build, native and cross): the same numbers and every enumerator
  as `_Static_assert`s, compiled with the target's C compiler (`SAI_SYS_ABI_CC`; in Nix the
  gcc syncd is built with). 7132 assertions; the build log names compiler and target, and
  with `SAI_SYS_ABI_CHECK=require` (set by the Nix package) a build without the check fails.
  Verified 2026-11-22 for `x86_64-unknown-linux-gnu` and `powerpc64-unknown-linux-gnuelfv2`.
- Exported functions are checked against the header's prototypes at compile time
  (`global.rs`, `prototypes_match_the_headers`); method tables are typed by the header's
  structs and list every field, so a header that grows an entry stops compiling.

### sai-meta: attribute metadata without the C metadata library

SAI's own metadata (`saimetadata.c`) only exists as a product of the sairedis build (perl +
doxygen over the headers), as C tables inside `libsaimetadata.so`: using it from Rust would
mean linking C and reading its structs through more unsafe code. The headers themselves
carry the source of that metadata as annotations (`@type`, `@flags`, `@objects`,
`@allownull`, `@condition`), so `sai-meta`'s build script reads those and writes Rust
tables: 2054 attributes of 125 object types. Ids are not counted by the script: the tables
name the `sai-sys` constants. Used for: which union member an attribute travels in (the
rim's decoder / encoder is one `match` on it, not one arm per attribute), create / set /
read-only rules, mandatory attributes, which object types an object id attribute accepts.

Limits, stated plainly: 272 attributes have a type the rim does not decode yet (ACL field /
action data, IP addresses and prefixes, maps, ...) and are answered "not supported";
conditions are recorded only as "is conditional" (a conditional mandatory attribute is left
to the object code); defaults and enum value ranges are not carried. The scanner is small
and is not SAI's parser: a test pins known attributes and the totals, and a header
change that confuses it fails the build (unknown constant) or that test.

### sai-core: the safe part

- **Typed ids** (`oid.rs`). 64 bits: object type 63..56, switch 55..48, index 47..0 (the C++
  layout). A raw id is decoded once into `PortId`, `VlanId`, ...; functions below the entry
  points take typed ids only. A typed id proves the type, not existence.
- **Store** (`store.rs`). Owns every object. Rules, checkable by `check_invariants`: an
  object can only be stored when everything it refers to exists; a referenced object cannot
  be removed (`OBJECT_IN_USE`); indices count up and are never reused, so a stale id never
  names a newer object. Lists (a VLAN's members, the switch's ports) are read from the
  store, not kept twice.
- **Attribute checks** (`attrs.rs`) by the metadata, before object code: unknown,
  read-only, duplicate, missing mandatory attributes, object ids of a wrong type or null.
  `CreateArgs::finish` refuses, by index, every attribute nobody implemented; a short "kept"
  list per object (stored and returned, no effect: callbacks, port MTU, ...) is explicit.
- **Failure rule**: backend first, store second. A failed backend call leaves model and chip
  as they were. The property test checks this with a backend that fails at random.
- **Stages**: `Sai::new` is API initialisation and touches nothing; `create_switch` brings the
  backend up, disables every front-panel port (SAI's default is admin down) and builds the
  default objects; a failure on the way shuts the backend down again.
- `Backend` (`backend.rs`): 15 operations in the spirit of `ob::Backend` (ports are chip
  logical ports, VLANs are `Vid`s that can only hold 1..4094). Backends report link changes
  by queueing on an `EventSink` and never call into the SAI layer.

Slice: SWITCH (create with INIT_SWITCH=true, remove; get PORT_NUMBER, PORT_LIST, CPU_PORT,
DEFAULT_VLAN_ID, DEFAULT_1Q_BRIDGE_ID, TYPE, OPER_STATUS; set/get SRC_MAC_ADDRESS, the
`*_NOTIFY` callbacks), PORT (get TYPE, HW_LANE_LIST, OPER_STATUS (live), OPER_SPEED,
SUPPORTED_SPEED; set/get ADMIN_STATE, SPEED; MTU kept), VLAN (create, remove, VLAN_ID,
MEMBER_LIST), VLAN_MEMBER (create, remove, tagging mode untagged / tagged), BRIDGE (the
default .1Q: TYPE, PORT_LIST), BRIDGE_PORT (type PORT: create, remove, ADMIN_STATE;
FDB_LEARNING_MODE kept). VLAN 1 exists with every port (the chip's default membership,
which sai-openbcm's README records orchagent as expecting to find and remove).

### sai-ffi: the rim, and all of the unsafe

One shape for every entry point (`generic.rs`): check pointers, decode by metadata, call
the core under the lock, encode, all inside `guard`. Every output pointer is checked
before anything is created, and written last.

Unsafe inventory (non-test code, 2026-11-22; `grep -c 'unsafe {'`): **29 blocks**, every one
with a `// SAFETY:` comment (clippy `undocumented_unsafe_blocks` and
`multiple_unsafe_ops_per_block` are errors). Six of them sit in macros and are expanded once
per union member (`convert.rs`) or per object type (`tables.rs`).

| file | blocks | what |
|---|---|---|
| `ptr.rs` | 5 | the pointer helpers below |
| `convert.rs` | 3 | reading a union member (macro), list in (macro), list out (macro) |
| `generic.rs` | 9 | forwarding the caller's contract to the helpers, per entry-point shape |
| `state.rs` | 2 | calling `profile_get_value`, reading the string it returns |
| `notify.rs` | 2 | address to function pointer, calling the port state callback |
| `tables.rs` | 4 | trampolines forwarding to `generic` (macros) |
| `global.rs` | 4 | exported functions forwarding to the helpers |

Plus 8 `#[unsafe(no_mangle)]` (the exported symbols). No `unsafe impl`, no `static mut`.
`sai-sys`: no block; 21 generated `unsafe extern "C"` declarations. `sai-core`, `sai-meta`,
`mdio-script`: `#![forbid(unsafe_code)]`.

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
`a_panic_under_the_lock_fails_every_call_until_uninitialize`). Needs `panic = "unwind"`
(set in the workspace profiles); with `abort` a panic ends syncd instead.

### Threads and notifications

- Every SAI call takes the one mutex for the time the core runs. No async runtime.
- Backends queue events (`mpsc`); one notifier thread per switch applies each to the model
  under the lock, **releases the lock**, then calls the user's callback. The callback may
  call back into the library (the C harness does).
- Profile callbacks (`profile_get_value`) are also called without the lock.
- `remove_switch` / `sai_api_uninitialize` close the queue and join the thread before
  returning: no callback after they return. A switch-generation number keeps an event of a
  removed switch from being applied to the next one.
- Only `PORT_STATE_CHANGE_NOTIFY` is ever called; other callbacks are stored and returned.

Profile keys: `SAI_RS_BACKEND=virtual` is required (the virtual switch is never chosen
implicitly, and refuses to start on a host with `/dev/linux-user-bde` or
`/dev/linux-kernel-bde` unless `SAI_RS_VIRTUAL_ON_HARDWARE=1`); `SAI_RS_VIRTUAL_PORTS` (32),
`_LANES` (4), `_SPEED` (100000), `_LINK` (`admin` / `none`). A value that does not parse is an
error, not the default.

## mdio-script

Port of `sai-openbcm/src/mdio_script.{c,h}` (format in the crate docs): `Script::parse` gives
typed operations, `Script::run` executes them over `trait MdioBus`. Clause 22 cannot be
expressed (`Dev` holds 1..31), and a `Script` value is the proof that the whole file parsed,
so "parse the whole file before touching a chip" is no longer a rule for callers. Not wired
into anything. Differences from C are listed in the crate docs (nothing runs before a
syntax error; no file path in messages; verify section taken from the script).

Tests: the C unit tests ported; property tests (parser never panics, round trips, run
statistics); a differential test that compiles the C interpreter (`MDIO_SCRIPT_C_DIR`) and
compares return code, statistics, messages, failure record and the bus trace over 85
hand-written scripts plus generated ones (7253 comparisons, no disagreement). It skips
with a message when the variable is unset; the Nix build sets it. One port defect was
found by a property test in the Nix build and fixed: a label the parser cuts at 150 bytes
inside a run of carriage returns (kept as C keeps it) did not survive `to_bytes`; the type
now says so and a regression test pins it.

## syncd

`nix/test-syncd.nix` (ai_re target `vm-sai-rs-syncd`): sonic-sairedis built with this libsai
as its vendor SAI, `syncd -SUu -z redis_sync` in a NixOS VM, `nix/sai_slice_smoke.py`
through libsairedis. Result 2026-11-22: passes. syncd links, initialises, creates the
switch, discovers 1 switch, 33 ports, 1 bridge, 32 bridge ports, VLAN 1 and its 32 members
into ASIC_DB; the client reads ports, lanes, speed, creates VLAN 100, sets a port admin up
and receives the oper-status notification back through syncd.

What syncd asked for and did not get (logged by syncd, survived):
`sai_api_query` of 75 other APIs; `SWITCH_HARDWARE_INFO`; the default objects
`DEFAULT_STP_INST_ID`, `DEFAULT_VIRTUAL_ROUTER_ID`, `DEFAULT_OVERRIDE_VIRTUAL_ROUTER_ID`,
`DEFAULT_TRAP_GROUP`, `ECMP_HASH`, `LAG_HASH`, the CPU buffer pools;
`VXLAN_DEFAULT_ROUTER_MAC`. sai-openbcm's own smoke test (`sai_smoke.py`) would stop at its
next step, `DEFAULT_VIRTUAL_ROUTER_ID`.

What syncd / orchagent need next, in order: virtual router + STP instance + trap group as
default objects; stats entry points (the tables' stats and bulk entries are null: a caller
that invokes one without checking crashes; which ones syncd calls unconditionally was not
checked, its flex counters were not started in the test);
`sai_query_attribute_capability` and friends (answer "not implemented" today); hostif (the
netdevs orchagent creates); queues / priority groups per port; FDB; then the L3 objects.
Not tried: orchagent on it, `saiplayer`, warm boot.

## Coexistence with the C++ sai-openbcm, and the way over

Nothing in the C++ tree changed. Both produce a `libsai.so` with the same soname and object
id layout; a syncd is linked against one of them.

Proposed order (each step ships alone and keeps the C++ tests green):

1. **MDIO script** (done here, unwired): next, the reporting around it (`mdio_report.c`:
   per-PHY verification, failure classes, status files) and the `openbcm-mdio-script` tool
   as a Rust binary over a `MdioBus` backed by the diag socket; the C tool stays until the
   Rust one passed the same VM test (`vm-openbcm-mdio-units`) and a run on the AS7772.
2. **External PHY table** (`ext_phy.h`: parser + sequence engine) and the MACsec table,
   as `forbid(unsafe_code)` crates with differential tests against the C++ parsers, linked
   into the C++ libsai as a static library behind a C header of a few functions (the reverse
   direction of the rim: Rust called from C++).
3. **Frontend object model**: grow this workspace's slice object type by object type
   against `tests/test_sai.cpp`, which is written against the C ABI and can run on either
   library; reaching parity on the fake / virtual backend is the gate.
4. **Hardware backend**: `impl Backend` over `sdk_glue.c`, which stays C (the only file that
   includes SDK headers, compiled with the SDK's own defines). Its interface is plain C
   functions; a `-sys` crate for it follows the `sai-sys` recipe.

## Not done, decided against for now

- No hardware backend, nothing deployed, no board module. Not run on ppc64 (cross-compiled
  and ABI-checked only): endianness is exercised by no test.
- Logging goes to stderr (level per API from `sai_log_set`); the C++ library logs to syslog.
- `sai_dbg_generate_dump`, capability and availability queries, object count / key queries:
  "not implemented". Bulk, stats, MDIO entries of the tables: null.
- One switch, `INIT_SWITCH=true` only (no attach, no warm boot). Ports cannot be created or
  removed (no breakout).
- Metadata conditions, defaults and enum ranges; the 272 undecoded attribute types.
- Attribute tables for all 125 object types are compiled in (the stripped `libsai.so` is
  1.2 MB on x86_64); trimming to implemented types is easy and was not worth it yet.
- No fuzz target; property tests cover the same entry points (`cargo-fuzz` would add a
  nightly-only tool to the build).
- The pinned-nixpkgs toolchain has no Miri; it was run from a rustup nightly, by hand.

## Miri

`cargo miri test -p sai-ffi --lib --test abi` (nightly 2026-11-21,
`-Zmiri-disable-isolation`): 9 tests pass, including the lifecycle test with the notifier
thread and the callback. It found one real defect, fixed: a read-only list attribute passed
to set had its list pointer read before the call was refused. Limit: the callback pointer
is kept in the safe core as an address (a raw pointer there would need `unsafe impl Send`),
so two integer-to-pointer casts remain, which Miri reports as "might miss pointer bugs"
for those pointers. The C harness is not run under Miri (it is C).

## Build and test

From `ai_re/` (builds and tests exactly what is in the working tree):

```sh
just test-eval                      # every target evaluates, incl. sai-rs, as7760-sai-rs, vm-sai-rs-syncd
just test-packages -k sai-rs        # native: fmt, clippy -D warnings, all tests, C harness, MDIO differential;
                                    # ppc64: cross build + both ABI checks
just test-vm -k sai-rs              # syncd on the Rust libsai (kvm; builds sairedis against it)
```

By hand, in the package's own build environment (cargo, clippy, rustfmt, libclang, the SAI
headers and the C MDIO sources set up as in the Nix build):

```sh
cd rust/sai
E='let f = builtins.getFlake "path:'$PWD'/../../ai_re/nix"; p = import f.inputs.nixpkgs { system = "x86_64-linux"; };
   sonic = import (f.outPath + "/packages/sonic") { pkgs = p; };
   in p.callPackage ./nix/package.nix { saiSrc = sonic.sources.sairedis; mdioScriptC = f.outPath + "/packages/sai-openbcm/src"; }'
nix develop --impure --expr "$E" -c cargo test --workspace
nix develop --impure --expr "$E" -c cargo clippy --workspace --all-targets -- -D warnings
nix develop --impure --expr "$E" -c cargo fmt --all --check
```

Outside Nix: `SAI_INC=<SAI/inc> cargo test --workspace` with libclang and a C compiler
around (`LIBCLANG_PATH`, `CC`).

## Dependencies

Run time: none besides `std` (`libsai.so` needs libc and libgcc_s). Build time: `bindgen`
(C headers to Rust, with `prettyplease` so the output does not depend on a rustfmt binary).
Tests: `proptest`. Lints: `[workspace.lints]` in `Cargo.toml` (clippy `all` + `pedantic` as
errors in CI, no `unwrap` / `expect` / `panic` / indexing in library code).

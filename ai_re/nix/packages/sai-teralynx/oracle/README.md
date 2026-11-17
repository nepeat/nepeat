# oracle: what IFCS calls does a SAI make?

isai 6.2.0 (Marvell's amd64 SAI, under [qemu-ipd](../../qemu-ipd/README.md)) is a reference by
behaviour: for any SAI call sequence, trace the libifcs calls isai makes and the ones our backend
makes, step by step, and diff them. No vendor code is read or copied; traces are local evidence
(`devices/cs8210-32x/analysis/oracle/`, git-ignored), summaries go to the docs
([sai-oracle.md](../../../../devices/cs8210-32x/sai-oracle.md)).

| file | what |
|---|---|
| `gen_tracer.py` | generates the interposer for one IFCS ABI: `--header ../build/gen/ifcs_api.h` (ours, generated in the build by `tools/gen_ifcs_api.py`: typed wrappers, bound to the real handle) or `--ctypes <isai pyctypes> --imports <libsai's ifcs_* imports>` (isai: generic register wrappers, `RTLD_NEXT`; 750 API functions; isai's own `ifcs_isai_ds_*` store and accessors with float / by-value struct arguments are left untraced) |
| `trace_rt.{c,h}` | the runtime: one line per top-level call, attribute arrays decoded with the ABI's layout (0.12.41: 136 B; 0.16.2: 224 B, bitmap-typed values when the caller set the bitmap) |
| `sai_script.cpp` | `sai-script PROFILE SCRIPT.json`: generic SAI client (builds against SAI 1.19 and isai's 1.16 headers), step markers into the trace |
| `oracle.py` | `diff OURS ISAI`, `show TRACE`, `summary TRACE`: attribute ids and counter ids to names (from each version's pyctypes, local), dynamic handles renamed by order of use, per-step diff, functions only one side calls |
| `box-run.sh` | box side: one script on one SAI as a transient unit; refuses unless only cages 1 and 16 hold modules; isai runs reload ipd with `legacy_abi=0` and always restore it |
| `run-oracle.sh` | workstation side: build (memory gate, 4 cores), `nix copy`, run ours and/or isai, fetch, diff |
| `scripts/*.json` | `perf` (link state, speed, counters), `temperature`, `traps` (SONiC's CoPP groups and traps), `traps-hw` (frames over the loop with `trap-frames`, CoPP counters, a policer); `acl` (aclorch-style tables, groups, entries, ranges, egress: the IFCS form), `hostif-counters` (IGMP/MLD/NEIGHBOR_MISS traps, every port stat id, CPU queues), `acl-hw` (ACL actions and matches, IGMP/MLD traps, system traps, counters, frames over the loop with `trap-frames` and `tools/raw_frames.c`), `hostif-create` (create-mode netdevs); `qos` / `mirror` / `sflow-genetlink` (isai's QoS, mirror, sFlow sequences), `qos-hw` (ours on the loop with `tools/qos_frames.c`: classification, shapers, DWRR / strict, mirror copies, sFlow samples), `lag-l2` (LAG, members, LAG in VLANs, FDB, flush, RIF on a LAG, hash; no traffic), `l2lag-hw` (CPU out of flood groups, learning/aging over the loop, teamd LACP between two LAGs with `l2lag-teamd.sh`, copied to `/run/l2lag/` on the box); `l3-basic` / `l3-vrf` / `l3-ns` / `l3-ecmpw` (routed copies seen through a loop-observer ACL: DMAC of the far hosts -> TRAP, since the CPU is in no flood group), `phasec-hw` (RIF counters / admin, glean, IPv4 options, hash objects, ACL ranges / TC / redirect / mirror / egress / in-place update, storm and flood control, RIF re-create), `phasec-recreate` (two switches in one process) |
| `default.nix` | the tracer + runner (+ `trap-frames`) for one ABI; built as `oracle-ours` / `oracle-isai` in qemu-ipd |

## Use

```sh
# from ai_re/, both SAIs (isai ~3 min under qemu), report in analysis/oracle/<script>-<time>/
MEMGATE=<memgate.sh> nix shell nixpkgs#python3 -c bash nix/packages/sai-teralynx/oracle/run-oracle.sh \
  nix/packages/sai-teralynx/oracle/scripts/traps.json both
# ours only, another profile on the box
PROFILE_OURS=/run/bench/traps.profile ... run-oracle.sh .../traps-hw.json ours
```

Profiles (on the box, tmpfs): `PROFILE_OURS` (default `/run/bench/native-error.profile`) and
`PROFILE_ISAI` (`/run/bench/isai-error.profile`), as the 2026-11-16 benchmark made them
([sai-qemu-hw-20261116.txt](../../../../devices/cs8210-32x/sai-qemu-hw-20261116.txt)). box-run adds
`SAI_TERALYNX_IFCS_TRACE` for ours. Attribute names for ours need the box's 0.12.41
`pyctypes/ifcs_ctypes.py` copied to `analysis/sai-teralynx/vendor012/` (`NAMES_OURS`).

## Script format

`{"name": "...", "steps": [ ... ]}`, a step is `{"op": ..., "desc": ...}`:
`create_switch` / `remove_switch`, `create` (`type`, `attrs`, `as`), `set` (`obj`, `attr`),
`get` (`obj`, `attr`/`attrs`, `as` keeps an oid or an oid list), `remove`, `stats` (`obj`, `ids`), `stats_cap` (`type`: the stat ids the SAI reports),
`create_route` / `set_route` / `remove_route` (`vr`, `prefix`), `create_neighbor` /
`remove_neighbor` (`rif`, `ip`), `foreach` (`list`, `as`, `steps`, `max`), `repeat`, `sleep`,
`exec` (a shell command while the switch is up; `$ORACLE` = the oracle store path), `diag` (one
command on `SAI_OPENBCM_DIAG_SOCKET`). An attribute is `["NAME", "type", value]`: types `bool u8 u16
u32 s32 u64 oid mac ip chardata oidlist u32list s32list u32range aclfield aclaction ptr`, and `s8`, `oids` (a list of objects: `["$a", "$b"]`), `qosmap` (`[["dscp", 46, "tc", 5], ...]`, fields tc dscp dot1p prio pg queue color); op `qstats` reads queue / PG / buffer pool counters; value names that several enums share (DYNAMIC, GENETLINK) go as numbers (aclfield `{"ip4": "10.0.0.0", "mask": "255.0.0.0"}`, `{"u8"|"u16"|"u32"|"s32": v, "mask": m}`, `{"ip6"|"mac": ...}`, `{"oids": [...]}`; aclaction `{"s32": "DROP"}`, `{"oid": "$c"}`); values by name for the enums in the runner's
table or numbers; objects as `"$name"`, `"$list[3]"`, `"switch"`. `may_fail` marks a step whose
failure is expected.

## Trace format

```
# ifcs-trace v1 abi=<abi> pid=<pid>
M step <depth>.<index> <op> <desc>                      (runner markers, also "M end ... status= ms=")
T <seq> <tid> <t_us> <dt_us> <depth> fn(arg, ...) = <REASON|value|ptr>
```
Arguments: integers, `0x` handles, `*v` single out-values, `[a,b]` lists, `[{id v=<first 16 value
bytes, hex>[ member=typed]}...]` attributes, `{hex}` structs, `ptr`/`fn`/`null`. Only top-level
calls (`IFCS_TRACE_NESTED=1`: also IFCS calling itself). `dt_us` is the call's duration: the
temperature stall was found with it.

# Teralynx 7 forwarding table capacity (CS8210-32X, vendor SDK V0.1.7)

No public datasheet gives route/FIB sizes for Teralynx 7 (the Innovium TL7
product brief only says "highly scalable/flexible layer 2 and 3 tables"). The
numbers below come from the SDK on this box, found offline by four parallel
agents on 2026-11-11. The detailed notes, with addresses, are local and
gitignored: `analysis/research/fib-size/{1-usage-api,2-forward-profiles,3-hw-tables,4-defaults-hints}.md`.
V = verified (disassembly, header constant or live log), I = inferred.

## Forwarding profiles (V)

The node attribute `IFCS_NODE_ATTR_FORWARD_PROFILE` (A=0 B=1 C=2 D=3 E=4 G=5
F=6) repartitions the shared table memory. In `libifcs.so`, `forward_program`
(0xa679c8) jump-tables to one setter per profile, and each setter writes a
22-field constant struct. The immediates match the
`FORWARD_PROFILE_<X>_*_MAX_ENTRIES` constants in `pyctypes/ifcs_ctypes.py`
(~L139440-139880):

| profile | v4 prefix* | v6 prefix* | v4 host | v6 host | MAC |
|---|---|---|---|---|---|
| **A (default)** | 73,728 | 36,864 | 98,304 | 48,128 | 45,056 |
| B (max host) | 24,576 | 12,288 | 196,608 | 96,256 | 4,096 |
| C (max LPM) | 294,912 | 147,456 | 49,152 | 24,064 | 4,096 |
| D | 204,800 | 102,400 | 49,152 | 24,064 | 20,480 |
| E | 147,456 | 73,728 | 98,304 | 48,128 | 20,480 |
| F | 24,576 | 12,288 | 49,152 | 24,064 | 45,056 |
| G | 147,456 | 73,728 | 49,152 | 24,064 | 4,096 |

\* The prefix columns are what the chip could do **with algorithmic LPM
(ILPM)**. This SDK build cannot use them (below).

## What this build can actually do (V)

- **The ILPM code is absent.** All six `ii_ilpm_*` symbols in `libifcs.so`
  are weak and undefined (`nm -D`: `w`, no address), and none are defined
  anywhere in the ONIE tree. The live route path is `ii_lpm_route_mod_*` plus
  `ii_tl_route_*`. No code reads the profile's prefix fields.
- **Prefixes are capped by the TCAM.** `ii_tl_route_usage_get_impl`
  (0x45cbc0) reports the v4 prefix max as the TCAM entry count and the v6
  max as half of it.
- **The live switch confirms the depth.** Boot log 2026-11-10 15:20:45:
  `TCAM Scrubber : IMT3008B_CAM_CTRL, depth: 24576` (and IMT3009B 12288).
- **Default profile is A, ILPM off, host-route-LPM off:** all three reset
  functions write 0. No vendor config sets them.

Effective capacity on this box today (profile A, this SDK):

| | entries |
|---|---|
| IPv4 prefixes | **24,576** (TCAM) |
| IPv6 prefixes | **12,288** (2 TCAM slots each, I) |
| IPv4 host routes (/32, ARP) | 98,304 (hash, best case) |
| IPv6 host routes | 48,128 (hash, best case) |
| MAC | 45,056 (hash, best case) |

- v4 and v6 prefixes most likely share the one TCAM (I).
- The TCAM may be split into areas by prefix length (warmboot field names
  `route_area_info`, `db_areas`; I), so the usable count can depend on the
  prefix mix.
- Hash tables never fill to 100%.

## Other findings

- **Selecting a profile:** only at node creation, through a YAML key
  `forward_profile: "IFCS_FORWARD_PROFILE_ID_PROFILE_X"` (the key string and
  accepted values are in libifcs at 0x147fd60, V). Its placement under the
  node options in the config is I. The CLI's `node create` passes no
  attributes, and the attributes are refused once the node is enabled.
- **Profile restrictions:** host-route-LPM requires profile D, and ILPM is
  rejected with B and F (libifcs strings, V).
- **The CLI under-reports prefixes:** `ifcs show route_entry usage filter
  v4_prefix` reports the TCAM count, not the profile's prefix max. To find
  the real limit, fill the table.
- **Table full:** creation returns `IFCS_RESOURCE_FULL` (13), and the SDK
  logs `LPM full!` or `Hash Table Full`.
- A newer Innovium/Marvell SDK with ILPM (e.g. from a SONiC TL7 build) would
  likely unlock the 74K-295K prefix range (I; we don't have one).

## Context

- A full IPv4 internet table is ~1M prefixes, so it does not fit even in
  profile C with ILPM.
- With two upstreams, FIB aggregation would leave roughly 300-600K
  (literature range, not computed).
- Selective install (the top-N prefixes by traffic plus a default route)
  fits easily.
- The home network's whole FIB is under 1,000 entries.

## Bench confirmation (management only, front ports empty: no LAN risk)

1. Run `ifcs show node` to confirm the active profile.
2. Read `ifcs show route_entry usage filter v4_prefix|v6_prefix|v4_host|v6_host`
   and the companion `nexthop`/`ecmp`/`l3vni usage`.
3. Fill test through the Python API, using `scripts/l3snake.py` (~L427-483)
   as the route-create template. Create routes until `IFCS_RESOURCE_FULL`,
   with several prefix-length mixes (uniform /24, a real BGP-dump shape,
   v4+v6 mixed) and host routes. Then repeat with `forward_profile` set to C
   and to D.

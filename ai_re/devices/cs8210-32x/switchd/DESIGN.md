# CS8210-32X mini-switchd: default forwarding, bring-up guard, Linux -> ASIC mapping

Status: **design + offline dry run only (2026-11-11). Nothing here has run on
the switch.** The switch was unplugged while this was written.

Tags used throughout:

- **(V)** verified from source: the cited vendor file does exactly this.
- **(I)** inferred: from API signatures, strings in `libifcs.so`, or how a
  neighbouring piece of vendor code behaves. Plausible, not confirmed.
- **(U)** unknown: needs the hardware. Collected in [section 6](#6-unknowns-that-need-hardware).

Paths: `SDK/` is
`analysis/onie-live-2026-11-10/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK/`
(vendor code, local only, not in git). `ipd/` is the Marvell GPL driver source
(`/nix/store/2yslnz4x…-source/drivers/ipd`). `nix/` is `nepeat/nix/nixos`.

Source note: `PROGRESS.md` was empty while most of this was written (commit
`1d813f4` truncated it), so (y), (x), (w), (u), (t) were read from `475c800`.
It was restored in `3a98985` and entry (z) was read afterwards; section 1.4
takes it into account.

Files in this directory:

| file | what |
|---|---|
| `DESIGN.md` | this document |
| `switchd_dryrun.py` | stdlib-only Python 3 generator + validator |
| `examples/*.json` | desired states |
| `examples/*.out.txt` | generated apply/teardown sequences with validation results |
| `examples/03-lag-bridge.ops_py2.txt` | the Python 2 module the generator emits for example 03 (never run) |
| `examples/selftest.out.txt` | port-mapper cross-check and validator negative tests |

---

## 1. What the ASIC forwards after `auto_load_user.sh -m … -k -a … -d`

### 1.1 What actually runs (V)

`cs8210-switch-up` calls `./auto_load_user.sh -m '$innv' -k $macarg -d`
(`nix/module/cs8210/vendor-sdk/cs8210-switch-up`). In daemon mode that script
does, in order (`SDK/auto_load_user.sh:205-232`):

1. `dynamic_config.sh "$portMode"` writes `cel_configs/dynamic_config.yaml`
   (devports) and `cel_cmds/dynamic_preemphasis.cmd`, which holds one
   `port enable N` (or `port disable N`) line per logical port
   (`SDK/dynamic_config.sh:278-292`).
2. `config.yaml` = `common_nd.yaml` + devports + ISG map. `-k` selects the
   `_nd` file, whose only differences are `netdev: - auto_create: "yes"` and
   `netdev: "true"` on each rx ring (diff of `cel_configs/common.yaml` and
   `common_nd.yaml`). There is **no VLAN, L2, STP, LAG or trap configuration in
   any yaml**.
3. **Pass 1** (`auto_load_user.sh:214-215`): `echo exit >> misc_config.cmd`,
   then `python2.7 ifcsshell.py -e switch -c config.yaml -i misc_config.cmd`.
   That is a complete, throw-away SDK instance: `ifcs_init(config, COLD)`
   (`SDK/ifcs_cli.py:91-106`), `node create 0`, serdes settings, **every
   `port enable N`**, then `exit`. `do_exit` just returns (`SDK/cli.py:189-190`);
   there is no `ifcs_deinit`. The ASIC is left exactly as pass 1 programmed it.
4. **Pass 2**: `innovium.user config.yaml &`, `sleep 10`, then
   `cls_shell source misc_config.cmd` (`auto_load_user.sh:216-228`): the same
   serdes lines and `port enable N` again, and as the **last** line
   `diagtest knet config -a <mac>` (appended at `:223`).

So during pass 1, during the gap between the passes, and during pass 2 until
the knet line finishes, front ports are admin-enabled with only the SDK's
built-in default L2 state. What that default is, is not in any script (U1).

### 1.2 Steady state after `diagtest knet config` (V, `SDK/cmds/knet.py:116-280`)

For N front devports (35 with today's spec) plus the CPU port (devport 0):

| object | what knet creates | lines |
|---|---|---|
| mc_dest | one per port including CPU, `LOCAL_DESTINATION` = that sysport | 164-173 |
| STP | **one** instance, every port added in state `FORWARDING` | 175-191, 232-242 |
| MDG (flood group) | one per front port `i`, members `{mc_dest[cpu], mc_dest[i]}` | 193-199, 224-230 |
| L2VNI | one per front port `i`, **VLAN id = i + 1**, with the STP instance and flood group `mdg[i]`; members `{cpu sysport, sysport i}` | 201-222 |
| sysport `i` | `DEFAULT_CVID = i+1`, `EGRESS_UNTAGGED_VLAN_ID = i+1`, enable | 244-257 |
| netdev | `inno_0_NNNN` renamed to `fpp<cage>` / `fpp<cage>-<k>`, MAC = base + i | 259-278 |

Consequences:

- **Every front port is alone with the CPU in its own L2 domain.** No L2VNI
  and no flood group contains two front ports. By construction the ASIC has
  nowhere to forward a frame from one front port to another (V for the object
  graph; that the hardware honours it is I, see U2).
- All ports are STP `FORWARDING`; there is no blocking anywhere (V).
- Learning: nothing in the bring-up path touches `learn_enable` or the
  learn/age mode; `l2learn info` reports mode from
  `IFCS_NODE_ATTR_L2_ENTRY_NOS_LEARN_AGE_MODE` (`SDK/cmds/l2learn.py:44-60`).
  The `ifcs-learn` thread is live on this box (PROGRESS (n), (y)), so SDK-side
  learning is on by default (I).
- Trap / hostif rules: the bring-up path creates **none**. `knet.py` makes no
  `ifcs_hostif_*` call; `ifcs_hostif_trap_create` appears only in diag
  commands that are not run (`cmds/diagtest.py`, `cmds/pktcmd.py`,
  `cmds/netdev_test.py`). Frames reach the CPU because the CPU sysport is an
  L2VNI member and its mc_dest is in each flood group (V), plus whatever the
  SDK's netdev auto-create programs for the netdev MACs (U3; observed in
  PROGRESS (t): unicast is delivered only for the base MAC and the per-port
  fpp MACs).
- CPU side (V, `ipd/inno_enet.c`): RX picks the netdev by source sysport
  (`:1322-1355`); TX defaults to `PIPELINE_BYPASS` with the netdev's sysport as
  destination (`:240-256`), i.e. CPU frames skip the L2 lookup entirely.

One residual question in steady state: a **tagged** frame whose VID equals
another port's private VLAN (2..N+1). Port `j` is not a member of L2VNI `i+1`,
and the SDK has a drop reason `ING_VNI_MEMBERSHIP_DROP`
(`SDK/pyctypes/ifcs_ctypes.py`, `IFCS_HOSTIF_TRAP_ING_VNI_MEMBERSHIP_DROP = 11`),
so ingress membership filtering exists (V) and is presumably on (I, U2). If it
were off, a trunk carrying VLANs 3..6 into cage 2 would leak between the
breakout legs.

### 1.3 What `-s` (singleL2Domain) changes (V)

`-s` appends `diagtest integrator_mode config -v`
(`auto_load_user.sh:225-227`). `SDK/cmds/integrator_mode.py:142-231` then
creates **one L2VNI, id 130, with every front port as a member**, one flood
group holding every front port's mc_dest (the CPU is not in it), all ports STP
`FORWARDING`, and PVID 130 everywhere. That is a flat, unprotected switch: two
ports cabled to the same neighbour **will** loop. The vendor's own
`integrator_mode` wrapper uses this mode unless `-k` is given, and relies on
sending a BPDU every 10 s (`polling_tx_bpdu.sh`) so the far side can block.
`cs8210-switch-up` never passes `-s` (V).

### 1.4 Can two front ports loop?

| state | port-to-port forwarding possible? | basis |
|---|---|---|
| steady state, knet mode (today) | **No by configuration.** Each port only shares an L2VNI and flood group with the CPU. | V (object graph), I (hardware honours it) |
| steady state with `-s` | **Yes**, flat L2 domain. Not used here. | V |
| pass 1 / gap / pass 2 before knet (about 10 s + init time, ports enabled, default L2 state) | **Cannot be excluded from source.** No script creates a shared domain, and the vendor L2 samples have to create L2VNI + members + STP themselves before anything forwards (`SDK/scripts/test_l2.py:63-151`), which suggests the default forwards nothing. | I, U1 |
| while `knet config` runs | No shared domain is ever created; the PVID is set last, so for a moment ports are members of their L2VNI but still on the default PVID. | V (order), U1 for the default PVID |
| SDK stopped (`cls_shell exit` = `kill -9`, `SDK/cls_shell:27-31`; also `cs8210-switch-up --down`, watchdog restart, crash) | ASIC keeps its last tables **and its links stay up**: no deinit, no port disable anywhere on that path. Still port<->CPU only, but nothing consumes the CPU queue and nothing answers LACP. | V |
| CPU soft-locked while the SDK re-initialises (last night) | The chip is somewhere inside a cold `ifcs_init` with links possibly still up from the previous instance. Not derivable from source. | U4 |

What entry (z) adds: the soft lockup was in the SDK's `PORT_BRUP` thread
inside an ipd ioctl, during `systemctl restart cs8210-switch`. A port
bring-up thread is only busy once ports have been admin-enabled, and in the
vendor sequence every `port enable N` comes **before** `diagtest knet config`
(1.1). knet's object creation needs no rtnl, but its netdev renames do
(`os.system("ip link set …")`, `knet.py:259-278`), and rtnl was held by the
stuck thread. So at the moment of the hang the chip had ports enabled and
links coming up, with either the SDK-default L2 state (the knet line not
reached yet) or knet's isolation already programmed; which one is not
recorded (I). That is exactly the window in the third row above, and U1 is the
question that decides whether the "ASIC bridged the two legs" hypothesis is
even possible.

**Answer:** in the configured knet state the Teralynx should not forward
between two breakout legs, so a plain "the ASIC bridged leg 1 to leg 2" loop is
not supported by the source. The source does show three things that fit last
night better and that the guard below removes:

1. Ports are enabled **before** any isolation exists, twice per start (pass 1
   and pass 2), and stay up across the gap between two SDK instances. Last
   night's hang happened at this stage of a restart.
2. Nothing ever disables ports on stop/crash, so a dead or half-initialised
   ASIC keeps both legs' links up toward the Unifi with no LACP partner behind
   them. What the Unifi does with two "up, no LACP" members of an aggregate is
   on its side (PROGRESS (u) already saw it flood ~30k frames/s at an
   unaggregated leg).
3. Pause frames: the vendor's own integrator notes require "No TX/RX Pause
   Frames" (`SDK/enablePacketForwarding.sh` usage text). Nothing in the
   bring-up path configures flow control, and the devport has
   `RX/TX_FRAMES_PAUSE` counters (`SDK/ifcs_cmds/devport.py:583,646`). If the
   default is pause-enabled, a stuck CPU queue could make the ASIC emit pause
   toward the Unifi, which takes links down without any loop (U5; check the
   counters first next time).

---

## 2. Safe bring-up guard

Goal: no front port carries traffic until isolation is verified and the port is
explicitly enabled; and a dead SDK never leaves links up.

### 2.1 Start with every port admin-down (config-level, no race)

Append the vendor's own option `down-pall` to every entry of `innoviumPorts`:

```
1:copper_1x400G:fec-def:down-pall;2:copper_4x10G:fec-def:down-pall;3-16:copper_1x400G:fec-def:down-pall;17-32:optics_1x400G:fec-def:down-pall
```

- `dynamic_config.sh:63-93` takes any option containing `p` as the admin
  option; `:278-292` then writes `port disable N` instead of `port enable N`
  for every logical port (V). Both init passes source that file, so **neither
  pass ever enables a port**.
- `knet.py:39-78` (`devPort2fpp`) also reads the option from the `# mode=`
  line and sets the netdev down (`ifconfig fppN down`, `:277`) (V). networkd
  will bring the netdev up again; that only affects the CPU side.
- Why this is enough: devports are admin-down until told otherwise. The
  vendor writes an explicit `port enable` for every port, and devport 129 on
  this box stays down until `cs8210-switch-up` runs `port enable 129` (V for
  129, I for the others, U6).
- `credoPorts` does not need the option. `set_speeds` in `cs8210-switch-up`
  parses only the first two fields, so the extra field is harmless (V).

`switchd_dryrun.py` prints this string as step A01 of every guarded plan.

### 2.2 After `auto_load_user.sh` returns: verify, then enable by allow-list

Post-init CLI, all forms validated (section 5):

```
cls_shell 'port disable 1-35'          # belt and braces; port.py:428-498 -> ADMIN_STATE false
cls_shell 'diagtest knet show'         # every L2VNI must list exactly cpu + one port
cls_shell 'ifcs show stp'              # one instance expected
cls_shell 'ifcs show lag'              # none expected
…apply desired state (section 3)…
cls_shell 'port enable 2-3'            # only ports the desired state names
```

The isolation check is a parse of `diagtest knet show` / `ifcs show l2vni <id>
members`: fail closed (leave everything down) if any L2VNI has two front-port
members that the desired state did not ask for.

### 2.3 Stop path: ports down before the SDK goes away

Add `cls_shell 'port disable 1-<N>'` to `cs8210-switch-up --down` and to the
watchdog restart path, **before** `cls_shell exit` (which is a `kill -9`).
That covers orderly stops only.

### 2.4 Races and windows that remain

| window | covered? |
|---|---|
| pass 1, gap, pass 2 before knet | Yes, by 2.1 (ports never enabled). |
| inside `knet config` | Yes, ports are down. |
| orderly stop | Yes, by 2.3. |
| SDK killed, crashed, or CPU soft-locked | **No.** No software guard runs. Needs a hardware-level action that does not depend on the SDK: ASIC reset or data-plane power-off (`cs8210-switch-power`) from `ExecStopPost=` / `OnFailure=`, and a hardware watchdog for the soft-lockup case (U7: whether a watchdog reset also resets the ASIC). |
| ports left up by an earlier instance when a new `ifcs_init` starts | Only if the previous stop went through 2.3. After an unclean stop, power-cycle the data plane before starting (same hook as above). |
| `port enable` races the apply | No: apply is fail-closed. The emitted module raises on the first failed mandatory step, and `port enable` is its last statement. |

Independent of this box: storm control / loop protection on the Unifi ports,
and LACP without "individual" fallback, would have contained last night.

---

## 3. Linux -> ASIC mapping

### 3.0 The vendor CLI cannot create objects

`ifcs` has exactly these verbs: `show enable disable clear set list help ?`
(V, `SDK/cmds/ifcscmd.py:384-393`). There is no create/delete/add-member. The
top-level commands are `verbosity node pen config diagtest port pci pkt
privilege reload l2learn blkpen errors ifcs debug debugparser` plus the
built-ins `source run shell redirect console exit` (V, `SDK/ifcs_cli.py:153-176`,
`SDK/cli.py`).

`ifcs set <obj> <id> <attr> <value>` works only for attributes whose setter
converts the value with `int()` (V, e.g. `sysport.py:1963-1967`). Handle-typed
attributes get the raw CLI string assigned to a ctypes field
(`sysport.py:1821-1825`, `setLag`), which cannot work (I: ctypes TypeError,
caught by the blanket `except` in `ifcscmd.py:430-444`).

So object creation has to be Python 2 running **inside** `innovium.user`, the
way `diagtest knet config` is. The supported hook is the `run` built-in:
`cls_shell 'run <module> <args>'` -> `run_script()` does `__import__(module)`,
`reload`, `module.main(args)` (V, `SDK/cli.py:22-33, 209-218`). The module must
be on the daemon's `PYTHONPATH`, which `auto_load_user.sh:212-213` extends
(`cmds/ utils/ testutil/ scripts/ ifcs_cmds/ pyctypes/ …`) while keeping
whatever was already set, so either drop the module in `SDK/scripts/` inside
the chroot or export `PYTHONPATH` in `cs8210-switch-up` (V for the mechanism;
that the daemon-side `run` behaves like the standalone one is I, U8).

The knet handles (per-port flood group, mc_dest, the STP instance) live in
class attributes of `Knet` in the same process (`knet.py:83-89`), so a module
that does `from knet import Knet` can reach them (I: same module object as
`diagtest.py:28` imports).

### 3.1 Linux bond (802.3ad) -> hardware LAG

No vendor script uses the LAG API, so everything below except the call
signatures is I.

| step | form | tag |
|---|---|---|
| detach each member from knet's private plumbing | `ifcs_mdg_mc_dest_remove(0, Knet.mdg_hdl[d], 1, [Knet.mcdest_hdl[d]])`; `ifcs_mc_dest_delete(0, Knet.mcdest_hdl[d])`; `ifcs_l2vni_member_remove(0, L2VNI(d+1), 1, [sysport d])` | required: `libifcs.so` refuses a member that "is part of N L2VNI(s) / L2_Entrie(s) / mc_dest(s)", is the CPU port, or is already primary in another LAG (V, strings) |
| create | `ifcs_lag_create(0, &h, 0, NULL)` | I |
| members | `ifcs_lag_member_add(0, lag, n, [sysports], 1, [IFCS_LAG_MEMBER_ATTR_TYPE = IFCS_LAG_MEMBER_TYPE_PRIMARY])` | I |
| flood destination for the LAG | `ifcs_mc_dest_create(0, &h, 1, [IFCS_MC_DEST_ATTR_LOCAL_DESTINATION = lag])` | I (a string mentions "mdg … mcdest … lag doesn't exist or has only secondary members") |
| standalone bond (L3 on the CPU, today's bond0) | LAG joins the first member's private L2VNI: `ifcs_l2vni_member_add(0, L2VNI(d1+1), 1, [lag])`, `ifcs_mdg_mc_dest_add(0, Knet.mdg_hdl[d1], 1, [mcd_lag])`; other members get PVID `d1+1` via `ifcs set sysport <d> default_cvid …` | I; CLI form V |
| bond as bridge port | LAG handle used wherever a sysport would be (3.2) | I |
| show | `cls_shell 'ifcs show lag'`, `ifcs show lag <id> members` | V |

**Hashing.** The lag object has only `l3intf_handle`, `type`, `member_count`
(V, `SDK/ifcs_cmds/lag.py:45-63`); there is no per-LAG hash attribute. Node
attributes `symmetric_hash`, `entropy_seed`, `hw_hashtable_seed` exist (V,
`node.py` set methods) and a string says "LAG Hash select val %d (valid range
0-3)" (U9). Linux `xmit_hash_policy` therefore cannot be mirrored exactly:
ASIC-forwarded traffic uses the ASIC hash, CPU-originated traffic keeps using
the bond's hash because ipd transmits `PIPELINE_BYPASS` straight to the member
sysport the bond picked (V, `ipd/inno_enet.c:240-256`).

**LACPDUs.** They must keep arriving on the *member* netdev for the bonding
driver. Today they arrive because the CPU is in each port's flood group (V,
and PROGRESS (u) saw a partner MAC). After a port becomes a LAG member:

- standalone LAG: the L2VNI still only holds `{cpu, lag}`, so LACPDUs are
  flooded to the CPU as before (I);
- LAG in a bridge: `01:80:c2:00:00:02` would be flooded to the other bridge
  ports too. The plan installs static L2 entries for `01:80:c2:00:00:00/02/0e`
  with destination = CPU sysport in the bridge L2VNI (U10: whether a group MAC
  is accepted as an L2 entry key; the alternative is an ACL or the
  `mac_only` key type with a mask).
- which netdev ipd delivers to depends on the source sysport the ASIC reports
  for a LAG member (member or LAG?) (U11).

**LACP state -> membership.** mini-switchd watches the bond
(`/sys/class/net/<bond>/bonding/slaves`, per-slave
`bonding_slave/ad_actor_oper_port_state` collecting+distributing bits, or
RTM_NEWLINK) and calls `ifcs_lag_member_add` / `ifcs_lag_member_remove` so the
ASIC only hashes onto members that are in sync. A member that drops out of the
LAG must get its knet private plumbing back (the restore steps in the teardown
plan) so its LACPDUs still reach the CPU.

### 3.2 Linux bridge + VLANs -> L2VNI / MDG / STP / sysport attributes

This SDK's L2 objects, as used by knet and integrator mode (V for the forms):

| Linux | SDK | form | tag |
|---|---|---|---|
| bridge VLAN `v` | L2VNI with handle value `v` | `ifcs_l2vni_create(0, &L2VNI(v), 2, [STP_INSTANCE = stp, FLOOD_GROUP = mdg])` | V (`knet.py:201-214`) |
| flooding (BUM) | MDG + mc_dest | `ifcs_mdg_create`; `ifcs_mdg_mc_dest_add(0, mdg, n, [Knet.mcdest_hdl[0], Knet.mcdest_hdl[d]…, mcd_lag], 0, NULL)` | V (`knet.py:193-199, 224-230`) |
| port membership | L2VNI member | `ifcs_l2vni_member_add(0, L2VNI(v), n, [cpu, sysports…, lag], 0, NULL)` | V; LAG handle I |
| untagged/PVID | sysport attrs | `ifcs set sysport <d> default_cvid <v>`, `… egress_untagged_vlan_id <v>`, `… egress_untagged_vlan_id_enable 1` | V (CLI and `knet.py:244-257`) |
| tagged member | member of the L2VNI with `egress_untagged_vlan_id` left on another VLAN | — | I (U12) |
| STP port state | one STP instance per bridge | `ifcs_stp_create`; `ifcs_stp_port_add(0, stp, n, […], 1, [STP_STATE = BLOCKING])`; later `ifcs_stp_port_attr_set(0, stp, port, 1, [STP_STATE = FORWARDING])` | V (`knet.py:175-191, 232-242`); LAG as STP port U13 |
| FDB static | L2 entry | `ifcs_l2_entry_create(0, key{MAC_L2VNI: mac, L2VNI(v)}, 2, [ENTRY_DEST = sysport/lag, ENTRY_TYPE = STATIC])` | V (`scripts/test_l2.py:156-186`) |
| FDB show | | `cls_shell 'ifcs show l2_entry'` (vendor `knet-l2-show`) | V |
| learning on/off | attrs | `ifcs set sysport <d> learn_enable 0|1`, `ifcs set l2vni <v> learn_enable 0|1` | V (int setters) |

Order used by the generator: STP instance (members `BLOCKING`) -> MDG -> L2VNI
-> members -> flood group -> PVIDs -> static entries -> **STP `FORWARDING`** ->
**`port enable`**. Forwarding only exists after the last two steps.

**VLAN id collision.** knet takes VLAN `devport + 1` for every front port, so
VLANs 2..36 are occupied with today's spec; the task's example VLAN 10 is
devport 9's (fpp6) private VLAN. The generator refuses such a bridge
(`examples/04-vlan-collision`). Options: use VLANs above N+1, or stop using
`diagtest knet config` and have mini-switchd build the per-port CPU plumbing
itself in a high range (e.g. 3900 + devport), plus the netdev rename and MAC
that `knet.py:259-278` does. The second is the cleaner long-term design.

**FDB learning, aging, kernel sync.**

- Hardware learns and ages by itself in "SDK" mode (I; mode shown by
  `l2learn info`, `l2learn.py:44-68`). Node attributes
  `l2_entry_aging_interval`, `l2_entry_nos_learn_age_mode`, `learn_enable`
  exist, but their CLI setters pass the raw string (V, `node.py:2301-2317,
  2445-2449`), so they need the API (`ifcs_node_attr_set`).
- Hardware -> kernel: poll `ifcs_l2_entry_get_all` (callback API, used by
  `l2_entry.py:562-580`) every few seconds, diff, and mirror dynamic entries
  as `bridge fdb replace <mac> dev <port> master extern_learn vlan <v>`;
  delete them when they age out of the ASIC.
- Kernel -> hardware: the bridge's own MAC and `bridge fdb … static` entries
  become static L2 entries (destination CPU sysport for local MACs).
- `l2learn flush` clears the SDK learn DB (V, `l2learn.py:70-72`); whether
  that removes hardware entries is U14.

**Coexistence with the knet netdev path.** The CPU sysport is a member of the
bridge L2VNI and its mc_dest is in the flood group, so broadcast, unknown
unicast and multicast reach the CPU and are delivered to the netdev of the
ingress port (V for the mechanism in knet's own per-port domains). Two things
follow:

1. **Double forwarding.** If `fpp3` and `bond0` are also ports of a Linux
   bridge, the kernel will forward every flooded frame again, and ipd will
   send it out with `PIPELINE_BYPASS`. The ASIC already flooded it, so the far
   side sees duplicates and, with two legs to one neighbour, a real loop. ipd
   has no `offload_fwd_mark` / switchdev support (V: no occurrence in
   `ipd/*.c`). **Before a Linux bridge may sit on hardware-bridged ports, ipd
   needs a patch** that sets `skb->offload_fwd_mark` on RX and gives all fpp
   netdevs one `ndo_get_port_parent_id`. Until then the bridge must exist only
   in the ASIC (desired-state JSON is the source of truth) with Linux holding
   an L3 interface for the VLAN.
2. **Known unicast to the CPU.** PROGRESS (t) showed unicast reaches the CPU
   only for MACs the SDK programmed. For a bridge VLAN the generator adds a
   static L2 entry `(bridge MAC, L2VNI v) -> cpu` when the JSON gives a `mac`
   (I, U3).

CPU TX is unaffected: `PIPELINE_BYPASS` to a sysport does not consult L2
tables. Sending into the pipeline instead (`INNO_TX_TYPE_PIPELINE_LOOKUP`,
`ipd/inno_enet.c:240-242`) would let the ASIC pick the LAG member / do the L2
lookup for CPU frames; not needed for a first version.

### 3.3 L3, briefly

The vendor L3 snake shows the full chain (V for forms,
`SDK/scripts/l3snake_cel.py`):

| Linux | SDK call | attrs | lines |
|---|---|---|---|
| VRF | `ifcs_l3vni_create` (`IFCS_HANDLE_L3VNI(n)`) | — | 335 |
| routed port | `ifcs set sysport <d> port_fwd_mode 1` (`L3_ONLY`) | CLI int setter, V | 283 |
| L3 interface | `ifcs_intf_create` | `TYPE = IFCS_INTF_TYPE_L3PORT`, `FORWARDING_INSTANCE = l3vni`, `TRANSPORT_INSTANCE = sysport`, `TRANSPORT_MAC`, `IPV4_UNICAST_ENABLE` | 361-383 |
| neighbour | `ifcs_nexthop_create` | `INTF`, `DEST_MAC`, `LOCAL_DESTINATION` | 405-419 |
| route | `ifcs_route_entry_create` (key `IP_DEST_L3VNI`: prefix + l3vni) | `NEXTHOP`, `FWD_POLICY = IFCS_FWD_ACTION_FORWARD` | 471-479 |
| show | `cls_shell 'ifcs show intf'`, `'ifcs show nexthop'`, `'ifcs show route_entry'` | | V (objects in `ifcs_cmds/all.py`) |

An SVI on a bridge VLAN would use `IFCS_L2VNI_ATTR_L3INTF` (attribute exists,
V; usage I). Routes to the CPU and host routes would follow the kernel FIB via
netlink. The generator does not emit L3 yet. Capacity limit for any FIB
sync: this SDK build has no ILPM and runs forward profile A, so prefixes are
TCAM-only, 24,576 IPv4 / 12,288 IPv6 (see [../fib-capacity.md](../fib-capacity.md)).

---

## 4. `switchd_dryrun.py`

```
./switchd_dryrun.py STATE.json --ports '<innoviumPorts>' [--sdk DIR] [--emit-py2 FILE]
./switchd_dryrun.py --selftest
```

Desired state:

```json
{
  "guard": true,
  "lags":    [{"name": "bond0", "members": ["fpp2-1", "fpp2-2"]}],
  "bridges": [{"name": "br0", "vlan": 100, "ports": ["bond0", "fpp3"],
               "tagged": [], "mac": "02:00:00:00:00:01"}],
  "enable":  ["bond0", "fpp3"]
}
```

`enable` defaults to every port named in a lag or bridge. `port_spec` may be
given in the JSON instead of `--ports`.

Port mapping follows `dynamic_config.sh` and `knet.py:devPort2fpp`: entries in
written order, one logical port per breakout leg, devport = sysport = running
counter from 1, netdev `fpp<cage>` or `fpp<cage>-<k>`, knet VLAN = devport + 1,
CPU = devport 0 / sysport 1000. With today's spec cage 2 (4x10G) is devports
2-5 and cage 3 is devport 6.

Output: port map, numbered APPLY and TEARDOWN steps (`pre` = configuration
change, `cli` = `cls_shell` line, `api` = `ifcs_*` call), each with a V/I/U
tag, a note, and its validation result. `--emit-py2` writes one Python 2
module that performs the same steps in order through the API (`cli` steps are
translated to the equivalent `ifcs_sysport_attr_set` / `ifcs_devport_attr_set`
calls, exactly what the CLI handlers do), stops at the first failed mandatory
step, and keeps created handles in `/tmp` keyed to the SDK's pid for teardown.

Examples (all with today's `innoviumPorts`):

| example | result |
|---|---|
| `01-guard-only` | 2 cli steps, the guarded port spec |
| `02-lag-standalone` | bond0 = fpp2-1 + fpp2-2 as a hardware LAG, still CPU-only: 15 cli + 24 api |
| `03-lag-bridge` | bond0 + fpp3 bridged in VLAN 100: 27 cli + 41 api, plus the emitted module |
| `04-vlan-collision` | the task's literal example (VLAN 10): refused, exit 2 |

---

## 5. How the generated commands were validated

All offline, against the vendor sources read in place (Python 3.14; the vendor
code is Python 2 and is never imported as a whole).

| what | how | strength |
|---|---|---|
| port mapping | `--selftest` parses the `# mode=` line of the vendor's own `cel_configs/dynamic_config.yaml` (128 devports), runs the mapper, and compares id / lanes / serdes group / speed / sysport for every devport; also compares the lanes table with `dic` in `dynamic_config.sh` | identical output |
| `ifcs set` / `ifcs show` lines | the object's **own** `preParse()` and `argparse` set-up are cut out of `ifcs_cmds/<obj>.py:parseArgs` at run time and executed (only `cmd is 'x'` is rewritten to `==`); then: verb exists in `Ifcs.sub_cmds`, object is in `ifcs_objs_dict`, attribute is in `set_methods`, the setter converts with `int()`, value fits the attribute type from `attrlist`, a show handler exists | real vendor parser |
| `port`, `diagtest knet`, `l2learn` lines | sub-command exists in the class's `sub_cmds` table; port list syntax as `port.py:428-460` splits it; devports exist in the port map | table lookup |
| top-level command | name registered in `ifcs_cli.py` / `cli.py` | table lookup |
| `api` steps | function has an `.argtypes` entry in `pyctypes/ifcs_ctypes.py` with the argument count the generator uses; every `IFCS_*` constant and handle macro exists; the `ifcs_attr_t.value` field exists; `Knet.<attr>` exists in `cmds/knet.py` | signature level |
| emitted Python 2 module | compiles (py2/py3 common syntax subset); every `ifcs_*` / `IFCS_*` identifier resolves in `ifcs_ctypes.py` | static |
| the validator itself | 13 deliberately wrong commands/calls must be rejected (`examples/selftest.out.txt`) | all rejected |

**Not validated, cannot be offline:**

- that any I/U step is accepted by `libifcs` (return codes), especially every
  LAG call and "LAG handle as L2VNI member / STP port / mc_dest destination";
- ordering constraints inside the SDK beyond the ones its error strings name;
- that `run <module>` works through the remote shell into the daemon;
- the emitted module under a real Python 2 interpreter (none on this host);
- `postParse()` of the CLI objects (needs `ifcs_ctypes`; it only wraps the id
  in a handle macro);
- anything about what the hardware does with the resulting tables.

---

## 6. Unknowns that need hardware

Read-only first (`ifcs show …`, counters), with the front ports unplugged or
admin-down.

| id | question | how to settle it |
|---|---|---|
| U1 | SDK default L2 state before knet: default PVID, any default L2VNI/flood domain, default STP state | start with `down-pall`, skip knet (`-k` off or stop before it), `ifcs show l2vni`, `ifcs show sysport <n>`, `ifcs show stp`, `ifcs show mdg` |
| U2 | Is ingress VNI membership filtering enforced for tagged frames? | send a frame tagged with another port's private VID; watch `ifcs show devport <n> stats` and the `ING_VNI_MEMBERSHIP_DROP` counter (`ifcs show hostif_trap`) |
| U3 | How unicast to the netdev MAC reaches the CPU (what netdev auto-create programs), and why unknown unicast is not flooded to the CPU | `ifcs show l2_entry`, `ifcs show netdev`, `ifcs show l2vni <id>` (`l2_dst_miss_ctc`) |
| U4 | ASIC behaviour while `ifcs_init` runs with links left up by a previous instance | cannot be tested safely with the Unifi attached; avoid the state instead (2.3, 2.4) |
| U5 | Default pause / flow-control behaviour; did the ASIC send pause last night | `ifcs show devport <n> stats filter nz` -> `TX_FRAMES_PAUSE`, and the Unifi's port counters |
| U6 | Are all devports admin-down after init until `port enable`? | `ifcs show devport <n>` (`admin_state`) after a `down-pall` start |
| U7 | Does a CPU watchdog reset / `cs8210-switch-power` drop the ASIC links | measure with a loopback cable, not the Unifi |
| U8 | `cls_shell 'run <module> <args>'` executes in the daemon | a module whose `main` only prints `Knet.max_ports` |
| U9 | LAG hash inputs / the "hash select 0-3" knob | after a LAG exists: traffic with varied L2/L3/L4 fields, per-member counters |
| U10 | Group MAC (`01:80:c2:…`) accepted as static L2 entry to the CPU | the optional steps in the plan report their return code |
| U11 | Source sysport reported to ipd for a LAG member (member vs LAG): do LACPDUs still land on the member netdev | bond stays in sync after the LAG is created; `tcpdump -i fpp2-1 ether proto 0x8809` |
| U12 | Tagged-member egress semantics of `egress_untagged_vlan_id` | capture on a loopback pair |
| U13 | LAG handle accepted by `ifcs_l2vni_member_add`, `ifcs_stp_port_add`, `IFCS_MC_DEST_ATTR_LOCAL_DESTINATION`; and whether member PVIDs are per sysport or per LAG | return codes of the apply; `ifcs show l2vni <v> members` |
| U14 | Does `l2learn flush` / `ifcs_l2vni_delete` remove learned hardware entries | `ifcs show l2_entry` before and after |
| U15 | Whether pass 1 (`python2.7 ifcsshell.py`) really completes in the NixOS chroot, i.e. whether the double init happens here too | the unit's journal for one start |

Suggested first hardware session, in order: U6 and U1 with everything down;
U8; then example 02 on a QSFP<->QSFP loopback (PROGRESS (x) says the cable is
on its way) rather than on the Unifi; U11; only then cables to the Unifi.

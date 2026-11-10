# Credo SAI 1.2.12 (x86_64) on the CS8210 Credo PHYs

Plan: run SONiC's public x86 Credo SAI (`libsaicredo` + `libsaicredo-owl`,
see `../reviews/2026-11-10-credo-sai-public.md`) under qemu-user on the
switch, with our own host program supplying the SAI register callbacks.
That gives MKA-capable MACsec (SAI MACsec API, hitless SA switch) on ports 1-16.
Static review (clean, with containable notes):
`../reviews/2026-11-10-credo-sai-static-review.md`.

## Step 0: fake PHY, sandboxed (desktop, 2026-11-10)

`run-step0.sh OUTDIR [TIMEOUT]` builds `fakephy.c` and runs it in bwrap
(`--unshare-all`, clean env, private /tmp, empty cwd, no $HOME/sys, timeout).
`CREDO_DEVICE_PATH` holds only `libSerdes_owl800.so`. `SWITCH_HARDWARE_INFO`
and `SWITCH_SHELL_ENABLE` are never set. One SAI PHY switch is created with
`REGISTER_READ/WRITE` pointing at a fake slice that logs every access and
returns 0 except the seeded identity registers. Profile: `owl800-slice.xml`
(retimer, host lanes 0-7, line lanes 12-19, no taps/polarity yet).

First run (`analysis/research/credo-sai-1.2.12/run0/`, git-ignored):

- Everything loads in the sandbox. `sai_api_initialize`/`query` return 0,
  libsai reads the XML profile and dlopens `libSerdes_owl800.so` from
  `CREDO_DEVICE_PATH`, and `ColdBoot` runs: `InitializeDevice`,
  `DownloadFirmware` (skipped by `LOAD_METHOD_NONE`), `get_firmware_info`
  **failed** (expected: the fake has no MCU), then `SetOptions`,
  `SetSerdesParams`, `ConfigurateLanePolarity`, `ConfigurateLaneOption` and
  `CreateObjects`. **`create_switch` returns success**, oid 0x1. **V**
- It made 4740 register accesses in 36 ms: 4540 reads and 200 writes, 92/117
  distinct registers. Every call has `n = 1`, `ctx = 1` (our
  PLATFROM_CONTEXT) and `device_addr = 0` (profile `phy_addr`). **V**
- **The identity registers (`0xcc/0xcb/0xca/0xf0`) are never read.** The
  device type comes only from the profile name, so the library would write
  to whatever answers. The real bridge must check the PHY ID itself before
  forwarding writes. **V**
- Sequence start: `R b7f0`, `R/W b73a` (the pad register the vendor sets to
  0x40, push-pull, here read-modify-write), `W b718 0aaa` then `0000` (looks
  like a command/unlock word, **I**), then per-lane blocks with stride 0x200
  (`xxaf = 8`, `xx43`, `xx61 = 0x3000`, `xxc1`). **V** for the accesses, **I**
  for the meanings.
- 4368 of the reads poll `b71b`, presumably the MCU/firmware mailbox status
  the fake never answers. The bound is a count, not time (36 ms total). **I**
- The register-to-MDIO mapping matches our vendor stack: our `libMdio` (and
  the vendor's) does every access as **clause 45, MMD 1**, 16-bit register,
  PHY address = bus + 1. The SAI callback gets exactly a 16-bit register, so
  the bridge is a direct `c45_xfer(phy, reg)`. **V** from `libMdio.c`.

## Step 1: trace of the vendor bring-up on the switch (2026-11-10)

`libMdio.c` now has an opt-in trace: if `/tmp/cs8210-mdio-trace.txt` exists
inside the SDK chroot (`/run/cs8210-sdk/tmp/` on the host) when phy_server
makes its first access, every transfer is appended to it as
`t_ns R|W phy reg val ret`. Deployed to the switch. Ran
`systemctl stop cs8210-switch`, created the file, ran
`systemctl start cs8210-switch` (54 s). Result: normal bring-up, `credo
config done!`, firmware 01.92.02 on 16/16, 32 fpp. Trace in
`analysis/research/credo-sai-1.2.12/vendor-trace-20261110/trace.txt.gz`
(git-ignored, 5 MB gz). The file was deleted afterwards and phy_server's
still-open handle truncated, so tracing is off again until the next start
with the file present.

- **669,735 accesses, 0 errors**, about 41.8k per PHY (addresses 0x02-0x11),
  over about 22 s per PHY. **V**
- **The vendor stack checks identity first** (`R 00cc = 2161`), then reads
  `b7f0 = 0x0020`, the chip version. libsai reads `b7f0` too, but not
  `00cc`. **V**
- Shared start: `W b73a 0040`, `W b718 0888`, …, `W b718 0aaa`,
  `W b718 0000`. libsai issues the same `0aaa`/`0000` pair. **V**
- Firmware download goes through a 16-word window `b900..b90f` (write 15
  words, commit by setting bit 15 of `b90f`, poll until it clears). That is
  about 1925 blocks per PHY, finishing at about 22.1 s. **V** for the
  accesses, **I** for the window semantics.
- **Every one of the 118 registers the x86 SAI touched in step 0 also
  appears among the 275 the vendor stack touches on one PHY.** None is
  SAI-only. **V**. This is strong evidence that the 2026 library and our
  2021 stack drive the same register interface.

Next:

1. Mailbox protocol: work out from the trace what `b71a`/`b71f` (vendor)
   and `b71b` (SAI) polling waits for, and the post-firmware command
   exchange.
2. Make the fake answer the mailbox (replay from the capture), so
   `get_firmware_info` succeeds, and add polarity/taps from `swap_setting.cmd`.
3. Only then: the switch (qemu-x86_64 + unix-socket MDIO bridge, `unshare -n`,
   boot type `fast` / `LOAD_TYPE=SKIP` first so the running 1.92.2 firmware is
   kept).

## Step 2: vendor state, profile, attach path, MACsec (desktop, 2026-11-10)

Offline only: fake PHY in bwrap, nothing touched the switch. Outputs are in
`analysis/research/credo-sai-1.2.12/step2/` (git-ignored). Mark: **V** =
verified (trace, disassembly, or fake run), **I** = inferred.

### Tools (all in this directory)

| file | what |
| --- | --- |
| `vendor-profile.py` | builds the libsai XML profile for one PHY address from the vendor c-phy files (`swap_setting.cmd`, `preemphasis_PAM4_400G_32_{HostLT,Optics}.cmd`); `--verify TRACE` checks the register encoding below against a libMdio trace on all 16 PHYs |
| `check-diff.py RUN [--phase P]` | the mechanical test: classes every write of a run as transient / same / DIFF / unknown against `seeds.txt`, and every TCM write (mailbox 5040) against `tcm.txt`; exit 0 iff no DIFF or unknown |
| `trace2fake.py` | now also emits `tcm.txt` (MCU TCM words seen on the PHY) and records the `b723` read-back of mailbox answers |
| `fakephy.c` | TCM model; `FAKE_MBOX_EXTRA` (answers not from the trace, logged as `EXTRA-hit`); stages `FAKE_FW_LOAD_TYPE`, `FAKE_QUERY` (switch/port gets), `FAKE_MACSEC` (create_macsec + capability gets), `FAKE_WARM_SHUTDOWN` (RESTART_WARM + remove_switch + api_uninitialize, writes the warm state file); `FAKE_GATE` write gate (below); `LOG_STDERR` merges the access log with libsai's log so phases can be attributed |
| `run-step0.sh` | `PROFILE=`, `BOOT_TYPE=`, `MBOX_EXTRA=`, `STATE_DIR=` (writable `/state`, `SAI_WARM_BOOT_READ/WRITE_FILE=/state/warm.bin`), `MERGE=1`, plus the `FAKE_*` switches; still `--unshare-all`, clean env, `CREDO_DEVICE_PATH` pinned, no hw_info, no shell |
| `mbox-extra.txt` | f000/f001/f002 answers derived from our own bring-up log (see 2.4) |
| `mbox-extra-warm-noports.txt` | **invented, exploratory**: a010-a01b answers meaning "port not configured", only to see what the warm path does after allocate |

Fake inputs for PHY 0x02: `step2/fake-phy02/` (`seeds.txt`, `mbox.txt`, `tcm.txt`) from
`trace2fake.py vendor-trace-20261110/trace.txt.gz step2/fake-phy02 02`.

### 2.1 Bit mapping (goal 1)

Per lane L (base `L*0x200`, lanes 0-7 host, 12-19 line, 8-11/20-23 unused):

| setting | register bit | vendor command | V/I |
| --- | --- | --- | --- |
| RX polarity inverted | `+0x043` bit 7 **and** `+0x161` bit 14 (always written together) | `rx_pol_mask` bit L | V |
| TX polarity | `+0x0a0` bit 5 = **NOT** inverted (`tx_pol_mask` bit L = 1 -> bit 5 = 0) | `tx_pol_mask` bit L | V (libsai encodes it the same way: tx-polarity=false writes bit 5 = 1) |
| TX MSB/LSB swap | `+0x0af` bit 10 | `set_tx_msb lane_mask=0xff0ff tx_msb=1` | V (bit), I (TX vs RX by command order: the trace sets all `xxaf` first, then all `xx43`, twice, matching tx, rx, tx, rx) |
| RX MSB/LSB swap | `+0x043` bit 15 | `set_rx_msb ...` | as above |
| TX taps pre2, pre1, main, post1, post2 | `+0x0a5`, `+0x0a7`, `+0x0a9`, `+0x0ab`, `+0x0ad`, bits 15:8, signed 8 bit | `set_tx_taps ... taps=a,b,c,d,e` (same order) | V (trace on 16 PHYs; libsai writes the same registers in the same order from `tx-taps="a,b,c,d,e"`) |

* Mask bit order: **bit L of the binary literal (LSB = lane 0) is lane L**; masks are ANDed with
  `lane_mask 0xff0ff`. Lanes 8-11 and 20-23 are untouched. **V**
* `vendor-profile.py --verify` on the 2026-11-10 trace: **2304/2304** expected fields match
  (16 PHYs x 16 lanes x 9 fields). **V**
* Line-lane `xxaf` ends at `0xfa09` (bits 10:9 = 01), host lanes at `0xfc09`: the host sets
  bit 10 on all 16 lanes, then the **MCU** rewrites the line lanes during
  `firmware_configure_port` (no host write of `fa09` in the trace). **V** for the values,
  **I** for the meaning.
* **Slice -> PHY mapping is not slice+1 for slices 1-8.** Polarity masks and taps both show
  vendor `slice_id` 1..8 swapped in pairs: slice 1 -> PHY 0x03, slice 2 -> PHY 0x02,
  slice 3 -> 0x05, slice 4 -> 0x04, ... slice 7 -> 0x09, slice 8 -> 0x08; slices 9-16 ->
  PHY slice+1. **So PHY 0x02 carries vendor slice 2's settings** (rx 0xe805d, tx 0xb4047),
  not slice 1's. **V** (two independent data sets, all 16 PHYs). Which front-panel port
  that is was not checked here. Profiles must be generated per PHY address
  (`vendor-profile.py --phy 0xNN` does the mapping).

### 2.2 Making libsai reproduce the vendor state (goal 2)

Profile: `vendor-profile.py --sdk <0.1.7 c-phy> --phy 0x02 --msb none` (polarity + PAM4 taps;
port-connector optional with `--port`). Parser facts (from `switch_profile_xml_t::Parse*`
string refs, **V**): `<phy_addr>`, `<mode>`, `<options><option name value/>` (slice
options), `<serdes_parameters><serdes_parameter id name type value/>`,
`<PAM4|NRZ><lane id tx-taps tx-taps-scale/>`, `<lane id tx-polarity rx-polarity>` with
`<option name value/>` children (lane options), `<port-connector>` with
`<system-side|line-side lane-number="0,1,.." speed fec/>`, `<mode>`, `<options>`
(`host_an/host_lt/line_an/line_lt/host_optical/line_optical`). **`<topology>` is not read**
(the string does not exist in libsai).

`check-diff.py` results (cold boot, LOAD_METHOD_NONE, seeds = PHY 0x02 post-vendor):

| run | profile | writes | transient | same | DIFF (regs) | TCM |
| --- | --- | --- | --- | --- | --- | --- |
| `run1`, `run2-fast` (step 1) | no polarity | 235 | 42 | 136 | 57 (41: +043 x16, +0a0 x8, +0af x8, +161 x9) | no model |
| `step2/run6-cold` | polarity + taps, slice-2 masks | 235 | 42 | 153 | **40 (24: +043 x16, +0af x8)** | 3 same, 0 DIFF |

(The step-1 note said "89 of 235 differ"; with the definitions above it is 57 lane-config
writes in 41 registers.)

**Final diff: 24 registers / 40 writes, all MSB**: `xx43` bit 15 on lanes 0-7,12-19 (RX MSB)
and `xxaf` bit 10 on lanes 0-7 (TX MSB; line lanes already have bit 10 = 0). Polarity:
0 differences. Taps: not written in a switch-only cold boot (they are applied per port,
see 2.5), and identical to the vendor values when a port is created. **V**

**Vendor settings with no SAI 1.2.12 equivalent** (all **V** from disassembly):
* **TX/RX MSB swap.** `switch_profile_xml_t::SetSerdesParams` and the JSON variant are
  stubs (`mov $0,%eax; ret`), so `<serdes_parameters>` is parsed and dropped; libowl's
  `tx_msblsb`/`rx_msblsb` exist only as serdes parameters. `CredoSwitch::SetSerdesParami/f/
  List*` have no callers. libSerdes `set_lane_msb` is a stub. The lane/slice option table
  (libowl 0x764a0, 12 entries: fast_recover_timeout ... kp_override) has no msb entry, and a
  lane `<option name="tx_msblsb">` makes `set_lane_option` fail **and aborts ColdBoot**
  (`run4-laneopt`). The custom-attribute path accepts only `fast_recover_timeout`. On the
  other hand the cold-path lane init (`owl_slice_init` type 1 -> `canary_set_tx_msb/rx_msb`)
  actively **clears** both bits. The only way to keep them is outside SAI (bridge-side).
* `mdio_reg_wr 0xb73a 0x40` is not needed: it is REG_MCU_CLK_SEL and libsai's reset path
  RMWs it (same value).
* Port flags: `host_lt=1` produces the vendor's `PORT_SYS_SIDE_LT` bit (809f arg 0x0800);
  **0x1000 of the 809f config word** (vendor 0x180c, libsai 0x080c) is set by nothing
  tried (`line_optical`, `host_optical` don't). Candidates: `double_CRC enable=1`
  (libowl has ENABLE_DOUBLE_CRC; libsai has no double-CRC string; Owl's port-option op,
  device-ops +0x800, is NULL) or MacsecBypass. **I**. Even with `host_lt+line_optical`,
  libsai's port config writes 4 TCM words differently from the vendor (e.g. 0x00088000
  `420d0041` vs `020d0041`): libsai port config != vendor port config.

### 2.3 Corrections to steps 0/1 (V, libowl register-name table + call sites)

* `b718` is **REG_CHIP_RST**. `W b718 0aaa; W b718 0000` is `common_mcu_reset()` (after an
  RMW of `b73a` = REG_MCU_CLK_SEL), called by `owl_slice_init` for init type 1. It is not a
  handshake. **Every cold boot resets the chip/MCU.**
* Mailbox `5040` is `tcm_write`/`tcm_memset` (MCU TCM = firmware data memory), `b000` is
  `owl_fw_get_status`, `f003/f000/f001/f002` are firmware version/hash/crc/date, `f005/f006`
  feature query, `a010+p` port query. TCM read port: address -> `b8fe:b8fc`, status `b8fa`,
  value `b8f6:b8f8`; TCM write: address = `b900 | (b901 & 0xff00) << 9`, value
  `b723:b71c` (encoding **I**, consistent on every read-then-write pair in the trace). So
  "transient" registers carry persistent MCU state changes; `check-diff.py` therefore
  checks TCM writes too.
* libsai's InitializeDevice does three TCM read-modify-writes: 0x0070b000 |= 1,
  0x00808008 and 0x00808808 field[4:0] = 8. With the TCM model they write back exactly the
  post-vendor values (1, 8, 8). Without the model (steps 0/1) the fake returned stale
  registers and libsai wrote `0800a991`/`0800a988`. **V** (fake), **I** that firmware has
  not changed those words since.

### 2.4 Fast boot / attach (goal 3)

* **FastBoot cannot succeed in 1.2.12, whatever the PHY answers.** `CredoSwitch::FastBoot`
  (credo_switch.cpp:1291-1313) loads the profile, uses it only for
  `LoadDeviceByPHYName`, **deletes it**, and calls `InitializeDevice` without
  `SetPHYName/SetPHYMode/SetPHYAddr` (only ColdBoot 0x1ef52f and WarmBoot 0x1f0f28 call
  them). DispatchCommand then fails with "Can't find device " (empty name). It makes
  **0 PHY accesses** before failing (`run14-*-fast`), then `Configure` falls back to
  **ColdBoot**. It is not a mailbox answer. **V**
* `FIRMWARE_LOAD_TYPE=SKIP` changes nothing in ColdBoot (`run14-skip-cold`: same 235
  writes, chip reset, MSB clear). **V**
* **WarmBoot is the attach path.** It calls `InitializeDevice` with `warm_recover`, which
  makes libSerdes call `cr_slice_init` with type 2. `owl_slice_init` type 2 does
  `common_detect_mdio_mode`, `owl_update_lane_mode` per lane (b000) and
  `owl_fw_query_port` for ports 0-11 (a010-a01b): **no MCU reset, no lane writes**. Then
  `UpdateDeviceFWInfo`, `UpdateDeviceCapabilities`, `load_switch_state`. No SetOptions,
  polarity or CreateObjects. **V** (disassembly + fake).
* The warm state file is produced offline: cold boot on the fake with the same profile, then
  `SAI_SWITCH_ATTR_RESTART_WARM=true`, `remove_switch`, `sai_api_uninitialize` (the file
  is written only at uninitialize). `run7-warmA` (no ports, 660 B) and
  `run16-warmA-port` (2078 B: SWITCH 1, PORT 2, PORT 3, PORT_CONNECTOR 4).
* a010-a01b answers are not in the trace (the 2021 stack never attaches). With the fake's
  generic answer, `allocate_device` fails and libsai **falls back to ColdBoot**
  (`run8-warmB`). With `mbox-extra-warm-noports.txt` (invented: "no port configured")
  WarmBoot succeeds (`run9-warmB-noports`, `run15-gate-warm`, `run17-warmB-port-gate`):

  | run | writes | lane/config DIFF | same | unknown | TCM | b718 / b73a / lane regs written |
  | --- | --- | --- | --- | --- | --- | --- |
  | warm, state without ports | 220 (all mailbox class) | 0 | 0 | 0 | 3 same, 0 DIFF | none |
  | warm, state with ports | 220 | 0 | 0 | 0 | 3 same | none; PORT 2/3 and PORT_CONNECTOR 4 recreated from the state file without any extra access |

  b71b commands on this path: 49 x b000, a010-a01b (12), f005, f003, f000, f001, f002, and
  3 x 5040 (identical TCM values). **So yes: warm attach makes zero lane-config writes and
  only value-preserving TCM writes, on the fake.** **V** for the fake, **I** for the real
  PHY, because the a01x answers were invented and port 0 *is* configured on the switch
  (`owl_fw_get_port_info` will run and issue commands not seen here).
* Firmware info: f003 is replayed from the trace (`0f01/5c02` = 01.92.02). f000/f001/f002
  are in `mbox-extra.txt`, derived from `evidence/mainline-sdk-2026-11-10/credo-bringup.log`
  ("Date Code 0x4914, Hash Code 0x2e4303, CRC Code 0xa0b3", also in the firmware header)
  plus libowl's decode. f005 (feature) is left as a MISS. libowl only uses features if f005
  answers 0xffffff, so any other answer = "no features". What 1.92.2 really answers is
  unknown.
* The fake's generic MISS answer (`(cmd>>4) & 0xf00`, "done") is an invented "success"
  answer; every one is logged as `MISS (generic ok, not from trace)`.
* Warm state is keyed by **hw_info** ("load switch state with hw_info %s"). We must leave
  hw_info empty (else the ioctl MDIO path), so **run one process per slice** with its own
  state file. **I** (strings; multi-switch not tested).
* **Write gate prototype** (`FAKE_GATE=1`): only b71c/b723/b8fc/b8fe/b900-b903 may be
  written; b71b only with query commands (b000, b008, f000-f006, a010-a01b) or 5040 that
  does not change the TCM word; anything else exits **before** the write. Warm attach passes
  (`run15-gate-warm`, `run17-warmB-port-gate`). Cold boot is stopped at its first write
  `b73a` (before the chip reset). A warm attach that fails and falls back to cold is
  stopped the same way (`run15-gate-warm-noports-missing`). A cold boot with
  port-connector is stopped at `b73a` too (`run15-gate-port`).

### 2.5 Ports and MACsec on the fake (goal 4)

* Port-connector cold boot (`run10-port`, `run11-port-*`): two SAI ports (system lanes 0-7,
  line 12-19, 400G, rs544) + connector. `configure_port` = taps RMW on all 16 lanes
  (identical values), then mailbox e011/e021 (a723 0000, vendor 0017), 809f (config word,
  see 2.2), b008, then 10 (`host_lt+line_optical`) to 48 (no options) TCM writes. That is
  a full firmware port reconfiguration, never
  acceptable on a live slice. **V** (fake)
* Read-only gets (`run12-macsec`, and after warm attach `run18-warm-port-macsec-gate`):
  PORT_NUMBER 2, PORT_LIST 2, FIRMWARE_MAJOR_VERSION 1, MINOR 0x5c0002 (92.2),
  FIRMWARE_STATUS 1, ports HW_LANE_LIST 0-7 / 12-19, SPEED 400000, ADMIN_STATE false,
  FEC_MODE get = SAI_STATUS_ATTR_NOT_IMPLEMENTED_0. Accesses: one b000 status query
  (3 mailbox writes) in query_switch, none in query_ports. **V**
* **`SAI_MACSEC_ATTR_SUPPORTED_CIPHER_SUITE_LIST` etc. need a MACsec object, and
  `create_macsec` = `macsec_device_initialize` (EIP-164/163 init)**, so it is not
  read-only. It starts with one read through the TCM/register read port at
  `b8fe:b8fc = 0x01f890ff` (egress) / `0x01f8a0ff` (ingress). The fake reads 0 and
  `create_macsec` fails for both directions (status -1), so no capability values. With
  invented EIP signatures (`0x5ba4`/`0x5ca3`, `EIP164/163_REV_SIGNATURE_MATCH` in
  libdriver_164, `step2/fake-phy02-eipsig`, labelled), egress still fails right after the
  version read, and ingress reads `0x01f8a0d8` next and fails. The EIP engines live in that
  0x01f8xxxx window of the MCU address space (**I**). The vendor trace has no EIP traffic
  (macsecbypass skips `macsec_datapath_init`), so it cannot seed this.

### 2.6 What remains before a live run

1. Real a010-a01b answers and what `owl_fw_get_port_info` sends for a configured port.
   Get them with a gated live run: the gate stops at the first unknown command and logs it.
2. Coexistence with phy_server: libMdio holds a flock, and phy_server polls the mailbox.
   Either share the lock in the bridge, or establish (read-only first) whether stopping
   `cs8210-switch` leaves the PHYs running.
3. EIP-164/163 init of `create_macsec` on a running bypass datapath: model it from the
   2021 `libdriver_164_u` + DWARF'd glue, offline, before allowing any EIP write.
4. f005 answer of 1.92.2 (harmless if not 0xffffff).
5. Per-PHY profiles + state files (`vendor-profile.py --phy 0xNN --msb none --port
   --port-opt host_lt=1 --port-opt line_optical=1`, then cold + `FAKE_WARM_SHUTDOWN=1` on
   the fake).

### 2.7 Live-run checklist (one slice, first attempt)

1. Pick one PHY (start with 0x02 = vendor slice 2) whose port can drop without harm. Record
   its link state and a **read-only** snapshot of the 275 trace registers (no TCM reads:
   those need b8fc/b8fe writes) while the vendor stack is up.
2. Resolve phy_server coexistence (2.6.2) first. Never run both against one PHY unlocked.
3. Bridge: callback -> clause 45, MMD 1, PHY address = bus + 1, one register per call.
   Before forwarding **any** write, check `1.00cc == 0x2161`, `1.00cb >> 4 == 0xa`,
   `1.00f0 == 0x20`. Log every access to a `.txt` file.
4. Bridge write gate exactly as `FAKE_GATE` (2.4), with the TCM check done by the bridge's
   own TCM read of the target word before forwarding a 5040. On a violation, kill the x86
   process before the write.
5. x86 process: qemu-x86_64 in bwrap/unshare -n, `CREDO_DEVICE_PATH` = dir with only
   `libSerdes_owl800.so`, **no** `SWITCH_HARDWARE_INFO`, **no** `SWITCH_SHELL_ENABLE`,
   `FIRMWARE_LOAD_METHOD_NONE`, `SAI_BOOT_TYPE=warm`, `SAI_WARM_BOOT_READ_FILE` = that
   PHY's offline state file, profile from `vendor-profile.py --phy 0xNN`. One process per
   PHY.
6. Expect `create_switch` success with only the writes listed below, or a gate stop at
   the first unknown query (then add it to the allowlist only if it is a query).
7. Afterwards compare the read-only snapshot (same 275 registers) and link state. Don't
   create MACsec objects until 2.6.3 is done.

**Writes a warm attach would send to the real PHY** (measured on the fake): mailbox
`b723`/`b71c`/`b71b` for 49 x b000 (a723 = 0 alternating with lane 0..0x17), a010..a01b, f005, f003,
f000, f001, f002; TCM read addresses via `b8fc`/`b8fe`; 3 TCM writes via
`b900`-`b903` + 5040 to 0x0070b000 (|= 1), 0x00808008 and 0x00808808 (field = 8), all
value-preserving if the words are as the vendor left them; plus whatever
`owl_fw_get_port_info` sends for the configured port (unknown). **Nothing else**: no
b718, b73a or lane register.

**A cold boot (also reached by `fast`, and by any warm failure) would send**: `b73a`
RMW, **`b718 0aaa, 0000` (chip reset)**, 24 lanes x (`xxaf` x2 clearing TX MSB, `xx43`
clearing RX MSB, `xx161` x2, `xxc1 = 0`), 3 TCM RMWs, f00x queries, 48 polarity writes,
and with a port-connector: taps on 16 lanes, e011/e021/809f port config and 10-48 TCM
writes.

### 2.8 Stop-ship for a live run

* **Any path into ColdBoot**: the default `cold`, **`fast` (always falls back)**, and any
  WarmBoot failure ("Warm boot failed. do cold boot" is unconditional). ColdBoot pulses
  REG_CHIP_RST, clears the MSB bits (no SAI way to restore them), and with
  LOAD_METHOD_NONE loads no firmware. Without the bridge write gate, do not run.
* Missing or incorrect slice->PHY mapping (2.1): wrong polarity on 8 of the 16 slices if
  `slice_id` = PHY - 1 is assumed.
* Unsynchronised access with phy_server on the same MDIO master.
* A non-empty `SWITCH_HARDWARE_INFO` (ioctl MDIO path), or one process holding several
  slices' warm state with empty hw_info.
* Any MACsec object creation on a live slice before the EIP init sequence is understood.
* Letting libsai configure ports on a live slice (2.2: its port config differs from the
  vendor's, and it is a reconfiguration anyway).

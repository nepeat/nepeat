# AS7772-32X: SONiC at boot

2026-11-22. SONiC (`services.sonic`, sai-openbcm on the BCM56930) runs at boot behind the 32 Evora PHYs. Cabled and
declared up: cages 1 <-> 2 and 31 <-> 32 (100G DACs); every other port is admin down. Background:
[extphy.md](extphy.md) (PHYs, MDIO rule), [evora-rehost.md](evora-rehost.md) (where the PHY init comes from),
`nix/modules/switch/sonic/README.md`, `nix/packages/sai-openbcm/README.md`.

## What runs, in boot order
1. `openbcm-devices` (BDE/KNET nodes), then **`openbcm-mdio-script.service`** (oneshot, once per boot): the SDK's
   `init soc` + `init misc` with the generated config.bcm (MDIO divisor 53, hold delay 10, read back and refused if
   different), then the MDIO script `/var/lib/evora-phy/evora-100g-bypass.mdio` through the chip's MDIO master:
   firmware download (RAM) + 100G MACsec-bypass config of all 32 PHYs, then a read-back check per PHY (ID
   2391/80a0, firmware d00b, checksum 600d, both sides active). **About 98 s**; boot to multi-user 2 min 4 s.
   While it runs it publishes the ASIC die temperature itself (it owns the SDK), as the reader and syncd do.
2. `sonic.target`: syncd runs `init all` (resets the ASIC, not the PHYs) and applies the board's port quirks;
   orchagent enables the four declared ports; pmon, LLDP, FRR as on the AS7760.
   **Hard fail:** `sonic.target` and `sonic-syncd` require the bring-up. If it fails nothing of SONiC runs, the
   switch stays reachable, `openbcm-asic-temp` takes over the temperature and the fans stay on their curve:
   [Debugging the PHY bring-up](#debugging-the-phy-bring-up).
- Nix: `hardware.openbcm.mdioScript` (`nix/modules/switch/openbcm.nix`, generic: any script, any board), tool and
  format `nix/packages/sai-openbcm` (`openbcm-mdio-script`, `src/mdio_script.h`); board wiring
  `nix/modules/boards/as7772/default.nix`; ports in `nix/configs/as7772/configuration.nix`.
- ASIC side of every port (sai.profile `SAI_OPENBCM_PORT_*`): interface CAUI, autoneg off, no ASIC FEC whatever
  the SONiC `fec` field says (the PHY does RS-FEC towards the cage), average IPG 16 bytes. Port LEDs: our own LED
  programs, loaded by syncd at SDK init (`hardware.as7760.ledProgram`, [leds.md](leds.md)).
- The script file is **private** (it contains the vendor firmware words): not in git, not in the Nix store.

## Make and install the PHY script (workstation, from `ai_re/`)
```
T=devices/edge-core-as7772-32x/dumps/x/evora-trace;  R=devices/edge-core-as7772-32x/evora-rehost
# (only for a new mode or after a change to the vendor flow: record traces with rehost.py, see evora-rehost.md;
#  the switch must be freshly booted with SONiC and openbcm-mdio-script stopped)
python3 $R/mkscript.py -o $T/evora-100g-bypass.mdio $T/all-init.trace.txt $T/all-cfg.trace.txt $T/all-pre.trace.txt   # pre = factory line-side TX taps
$R/install-script.sh            # scp to /var/lib/evora-phy (0700), sha256 checked on the switch
```
Installed now: sha256 `e18dddef7c9e...` (with the factory TX taps, [load-test.md](load-test.md)); the tap-less script `fdda87ade40b...` is kept beside it as `evora-100g-bypass-notaps.mdio`. A new SSD image needs `install-script.sh` once: without the file
the unit fails (`script-missing`, nothing is written to any PHY) and SONiC does not start; after installing it,
`systemctl restart sonic.target`.

## Operating it
- `phy-status`: the last bring-up (outcome line, counts, the table it verified) and the 32 PHYs read now through
  syncd, by cage: id, id2, fw, cksum, sides, state. `phy-status --json`: the last run for programs
  (`/run/openbcm-mdio-script/status.json`, `status.txt` next to it; both exist after every run, good or bad).
- Re-run by hand: `systemctl restart openbcm-mdio-script` (it stops the temperature reader itself and takes the
  stack down and up around it), or after a failure `systemctl restart sonic.target` (a failed bring-up is run
  again first, a good one is not). A second SDK process is refused by the SDK lock (`sdk-busy`, names the
  holder), nothing is written.
- **A reboot clears the PHYs** (they read as after power-on: 1.8215 = 0001); an SDK or SONiC restart does not.
  The script therefore runs on every boot and never on `systemctl restart sonic.target` / `nixos-rebuild switch`
  (except while it is in the failed state: both then try it again; a `deploy.sh switch` that finds the cause
  still there activates the new system and ends with "the following units failed: openbcm-mdio-script").
- The script also runs cleanly over PHYs that are already up (in-band reset first).
- **Fans at boot** (since 2026-11-22, [fan-boot-20261122-cold1.txt](fan-boot-20261122-cold1.txt)): no failsafe
  phase any more (before: 100 % for the first 2 min 45 s of every boot). The CPLD runs the fans at full speed
  until fand starts 14 s after the kernel; fand starts at **60 %** (`startup_duty`: one CPLD step above the 53 %
  minimum, for the 10 s dwell) and is at the **53 % minimum 50 s after boot** at the latest (first sample), where
  it stays: 12,000-14,600 rpm, ASIC 42-45 C during the bring-up, CPU die 39-40 C, board 33-34 C.
  - The ASIC sensor is "not online yet" for the 4 s until the bring-up has the SDK up (fand `startup_grace` 10 min:
    no reading since fand started is no fault for that long; then, and for a reading that went away, it is the
    usual failsafe). Meanwhile, and whenever the ASIC is not readable from the start (a `bcm.user` session
    right after boot), the `cpu` zone (T2080 die diode: flat to 65 C, 100 % at 90 C, failsafe at 100 C) and the
    board zone run the fans: a cool CPU means a cool ASIC while the ASIC does nothing.
  - The bring-up publishes the temperature every 5 s between two MDIO operations (20 readings per run, longest
    read 0.14 ms; 0 recorded values differed, same 98-99 s as before). Hand-overs are bridged by the sensor's
    `grace` (45 s, last value held): bring-up -> syncd 6 s, reader -> bring-up 4 s, measured.
  - Not changed: any real fault (sensor lost for longer, fan fault, critical temperature) is 100 % at once.
- `openbcm-diag "<cmd>"`: diag shell inside syncd. [sonic-traffic-test.sh](sonic-traffic-test.sh) (run on the
  switch): runtime VRFs + /31s on both DAC pairs, pings, SONiC and ASIC counters.

## Debugging the PHY bring-up
Where to look, in this order: the login message (interactive shells, while failed), `phy-status`,
`systemctl status openbcm-mdio-script` (its `Status:` line is the outcome), `journalctl -b -u openbcm-mdio-script`
(on a read-back failure it holds the full PHY table). The outcome line names the script line and step, the PHY
(by cage), the register, expected and read value, and how many operations had run, then a class:

| class | means | do |
| --- | --- | --- |
| `script-missing`, `script-malformed` | the private file is not on the SSD, or truncated; nothing was written | workstation: `evora-rehost/install-script.sh`; switch: `systemctl restart sonic.target` |
| `phy-not-answering` | a PHY reads one constant on every verified register (`CONSTANT 0x....` in the table): hung, or nothing there | **cold** power cycle (`lab-power 1 off`, `on`); a reboot or a re-run does not help ([extphy.md](extphy.md)). It boots by itself afterwards |
| `sequence-diverged` | a poll never saw its value but the PHY still answers: it left the recorded sequence | `systemctl restart sonic.target` (100 s). The same line twice: cold power cycle; still: the script does not fit this hardware/firmware |
| `verify-mismatch` | the script ran to its end, the read-back differs (`MISMATCH`, the differing registers are marked `!`) | as above. `fw 0001` on all 32 = the PHYs are in their after-reset state: the download did not run or did not take |
| `sdk-busy` | another process owns the switch chip (the message names pid and command) | `systemctl stop sonic.target openbcm-asic-temp`, end any `bcm.user`, `systemctl restart sonic.target` |
| `mdio-clock` | hold delay or clock divisor on the chip is not the config's after misc init; no frame was sent | the generated config.bcm (`rate_ext_mdio_divisor`, `mdio_output_delay`): a build problem, never work around it ([extphy.md](extphy.md): frames at the reset values hang PHYs) |
| `sdk-init`, `sdk-config-missing`, `mdio-io` | the switch chip or its MDIO master, not a PHY | SDK messages in the journal |
| `interrupted` | stopped by a signal between two operations; PHYs may be half-programmed | `systemctl restart sonic.target` (the script resets them first) |

- Seen on this switch: `script-missing` and `verify-mismatch` (table above the pmon row below). The other
  classes are covered by the tool's unit tests and the VM test only (fake bus / stand-ins), not provoked here.
- While failed: no `sonic-*` unit, no `EthernetN` netdev, ssh and the management port as usual, fans on their
  curve with the reader's temperature, `systemctl --failed` shows only `openbcm-mdio-script`.
- `phy-status --live` (only while syncd is down): reads the PHYs with the SDK itself; it stops the reader for a
  few seconds and re-initialises the switch chip (`init soc`, `init misc`), never writes to a PHY.
- Reads through syncd (`phy-status` with the stack up) are the diag shell's `phy raw c45`, clause 45 only.
- Tool, classes and file formats: `nix/packages/sai-openbcm/README.md` (openbcm-mdio-script); unit wiring:
  `nix/modules/switch/openbcm.nix`; names and hints for this board: `nix/modules/boards/as7772/default.nix`.

## Tested on the switch (2026-11-22)
| test | result | evidence |
| --- | --- | --- |
| script from a cold power cycle, SONiC off, then the diag-shell link test | 32/32 verified, 95.8 s; ce0/ce1/ce30/ce31 up, counters equal | `mdio-script-20261122-cold1.txt` (ca5ecee14010), `asic-link-2dacs-20261122-after-cold-replay.txt` (e185635268d8) |
| cold power cycle to SONiC, unattended | 4 ports up, 0 failed units; 6400 pings (64/1400/8000 B, both directions, both DACs) 0 lost; frames and bytes equal at both ends, no error counter; LLDP sees the far cage | `sonic-20261122-coldboot1-traffic.txt` (52bb9dada748), final system: `sonic-20261122-coldboot2-traffic.txt` (67b9aa3e312f) |
| `systemctl restart sonic.target` | links back without touching the PHYs, same traffic result | `sonic-20261122-stack-restart-traffic.txt` (0d620e97fc23) |
| reboot | PHYs re-initialised (99.0 s), same traffic result | `sonic-20261122-reboot1-traffic.txt` (212d9927a06d) |
| ASIC port state under SAI | `ps`: CAUI, autoneg No; `CLMAC_TX_CTRL AVERAGE_IPG=0x10`; CL74/CL91 False on the ports SONiC has `fec=rs` | in the traffic files |
| cold power cycle, fans (final wiring) | no failsafe; 60 % at fand start, 53 % from 50 s after boot; ASIC temperature published during the bring-up; 4 ports up, 0 failed units | `fan-boot-20261122-cold1.txt` (c027037dce7b) |
| forced failure: script moved away, reboot | `script-missing`; SONiC not started, only that unit failed, ssh up, reader running, fans 53 %, reason in `systemctl status`, status files, `phy-status`, login message | `phy-fail-20261122-script-missing.txt` (4c0ccd644b15) |
| forced failure: read-back on PHYs in their after-reboot state (a script of reads only) | `verify-mismatch`, 32 of 32 `fw 0001`, full table in the journal and status files; `phy-status --live` shows the same | `phy-fail-20261122-verify-mismatch.txt` (cfaa872e1018) |
| back to healthy without a reboot: script restored, `systemctl restart sonic.target` | bring-up ran (99.3 s, 0 differ), stack up after 110 s, fans stayed at 53 %, 4 ports up, 6400 pings 0 lost | `sonic-20261122-taskj-restore-traffic.txt` (35f1c8aa4e2b) |
| reboot on the final system | 60 % then 53 %, no failsafe; 4 ports up, 6400 pings 0 lost, 0 failed units | `sonic-20261122-taskj-reboot-traffic.txt` (96b9bf88f395) |
| pmon | 9 thermals (6 LM75, MAX1617 local + CPU die, ASIC), 12 fans, PSU 1 OK, syseeprom from NOR, 4 DACs present | `sonic-20261122-platform.txt` (4d315fdeb1e4) |
The traffic is CPU-path pings (KNET netdevs, each DAC end in its own VRF): it proves the links and the port setup,
not forwarding at rate.

## Replay fidelity: what was found
- The script is the vendor library's recorded MDIO sequence (emulator on the workstation), compiled so the
  runner needs no chip knowledge. 57,144 read-back values are compared on every run.
- **Timing bug found and fixed in the script:** the vendor waits "1 us" (192 times) after starting an engine
  operation and then reads its done flags. Replayed at full speed that read came too early on 147-152 of 192
  (`1.a101 = 6000`, recorded `7f00`), i.e. the following writes ran before the engine was done. `mkscript.py`
  now makes those waits 10 ms; since then **0 of 57,144 values differ** on every run (3 runs: re-run, reboot,
  cold). The ports linked and passed pings before the fix too, so links are no proof of a faithful replay.
- Short delays are spun, not slept: the appliance kernel rounds sleeps to a 4 ms tick (first run: 469 s).
- Not provable from here: delays the vendor code makes without calling a sleep (busy loops) are not in the trace.
  The compared values are the only check that the chip follows the recording.

## Open
- Only bypass at 100G on four cabled cages. Not done: load at rate, soak, PHY-side FEC/PCS counters, `extphy pre`
  taps, 25G break-out (needs its own script and SDK config), MACsec on, the other 28 cages with a cable.
- Port LEDs: programs run at boot, register read-back correct, **not yet seen by eye**; a lamp (and SONiC's oper
  state) follows the ASIC-to-PHY link, i.e. admin state, not the cable ([leds.md](leds.md)).
- PSU 2 (slot empty) shows "NOT OK" rather than absent: CPLD1 0x02 reads 0x28 and the AS7760 decoding takes
  bit 0 low as present. "Fan 3 front: Not OK" at 76 % is the AS7760 platform code's rpm-versus-fand-expectation
  check with an uncalibrated fand, not a fan fault.
- The `sonic-nix` saved config is empty: the VRFs and addresses of the traffic test were runtime only.
- Nothing is committed.

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
2. `sonic.target`: syncd runs `init all` (resets the ASIC, not the PHYs) and applies the board's port quirks;
   orchagent enables the four declared ports; pmon, LLDP, FRR as on the AS7760.
- Nix: `hardware.openbcm.mdioScript` (`nix/modules/switch/openbcm.nix`, generic: any script, any board), tool and
  format `nix/packages/sai-openbcm` (`openbcm-mdio-script`, `src/mdio_script.h`); board wiring
  `nix/modules/boards/as7772/default.nix`; ports in `nix/configs/as7772/configuration.nix`.
- ASIC side of every port (sai.profile `SAI_OPENBCM_PORT_*`): interface CAUI, autoneg off, no ASIC FEC whatever
  the SONiC `fec` field says (the PHY does RS-FEC towards the cage), average IPG 16 bytes. No LED program
  (`hardware.as7760.ledProgram = null`: port LEDs dark).
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
the unit fails ("No such file ... nothing was written to any PHY"), SONiC still starts, the ports stay down.

## Operating it
- Re-run by hand: `systemctl stop sonic.target`, wait for `openbcm-asic-temp` to come back (10 s) and stop it too,
  `systemctl restart openbcm-mdio-script`, `systemctl start sonic.target`. A second SDK process is refused by the
  SDK lock (exit 3), nothing is written.
- **A reboot clears the PHYs** (they read as after power-on: 1.8215 = 0001); an SDK or SONiC restart does not.
  The script therefore runs on every boot and never on `systemctl restart sonic.target` / `nixos-rebuild switch`.
- The script also runs cleanly over PHYs that are already up (in-band reset first).
- Unit failed with "phy ... reads ..., script wants ...": the chip left the recorded sequence. If every register of
  a PHY then reads one constant, it is hung: cold power cycle ([extphy.md](extphy.md)).
- Fans: nothing publishes the ASIC temperature while the script holds the SDK, so fand runs the fans at
  **100 % (failsafe) for the first 2 min 45 s of every boot** (script + syncd start + fand's 60 s calm period),
  then follows its curve on syncd's reading (journal of 2026-11-22 01:05: failsafe 01:05:13, sensor back
  01:06:58, curve 01:07:59).
- `openbcm-diag "<cmd>"`: diag shell inside syncd. [sonic-traffic-test.sh](sonic-traffic-test.sh) (run on the
  switch): runtime VRFs + /31s on both DAC pairs, pings, SONiC and ASIC counters.

## Tested on the switch (2026-11-22)
| test | result | evidence |
| --- | --- | --- |
| script from a cold power cycle, SONiC off, then the diag-shell link test | 32/32 verified, 95.8 s; ce0/ce1/ce30/ce31 up, counters equal | `mdio-script-20261122-cold1.txt` (ca5ecee14010), `asic-link-2dacs-20261122-after-cold-replay.txt` (e185635268d8) |
| cold power cycle to SONiC, unattended | 4 ports up, 0 failed units; 6400 pings (64/1400/8000 B, both directions, both DACs) 0 lost; frames and bytes equal at both ends, no error counter; LLDP sees the far cage | `sonic-20261122-coldboot1-traffic.txt` (52bb9dada748), final system: `sonic-20261122-coldboot2-traffic.txt` (67b9aa3e312f) |
| `systemctl restart sonic.target` | links back without touching the PHYs, same traffic result | `sonic-20261122-stack-restart-traffic.txt` (0d620e97fc23) |
| reboot | PHYs re-initialised (99.0 s), same traffic result | `sonic-20261122-reboot1-traffic.txt` (212d9927a06d) |
| ASIC port state under SAI | `ps`: CAUI, autoneg No; `CLMAC_TX_CTRL AVERAGE_IPG=0x10`; CL74/CL91 False on the ports SONiC has `fec=rs` | in the traffic files |
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
- Port LEDs: none (the factory `ES7632BTx.hex` is not converted).
- PSU 2 (slot empty) shows "NOT OK" rather than absent: CPLD1 0x02 reads 0x28 and the AS7760 decoding takes
  bit 0 low as present. "Fan 3 front: Not OK" at 76 % is the AS7760 platform code's rpm-versus-fand-expectation
  check with an uncalibrated fand, not a fan fault.
- The `sonic-nix` saved config is empty: the VRFs and addresses of the traffic test were runtime only.
- Nothing is committed.

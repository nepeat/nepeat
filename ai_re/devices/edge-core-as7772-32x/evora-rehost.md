# AS7772-32X: re-hosting the vendor Evora (BCM82391) PHY library

The vendor's own `extphy` code (diag `bcm.user`, ppc32) runs under Unicorn on the workstation, its two MDIO
callbacks forwarded to the real PHYs. It makes the MDIO traces that the switch replays at boot; it is not needed
at boot. Tools: [evora-rehost/README.md](evora-rehost/README.md). Rules: [extphy.md](extphy.md). Vendor
addresses: [evora-vendor-re.md](evora-vendor-re.md). Traces, states and the vendor binary stay in `dumps/`.

## Commands (from `ai_re/`, in the devshell)
```
R=devices/edge-core-as7772-32x/evora-rehost;  T=devices/edge-core-as7772-32x/dumps/x/evora-trace
python3 $R/rehost.py 'init 0x24 4' 'info'                        # mock backend: no hardware, polls time out
python3 $R/idscan.py --label baseline                            # read-only: 1.8b00/1.8b01 at the 32 ids (ssh)
# ssh backend: own bcm.user on the switch, needs the SDK lock (stop sonic.target and openbcm-asic-temp first);
# record init traces on freshly booted PHYs (openbcm-mdio-script not run)
python3 $R/rehost.py --backend ssh --trace $T/a.trace.txt --log $T/a.console.txt --save-state $T/a.state 'init all'
python3 $R/rehost.py --backend ssh --load-state $T/a.state --save-state $T/b.state \
    'cfg 0x24 nphy=4 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1' 'ver 0x24' 'status 0x24' 'status 0x24 dsc'
# diag backend: through syncd, SONiC stays up (status, pre, reg64, phystat.py): see load-test.md
python3 $R/rehost.py --backend diag --load-state $T/all-cfg.state 'status 0x0'
python3 $R/mkscript.py -o $T/evora-100g-bypass.mdio $T/all-init.trace.txt $T/all-cfg.trace.txt $T/all-pre.trace.txt
$R/install-script.sh                                             # to /var/lib/evora-phy, see sonic.md
# as of 2026-11-22 the workstation's evora-100g-bypass.mdio is the tap-less script (fdda87ade40b); the one on the
# switch is evora-100g-bypass-taps.mdio (e18dddef7c9e): pass that file to install-script.sh, or rebuild as above
python3 $R/summarize.py $T/all-init.trace.txt                    # registers written, polls, LMI addresses
```
- Arguments are `extphy` sub-commands as in the vendor usage text ([evora-vendor-re.md](evora-vendor-re.md));
  `--stdin` reads more, `-v` prints every MDIO access with the vendor call chain.
- `init all` walks the 32 ids in groups of 4 and gives up on a group as soon as one PHY in it fails.
- **The library keeps per-PHY state in RAM.** `cfg`/`status`/`ver` need the state of the `init` run: same
  process, or `--save-state`/`--load-state` (the state file holds vendor data: keep it under `dumps/`). Without
  it `ver` returns -25 and `status` dereferences NULL in `bcm_plp_secy_bypass_get` (a vendor bug; the emulator
  stops with "unmapped read"). `init` of a PHY the loaded state already knows returns -24 ([macsec.md](macsec.md)).
- Timings: `init all` 331 s for 32 PHYs through ssh, `cfg` 6 s per group; one read about 0.4 ms (ssh), 6 ms (diag).

## How it works
- Unicorn 2.1.4, ppc32 BE, CPU model **e500mc** with `UC_TLB_VIRTUAL`. The binary is built for the e6500:
  compiler output uses `isel`, and glibc's `memcpy` (0x119164a0) / `memset` (0x11915e64) use 64-bit `ld`/`std`,
  so those two are replaced by Python (every Unicorn ppc64 model faulted on `isel`). Another unsupported routine would stop the run with "exception 96".
- glibc start-up runs for real up to `main` (not run) on a minimal Linux syscall layer. No SDK or diag-shell
  init: the `extphy` handlers are called as `handler(argc, argv)`; the diag's MDIO callbacks
  0x11737db0 / 0x11737e20 are hooked, so `soc_miimc45_*` is never reached.
- Time: `gettimeofday` is virtual (advances with requested sleeps), so vendor time-outs count the vendor's own
  delays, not bridge latency. Sleeps are real on hardware.
- Writes are pipelined and flushed before every read or delay: order is preserved, **write-to-write spacing is
  not the vendor's**. Each `phy raw` is a complete CMIC operation, so frames cannot overlap.
- ssh bridge: `stdbuf -o0 bcm.user` on the switch (`init soc`, `init misc`), synchronised with `echo MK<n>`
  markers (no prompt on a pipe; `@` is eaten by the diag shell).
- **Safety guards:** `fw_load` other than 0/1/2 is refused; entering `plp_evora_download_prog_eeprom` with its
  EEPROM flag (r8) set stops the run; so does a write that newly sets 1.8210 bit 1 or 1.8203 bit 10. (1.8210
  reads 0x0033 after power-up, bit 1 already set: the guard's first version tripped on that mid-broadcast.)

## Trace format and what the traces show
`R|W <phy> <devad> <reg> <value>`, `D <usec>`, `# marker`. Reference trace
`dumps/x/evora-trace/init-cfg-0x24-0x27.trace.txt` (sha256 1708fc90687f..., 133094 lines: 52106 reads, 68840
writes, 12140 delays = 6.8 s). Classification is from traces and replay behaviour, not from source.
- **Order:** every stage walks the ids ascending. Per PHY: ID read (1.0002, 1.0003, 1.8b00, 1.8b01) + core reset;
  broadcast enable; **firmware words written to the last id of the group only** (0x27: 24876 writes of 1.8222
  for a 24032-word image) while the others listen; verify; end; then MACsec init, bypass and mode config.
- **Reads that steer the code:**

  | register | role |
  | --- | --- |
  | 1.900a | LMI status, **clears on read**; 0x0001 = write done, 0x0007 = read data ready, bit 2 comes and goes |
  | 1.9005 | LMI command/data FIFO: each read pops a word; never re-read |
  | 1.8221 | download handshake: 0xf1ac, 0x0ead, then 0x1000+n per 32 words, 0x0000 between blocks, 0x0303 at the end |
  | 1.8213 bit 1 | serboot busy |
  | 1.8251 | & 0xd == 0xd: line/sys TSC active |
  | 1.8215, 1.8245/1.8247 | firmware version (d00b; 0x0001 = none, as after power-on), checksum words (0x600d) |
  | 1.82ff | boot control: 0x8003 untouched, 0x50c0 running (0x4000 on the PHY that refused its download) |
  | 1.8b90 | LMI side select (0 line, 1 system), read back |
  | 1.a000-1.a005, read-back 1.a100/1.a101 | second indirect port, used by the MACsec init (firmware messages) |

  All other reads are read-modify-write inputs. LMI protocol decoded: `evora-rehost/lmi.py`.
- **Direct dev-1 registers written** (functions are guesses; there is no register map): 1.8201 = 0x007e (in-band
  reset), 1.8b10 = 0x0081 then 0x008b, 1.8b02 = 0x0004, 1.9011 = 0, 1.8b03 = 0x8000/0, 1.8b11-1.8b14 = 0x9f3f
  then 0, 1.8b91 = 0x0004/0x0404/0x0406, 1.8a4c = 0x0a24, 1.8255 = 0xbeaf, 1.826d = 1, 1.8b96, 1.8204;
  1.82ff, 1.8202, 1.8210, 1.8222, 1.823x = download. LMI addresses written in the reset stage: 0x020a-0x020d,
  0x0214, 0x0224, 0x0228, both sides. Nothing is identifiable as a GPIO or clock-output write to
  another chip (not excluded without a register map).

## From trace to boot script
- `mkscript.py` writes `replay.py`'s rules out per operation (polls collapsed, 1.900a read for read, 1.9005 never
  re-read) plus a read-back verification per PHY, so the runner on the switch needs no chip knowledge.
- A replay is only valid for a chip that takes the same branches; the compared read-backs are the check
  (57,144 values per run, 0 differ). Timing fix it needed (vendor "1 us" waits = 10 ms) and results:
  [sonic.md](sonic.md). Taps script: [load-test.md](load-test.md).

## The first night (2026-11-21), superseded
- Bus 1 (0x24-0x27) came up fully; on bus 0, id 0x00 read 0x0000 on every register, id 0x02 hung during the
  vendor init (0x1900, later 0xff9c, on every register), id 0x01 refused the download ("SERBOOT BUSY"); 11 ids
  read 0xffff. We took these for bad or reset-held PHYs and stopped.
- **It was MDIO timing**: the bridge ran `init soc` only ([extphy.md](extphy.md)). With `init misc` and a cold
  power cycle all 32 PHYs init cleanly. Trace comparison for id 0x02: [mdio-review.md](mdio-review.md).
- Evidence: `evidence/evora-status-20261121.txt` (vendor console output), `evidence/evora-idscan-20261121.txt`
  (ID scans between stages), traces `dumps/x/evora-trace/b0-*`, `init-cfg-0x24-0x27.*`, `replay4.result.txt`
  (live replay on 0x24-0x27: 0 handshake mismatches, 39 reads of 1.900a differing only in bit 2).

## Unverified
- Whether any vendor step depends on write-to-write spacing (none has failed).
- Delays the vendor code makes without calling a sleep (busy loops) are not in a trace.
- `mode=1`, break-out, `prbs config`, `eyescan`, `polarity` were not exercised (`mode=2`, `pre`, `reg64`,
  `status *`: done, see macsec.md and load-test.md).

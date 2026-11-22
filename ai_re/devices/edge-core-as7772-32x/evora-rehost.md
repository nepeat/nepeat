# AS7772-32X: re-hosting the vendor Evora (BCM82391) PHY library

2026-11-21. Tool: [evora-rehost/](evora-rehost/) (scripts only; the vendor binary is read from `dumps/` at run time).
Evidence: [evora-status-20261121.txt](evora-status-20261121.txt) (vendor console output),
[evora-idscan-20261121.txt](evora-idscan-20261121.txt) (ID scans between stages), traces in
`dumps/x/evora-trace/` (git-ignored). Background: [evora-vendor-re.md](evora-vendor-re.md).

## Result
- **2026-11-22: the bus 0 failure, the "lost" PHY and the silent ids below were all MDIO timing** (the bridge ran
  `init soc` only: hold delay 3, see [extphy.md](extphy.md)). The bridge now runs `init soc`, `init misc`; with that,
  `init all` + `cfg` succeed on all 32 PHYs. Read the rest of this section as the record of the first night.
- **Update, same night, after a cold power cycle (coordinator):** ids 0x00-0x03 answered again; `init 0x00 4` failed
  at id 0x02 again (firmware verify, then hung); `init 0x00 2` + `cfg 0x00 nphy=2 ...` succeeded and **cages 1-2
  pass frames** ([extphy.md](extphy.md)). Traces `b0-*.trace.txt`. The bus 0 notes below describe the first attempt.
- **Works:** the vendor's own `extphy` code (diag `bcm.user`, ppc32) runs under Unicorn on the workstation, with its
  two MDIO callbacks forwarded to the real PHYs through our OpenBCM `bcm.user` (`phy raw c45`).
- **Bus 1, ids 0x24-0x27: fully up** in 100G MACsec bypass. `extphy init 0x24 4` (broadcast RAM download, 42 s):
  "Firmware download success", FW version **D00B**, checksum word 600D, status Running; then
  `extphy cfg 0x24 nphy=4 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1` rc 0 (6 s). `status`: speed 100000,
  line CAUI4_C2C, system CAUI, FEC enabled, low-latency bypass egress+ingress YES, link DOWN (no cable in
  cages 5-8). `status dsc`: Falcon ucode D10B_16, COM_CLK 156.25 MHz, VCO 25.75 GHz, **PLL_LOCK 1**, 34-35 C,
  no signal detect on any lane. `status pmdiag`: PCS block/AM lock 0 on line and system side (nothing drives
  either side: no cable, and our SDK had only done `init soc`, no port enabled).
- **Bus 0 did not come up, and one PHY was lost doing it** (see "Bus 0"). Id 0x03 has firmware
  (scan: 1.8215 = d00b, 1.8245 = 600d) and took `cfg`, but the group's MACsec/bypass init stage was skipped;
  id 0x01 refuses the download; **id 0x02 stopped answering during the vendor init**; id 0x00 had stopped
  answering before this work touched the bus. So neither DAC cage (ids 0x00, 0x01) could be brought up.
- **No silent id started answering** after any stage (7 scans: baseline, after init of bus 1, after cfg of
  bus 1 and each bus 0 attempt, after the replay). I stopped there: the other answering groups
  (0x48-49, 0x6e-6f, 0x110-111, 0x135-137, 0x159-15b, 0x17d) were **not** initialised, because 1 of 7 PHYs hung
  in the vendor init and nothing is known to revive a hung PHY short of a power cycle (not tried, not allowed here).
- The recorded trace of bus 1 **replays without the emulator** (`replay.py`, 49 s) and leaves the four PHYs in
  the same state (vendor `ver`/`status`/`status dsc` afterwards identical apart from temperature).

## Commands (from `ai_re/`)
```
PY="nix shell nixpkgs#python3 nixpkgs#python3Packages.unicorn -c python3"      # not in flake.nix yet
R=devices/edge-core-as7772-32x/evora-rehost;  T=devices/edge-core-as7772-32x/dumps/x/evora-trace
sudo ssh root@10.60.2.23 systemctl stop openbcm-asic-temp       # the bridge needs the SDK lock; start it again after
$PY $R/idscan.py --label baseline                                # read-only: 1.8b00/1.8b01 at the 32 ids
$PY $R/rehost.py 'init 0x24 4' 'info'                            # mock backend: no hardware, polls time out
$PY $R/rehost.py --backend ssh --trace $T/a.trace.txt --log $T/a.console.txt --save-state $T/a.state 'init 0x24 4'
$PY $R/rehost.py --backend ssh --load-state $T/a.state --save-state $T/b.state \
    'cfg 0x24 nphy=4 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1' 'ver 0x24' 'status 0x24' 'status 0x24 dsc'
$PY $R/replay.py $T/init-cfg-0x24-0x27.trace.txt --backend ssh   # no emulator; --map 24=135 moves an id
$PY $R/summarize.py $T/init-cfg-0x24-0x27.trace.txt              # registers written, polls, LMI addresses
```
- Arguments are `extphy` sub-commands as in the vendor usage text; `--stdin` reads more, `-v` prints every MDIO
  access with the vendor call chain. `init all` works too but walks all 32 ids in groups of 4 and gives up on
  a group as soon as one PHY in it fails, so use `init <first> <count>` while PHYs are missing.
- **The library keeps per-PHY state in RAM** (firmware loaded, MACsec devices). `cfg`/`status`/`ver` need the
  state of the `init` run: same process, or `--save-state`/`--load-state` (the state file holds vendor data:
  keep it under `dumps/`). Without it `ver` returns -25 and `status` dereferences NULL in
  `bcm_plp_secy_bypass_get` (also after a failed init; a vendor bug, the emulator stops with "unmapped read").

## From trace to boot-time script (2026-11-22)
- `evora-rehost/mkscript.py` compiles recorded traces into one MDIO script for `openbcm-mdio-script` on the switch
  (`replay.py`'s rules written out per operation, plus a read-back verification per PHY);
  `evora-rehost/install-script.sh` copies it to `/var/lib/evora-phy`. Commands, results and the one timing fix the
  fast replay needed ("1 us" vendor waits made 10 ms): [sonic.md](sonic.md).
- The emulator stays the tool that makes traces (new modes, MACsec on, break-out); it is not needed at boot.

## How it works / what is emulation artefact
- Unicorn 2.1.4, ppc32 BE, CPU model **e500mc** with `UC_TLB_VIRTUAL`. The binary is ppc32 code built for the
  e6500: compiler output uses `isel` (not in the classic 32-bit models), and glibc's hand-written
  `memcpy`/`memset` use 64-bit `ld`/`std` (not in any 32-bit model; every Unicorn ppc64 model faulted on
  `isel` in a test). So `memcpy` (0x119164a0) and `memset` (0x11915e64) are replaced by Python. No other libc
  routine faulted on the paths run so far; another one would stop the run with "exception 96, instruction ...".
- glibc start-up runs for real (`_start` to the entry of `main`, which is not run) on a minimal Linux syscall
  layer (brk, anonymous mmap, write, nanosleep, futex stub, uname, ...). No SDK/diag-shell init is done; the
  `extphy` sub-command handlers are called directly as `handler(argc, argv)`. Nothing in the paths used
  needed the SDK (`soc_miimc45_*` is never reached: the two diag callbacks 0x11737db0/0x11737e20 are hooked).
- Time: `gettimeofday` is virtual (advances with the requested sleeps), so vendor time-outs count the vendor's
  own delays, not bridge latency. Sleeps are real on hardware. One MDIO read costs about 0.4 ms through ssh;
  writes are pipelined and flushed before every read or delay, so order is preserved but **write-to-write
  spacing is not the vendor's** (unverified whether any step depends on it; none failed on bus 1).
- Bridge: `stdbuf -o0 bcm.user` on the switch, synchronised with `echo MK<n>` markers (no prompt on a pipe;
  `@` is eaten by the diag shell). Reads print `	0x<reg>: 0x<val>`, writes print nothing.
- Safety: `fw_load` other than 0/1/2 is refused; entering `plp_evora_download_prog_eeprom` with its EEPROM
  flag (r8) set stops the run; so does a write that newly sets 1.8210 bit 1 or 1.8203 bit 10. (1.8210 reads
  0x0033 after power-up, bit 1 already set: the first version of this guard tripped on that and aborted one
  run in the middle of the bus 1 broadcast; the next full run recovered the group.)

## Trace (`dumps/x/evora-trace/init-cfg-0x24-0x27.trace.txt`, sha256 1708fc90687f..., 133094 lines)
Format: `R|W <phy> <devad> <reg> <value>`, `D <usec>`, `# marker`. 52106 reads, 68840 writes, 12140 delays
(6.8 s of requested delay); it ends with the `ver 0x24` and `status 0x24` that followed. Summary: `init-cfg-0x24-0x27.summary.txt` beside it.
- **Order:** every stage walks the ids ascending (0x24, 0x25, 0x26, 0x27); no "first id of the bus first"
  special case. Stages: per PHY ID read (1.0002, 1.0003, 1.8b00, 1.8b01) + core reset; per PHY broadcast
  enable; **the firmware words are written to the last id only (0x27: 24876 writes of 1.8222; the image is
  24032 words)** while the others listen (1.8222 is also the message-in register after boot, about 850 writes
  per PHY); per PHY verify; per PHY end; then MACsec init, bypass and mode config per PHY.
- **Reads that steer the code (polls/branches):** 1.900a (LMI status, 11753 reads; **clears on read**; 0x0001 =
  write done, 0x0007 = read data ready, bit 2 comes and goes and is ignored), 1.8221 (download handshake,
  28085 reads: 0xf1ac, 0x0ead, then 0x1000+n per 32 words, 0x0000 between blocks, 0x0303 at the end), 1.8213
  bit 1 (serboot busy), 1.8251 (& 0xd == 0xd: line/sys TSC active), 1.8210, 1.8245/1.8247 (0x600d),
  1.8b00/1.8b01 (ID), 1.8b90 (side select read-back). 1.9005 reads are LMI data (a FIFO: each read pops a
  word); most are read-modify-write inputs, a few are status (temperature, PLL). All other reads are RMW inputs.
  This classification is from the trace and the replay behaviour, not from source.
- **Direct dev-1 registers written** (function names are guesses; there is no register map): 1.8201 = 0x007e
  (in-band reset, bit 0 self-sets again); **1.8b10 = 0x0081 then 0x008b; 1.8b02 = 0x0004; 1.9011 = 0;
  1.8b03 = 0x8000/0; 1.8b11-1.8b14 = 0x9f3f then 0; 1.8b91 = 0x0004/0x0404/0x0406; 1.8a4c = 0x0a24;
  1.8255 = 0xbeaf; 1.826d = 1; 1.8b96; 1.8204** - these 0x8bxx/0x82xx chip-level registers are the
  candidates for reset / clock / pad / broadcast control; 1.82ff, 1.8202, 1.8210, 1.8222, 1.823x = download;
  1.8b90/1.9005 = LMI; 1.a000-1.a005 (3382 writes, read-back 1.a100/1.a101) = a second indirect port used by
  the MACsec init. LMI addresses written in the reset stage: 0x020a-0x020d, 0x0214, 0x0224, 0x0228 on both sides.
  **Nothing in the trace is identifiable as a GPIO or clock-output write to another chip**, but without a
  register map that is not excluded.
- **Replay:** `replay.py` re-does every write and delay; reads are compared. Recorded polls are collapsed, 1.900a
  is replayed read for read (clear-on-read, the delays around it matter), 1.9005 is never re-read. Live result
  on 0x24-0x27: 0 status/handshake mismatches, 39 reads of 1.900a differing only in bit 2
  (`replay4.result.txt`). A replay is only valid for a chip that takes the same branches; it stops after 3
  unresolved handshake mismatches. Not tried on another group or after a power cycle.

## Bus 0 (ids 0x00-0x03): what happened, in order
1. 21:38 (coordinator's scan): 0x00-0x03 all answer 0x2391. 21:57:00, my first read on the switch, before
   any write of mine: **id 0x00 reads 0x0000 on every register** and has done so since. (At 21:57:17 I wrote
   1.8b02 = 0 to id 0x00 as a write-syntax test; it was already dead.) Cause unknown to me.
2. 22:05 `init 0x01 3` (broadcast): id 0x02's first **system-side** LMI read (address 0x020a) never completed
   ("LMI Access timeout"); line side was fine. The download ran; id 0x03 verified (D00B); id 0x01 "Line/Sys TSC
   is not active" and shows no firmware (1.8215 = 1). In the "end" stage, right after the system-side LMI write
   0x020a = 0x047f0664 (the same write that succeeds on the line side and on every other PHY), **id 0x02 began
   returning 0x1900 for every register** and still does. Writing 1.8201 = 0x007e to ids 0x02 and 0x00 changed nothing.
3. `init 0x01 1 fw_load=1` and `init 0x01 1`: "ERR:SERBOOT BUSY BIT SET" (1.8213 bit 1 never clears). Id 0x01
   still answers its ID; 1.82ff reads 0x4000 (0x8003 untouched, 0x50c0 on a running PHY).
- Observation, not a conclusion: a hung PHY returns one constant for every register (0x0000 at 0x00, 0x1900
  at 0x02). The 11 ids silent since boot return 0xffff for every register, which has the same shape. Whether
  they are hung, unpowered or held in reset is not shown by anything here. The hang of 0x02 coincided with
  taking its system-side core out of reset.

## Open / unverified
- No link was tested (no healthy PHY has a cable; the ASIC ports were not enabled).
- Whether a power cycle revives ids 0x00/0x02 and cures 0x01; whether the remaining 13 answering PHYs init
  cleanly (not attempted, see Result).
- `mode=1/2`, break-out, `prbs`, `eyescan`, `polarity`, `pre`, `reg` beyond one `reg get` were not exercised.
- To add to the devshell later: `python3Packages.unicorn` (capstone was only used for my own disassembly).

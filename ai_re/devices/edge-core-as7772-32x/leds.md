# AS7772-32X front-panel port LEDs

2026-11-22. **Our own LED programs run at boot** (syncd loads them at SDK init) and every register read-back is as
predicted. **Nobody has looked at the panel yet**: colours, lamp positions and blink are the
[owner's check](#owners-check-by-eye-not-done). Known limit: a lamp shows the ASIC-to-PHY link, which is up
whenever the port is admin-up, **not the cable** (measured, below).

## How the factory drives them (verified offline unless marked)
Sources: `dumps/x/diag/root/usr/local/bin/` (`portled-*.soc`, `ES7632BTx.hex` sha256 a8636dec35bf, `LED_*.hex`)
and the diag's `bcm.user` (SDK 6.5.9, sha256 e163e815a5cf, PPC32, stripped), disassembled with binutils. The LED
programs were disassembled with the SDK's own `tools/led/tools/leddasm` (OpenBCM 6.5.27 tree).

- **Loading.** Every `rc.soc-*` ends with `portled-default` = `led auto off; led 0/1 stop; led 0/1 load ES7632BTx.hex;
  led 0/1 start; led auto on`. The same file goes to both LED processors (LEDUP0, LEDUP1); LEDUP2 (management
  ports) gets none. **No `CMIC_LEDUP*_PORT_ORDER_REMAP` write anywhere**: the order in use is the SDK's own, which
  Tomahawk's `init all` programs (`_soc_th_ledup_init`, `src/soc/esw/tomahawk.c`): LED port index = lane - 1
  (lanes 1-32) and lane - 65 (lanes 97-128) on LEDUP0, lane - 33 (lanes 33-96) on LEDUP1. The diag shell's linkscan
  callback uses the same index for the link byte, data RAM 0xa0 + index.
- **Serial format.** Each processor sends 224 bits per refresh = 16 cages x 14 bits:
  `H1 H0`, then for each of the 4 lanes `C1 C0 ON`. `H0` and `C0` are 0 in every factory program.
  The bits are not one-per-LED (a 40G/100G cage sends `0 0 0` for lanes 1-3, "off" sends `1 0 0`), so something
  after the ASIC decodes them (a CPLD, **inference**; [cpld.md](cpld.md) lists only the system LEDs, CPLD1 0x41, for LEDs).

  | factory test (`portled-<colour>`) | file | 14 bits per cage | meaning by the script's own comment |
  | --- | --- | --- | --- |
  | blue | LED_100G.hex | `10 101 000 000 000` | 100G: H1 = one 4-lane port, C1 = 1 |
  | yellow | LED_40G.hex | `10 001 000 000 000` | 40G: H1 = 1, C1 = 0 |
  | cyan | LED_25G.hex | `00 101 101 101 101` | 4 x 25G: H1 = 0, each lane C1 = 1 |
  | green | LED_10G.hex | `00 001 001 001 001` | 4 x 10G: H1 = 0, each lane C1 = 0 |
  | off | LED_off.hex | `00 100 100 100 100` | all lamps off |

  The test programs take no input: they send the constant pattern (no data RAM pokes; `portled-all-type` just
  loads them one after another, 2 s apart, then the default).
- **Cage order** (the part that replaces a remap): slot k of a processor's frame shows the 4 indices starting at a
  per-slot base. The factory keeps the bases in data RAM 0xe0-0xef, written by its patched `led <n> start`
  (code at 0x100287bc, tables at file offset 0x19772c4). With the SDK index above and this board's lanes
  ([extphy.md](extphy.md)) they come out as plain cage order, which is the cross-check that the decoding is right:

  | processor | slots 0-15 | base index per slot | first lane per slot |
  | --- | --- | --- | --- |
  | LEDUP1 | cages 1-16 | 16 20 24 28, 32 36 40 44, 0 4 8 12, 48 52 56 60 | 49 53 57 61, 65 69 73 77, 33 37 41 45, 81 85 89 93 |
  | LEDUP0 | cages 17-32 | 32 36 40 44, 16 20 24 28, 48 52 56 60, 0 4 8 12 | 97 101 105 109, 17 21 25 29, 113 117 121 125, 1 5 9 13 |

  A second table there (file offset 0x1977204) maps the same slots to the Evora PHY ids and matches
  `port_map-100G.csv` for all 32 cages. Which end of the chain is which physical LED is not knowable offline; we
  send in the factory's order.
- **Inputs of the default program**, per lane index: data byte 0xa0 + index, bit 0 = link, bit 5 -> `C1`, bit 4 -> `C0`;
  bits 7 and 6 of the cage's first lane -> `H1 H0`. `ON` = 0 without link, 1 with link; while the ASIC port status
  shows RX or TX frames, `ON` is 1 for one refresh in 16 only (so the lamp goes mostly dark under traffic).
- **Who writes those bytes: the factory `bcm.user`, not the stock SDK.** Its linkscan callback (0x10026960) writes
  the whole byte: bit 7 = the port has 4 lanes, bit 5 = speed is 25G or 100G, bit 0 = link, and copies bits 7:6
  into the cage's first-lane byte. **The link bit of a front port is not the ASIC's link**: a thread (0x10027030,
  "led task") asks the external PHY for each cage's link every 0.5 s and feeds that. Activity is the ASIC's.
  The stock callback (OpenBCM 6.5.27, `src/appl/diag/ledproc.c`) writes bit 0 from the ASIC link and clears
  bit 7, nothing else, and stock `led start` zeroes 0x80-0xff: the factory file cannot work in our stack as is.

## Ours
Own programs, written from the format above; **no factory file or byte is copied** (the two 16-entry cage orders are
board wiring facts). One per processor, 122 bytes each: `nix/modules/boards/as7772/led/{led0.asm,led1.asm}`,
assembled into `led_proc_init.soc` there, which `nix/modules/boards/as7772/default.nix` passes to
`hardware.as7760.ledProgram` (`SAI_OPENBCM_RC_FILE`: syncd `rcload`s it right after `init all`).

- Every cage is shown as **one 100G port**: `1 0  1 0 ON  000 000 000` (the factory "blue" when lit). The cage
  order is in the program (16 x `ld a,<base>; call cage`), inputs are only what the stock SDK provides: link =
  bit 0 of 0xa0 + index (`led auto on`), frames = RX/TX port status. No data RAM table, no remap, no speed bits.
- Lamp: off without link, on with link, **blinking 4 refreshes on / 4 off while frames pass** (deliberately not the
  factory's 1-in-16; `BLINK_BIT` in the source). Refresh: `CMIC_LEDUPn_CLK_PARAMS` 0x5b8d80; the tick counter
  advanced 3-4 per ~0.1 s read interval, i.e. roughly 30 Hz (rough: read timing, not measured).
- **Link source = the ASIC port's link to the PHY's system side, and that does not follow the cable** (measured
  2026-11-22): with Ethernet4 shut, Ethernet0 (the other end of the same DAC) stayed up; the empty cage 3 went
  oper up as soon as it was admin-up. So a lamp means "port admin-up and PHY alive". Correct today only because
  every uncabled port is admin-down. The same holds for SONiC's oper state. The fix is the factory's way: feed
  the PHY's line-side link (MDIO) into the link byte / the port state; not written.
- Not covered: 40G (would show blue; `COLOUR equ ZERO` gives yellow for all cages), break-out (needs
  `H1 = 0` and four lamps for that cage: a per-port-config generator, not written), management-port LEDs.
- Build and check (workstation, from `ai_re/`, in the devshell):
  `OPENBCM_SRC=<store path of nix/packages/openbcm .src> devices/edge-core-as7772-32x/led/build.sh`
  builds the SDK's `ledasm` with gcc, assembles, rewrites `led_proc_init.soc` and `led/pattern-*.soc`, then runs
  [led/ledcheck.py](led/ledcheck.py) (our emulator of the LED processor). Result (`led_proc_init.soc` sha256
  af9e0acc4913): for 200 random link patterns per processor our frame is **bit-identical to the factory
  program's** fed with the factory's 32 x 100G data bytes and order table; the five
  [led/pattern.asm](led/pattern.asm) variants are bit-identical to the factory `LED_*.hex` frames; blink 4/4.
  The emulator follows the SDK's `ledsim.c`; the chip gave the same frame bytes (below).
- If the `rcload` of the file fails, syncd does not create the switch (sai-openbcm README).

## Live test 2026-11-22 (register read-back; no eyes)
Transcript: [evidence/leds-live-20261122.txt](evidence/leds-live-20261122.txt) (sha256 eca3b8edb1f6). Only `led`
commands and register reads through `openbcm-diag`, plus the two approved runtime admin changes (reverted).
Tools: [led/led-run.sh](led/led-run.sh) (feeds a `.soc` line by line; `normal` reloads the system's own file),
[led/led-ram.sh](led/led-ram.sh) (data RAM bytes).

| cage / port | processor, slot | index | link byte | status byte (RX bit 0, TX bit 1) |
| --- | --- | --- | --- | --- |
| 1 / Ethernet0 | LEDUP1, 0 | 16 | 0xb0 | 0x20 |
| 2 / Ethernet4 | LEDUP1, 1 | 20 | 0xb4 | 0x28 |
| 31 / Ethernet120 | LEDUP0, 14 | 8 | 0xa8 | 0x10 |
| 32 / Ethernet124 | LEDUP0, 15 | 12 | 0xac | 0x18 |

- **Before:** both processors disabled (`LEDUP_EN=0`). Remap registers as the SDK source says
  (`LEDUP0 REMAP_PORT_0..3` = 31 30 29 28, `60..63` = 35 34 33 32, `LEDUP1 16..19` = 19 18 17 16). This SDK's
  `led <n> status` prints the port-to-index map itself (`ce0 1:17`, `ce30 0:9`, 1-based): the same indices.
- **Patterns:** all five loaded and ran on both processors; frame bytes 0x80-0x9b = the emulator's.
- **Our program:** link bytes 1 at exactly 0xb0, 0xb4 (LEDUP1) and 0xa8, 0xac (LEDUP0), all other 0xa0-0xdf 0;
  frame bytes equal to the emulator's prediction, byte for byte (LEDUP1 `15 40 05 50 00 14 00 05 40 01 ...`,
  LEDUP0 `... 05 40 01 50 01 54 00`); tick counter 0xf0 advancing.
- **Activity:** flood ping over cage 1-2: status bytes 0x20/0x28 read fb/f9 (idle f8), the lamp bits of cages 1 and 2
  toggle, cage 3's status and all of LEDUP0 unchanged; the mirror image for cage 31-32. 2 x 60,000 pings, 0 lost.
  (The host can read the status area at 2 x index: now seen.)
- **Admin / cable:** Ethernet4 shut: its link byte 0, lamp bit 0, **Ethernet0 stays up**; back up: 1 again.
  Empty cage 3 (Ethernet8) admin-up: **oper up, link byte 0xb8 = 1**; shut again: 0. Ports restored.
- **Deployed:** system `w87fmc0b...` = commit 17b4c25 plus this change only, built from a clean export of HEAD
  (the working tree held another agent's unfinished `nix/` edits, which give a different system and were not
  deployed). `nix store diff-closures` against the previous system `ymqdngj2...`: one new path, the `.soc`.
  After `deploy.sh`-equivalent `nixos-rebuild switch` (syncd restarted) and again **after a reboot**: both
  processors enabled, program RAM = our bytes, link bytes and frame bytes as above, activity bits follow both DAC
  pairs. `sonic-traffic-test.sh` after the reboot: 6400 pings 0 lost, counters equal, 0 failed units
  ([evidence/sonic-20261122-leds-reboot-traffic.txt](evidence/sonic-20261122-leds-reboot-traffic.txt), 8b03cb187a96).
- Left running: the normal program (loaded by syncd; re-loaded once more by `led-run.sh normal` as a test).

## Owner's check by eye (not done)
Only a person at the panel can do this. Nothing here touches ports or traffic. About five minutes.

Now, without typing anything (normal program, DACs in cages 1-2 and 31-32):
- cages **1, 2, 31, 32: one lamp lit each, expected blue**, steady; the other 28 cages dark.
- A lamp lit on any other cage, or a cabled cage dark: note which. That means the cage order is wrong.

Then, from `ai_re/devices/edge-core-as7772-32x` on the workstation:
```
sudo scp led/led-run.sh led/pattern-*.soc root@10.60.2.23:/tmp/
sudo ssh root@10.60.2.23
bash /tmp/led-run.sh /tmp/pattern-blue.soc      # expected: all 32 cages, one lamp each, blue
bash /tmp/led-run.sh /tmp/pattern-yellow.soc    # expected: all 32 cages, one lamp each, yellow
bash /tmp/led-run.sh /tmp/pattern-cyan.soc      # expected: all 32 cages, four lamps each (if a cage has four), cyan
bash /tmp/led-run.sh /tmp/pattern-green.soc     # expected: all 32 cages, four lamps each, green
bash /tmp/led-run.sh /tmp/pattern-off.soc       # expected: everything dark
bash /tmp/led-run.sh normal                     # back to normal: cages 1, 2, 31, 32 lit again
ping -f -c 20000 -q 10.0.1.1                    # ~40 s: lamps of cages 1 and 2 blink (about 4 per second), 31/32 steady
```
(If the ping has no route: the test addresses are runtime only; `bash -l sonic-traffic-test.sh` sets them and
makes all four lamps blink for a few seconds each.) Each pattern stays until the next command; a reboot or
`systemctl restart sonic.target` also returns to normal. The "expected" colours are the factory script names
(`portled-blue` = "100G" and so on), not something we have seen.

Please write down, per pattern: colour, how many lamps per cage and which position, any cage that differs; for
the normal state which cages are lit; for the ping whether 1 and 2 blink and roughly how fast. Optional: pull the
DAC from cage 31: by the measurement above the lamps of 31 and 32 are expected to **stay lit** (the known limit).

## Unverified
- Everything visible: colours, one or four lamps per cage, which physical cage a slot lights, blink rate.
- That the decoder ignores `H0`/`C0` (always 0 in factory data; we send 0).
- What the factory's PHY link poll reads (the call at 0x11737e60 was not followed), i.e. which PHY register a
  cable-true link bit would come from.
- The as7772 system was built and deployed from an export of HEAD plus this change, not from the working tree.

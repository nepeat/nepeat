# evora-rehost: tools

Scripts only; the vendor binary, traces, states and keys are read from `../dumps/` at run time (git-ignored).
Run from the ai_re devshell (it has Unicorn). Background: [../evora-rehost.md](../evora-rehost.md); rules:
[../extphy.md](../extphy.md) (MDIO only after `init misc`, clause 45 only, RAM firmware only).

| file | what |
| --- | --- |
| `rehost.py` | runs the vendor `extphy` sub-commands under Unicorn; records MDIO traces, saves/loads library state |
| `mdio.py` | MDIO backends: `mock` (no hardware), `ssh` (own `bcm.user` on the switch, SONiC stopped), `diag` (through syncd, SONiC up); trace format |
| `mkscript.py` | compiles traces into the boot-time MDIO script for `openbcm-mdio-script` |
| `install-script.sh` | copies that script to `/var/lib/evora-phy` on the switch (sha256 checked) |
| `phystat.py` | RS-FEC, PCS, PHY MAC counters and TX taps, SONiC running ([../load-test.md](../load-test.md)) |
| `macsec.py` | calls the library's MACsec API with our keys ([../macsec.md](../macsec.md)) |
| `lmi.py` | LMI (indirect register) decoder and mock model, used by `macsec.py` |
| `macsec_test.py` | offline checks of `macsec.py` (mock only, needs `dumps/`) |
| `macsec-live.sh` | the staged live MACsec test of 2026-11-22 |
| `seqtool.py`, `seqcheck.py` | decode a trace by register; compare `nix/modules/boards/as7772/ext-macsec.nix` (rendered by sai-openbcm's executor) with recorded traces ([../macsec.md](../macsec.md#under-sonic)) |
| `macsec-params.py`, `macsec-appdb.py` | random test SA parameter files for `openbcm-diag extmacsec run`; the same as APPL_DB entries for swssconfig |
| `idscan.py` | read-only chip-ID scan (1.8b00 / 1.8b01) of the 32 ids through the ssh bridge |
| `replay.py` | replays a trace through the bridge without the emulator (superseded at boot by `mkscript.py`) |
| `summarize.py` | trace statistics: registers written, polls, LMI addresses |
| `bringup.sh` | workstation-side init + cfg of a PHY group over ssh; never run as a wrapper |

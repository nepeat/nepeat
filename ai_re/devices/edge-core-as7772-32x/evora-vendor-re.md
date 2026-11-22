# AS7772-32X: BCM82391 "Evora" facts from the vendor `bcm.user`

Offline static RE, 2026-11-21; what was later confirmed on hardware is marked. Source: `dumps/x/diag/root/usr/local/sbin/bcm.user`
(sha256 e163e815…fc90, ELF ppc32 BE, stripped; .text VA 0x10000140 = file 0x140, .rodata VA 0x119653f8, .data VA
0x1238e290 = file 0x237e290, i.e. file offset = VA - 0x10000000 below 0x1238e1c0 and VA - 0x0fff0000 above).
Method: strings, a lis/addi cross-reference scan, and Ghidra decompilation of single functions (no full
auto-analysis). The Evora library (0x117c0000-0x118b0000) is built -O0, so the decompiles are literal.
"V" = read directly in the decompiled code; "I" = inferred. Addresses are VAs.

## 1. `extphy` command and MDIO path (V)
- Command table entry 0x12d9cb68 ("extphy", handler 0x11735b80, usage text 0x12d9cb88). Subcommands and handlers:
  `init` 0x117417f0, `cfg` 0x11741a40, `pre` 0x11741c80, `prbs` 0x11742010, `eyescan` 0x11742270, `status`
  0x11742860, `info` 0x11743000 (no args), `reg` 0x117431d0, `reg64` 0x11743660, `lba` 0x117416e0 (not in the
  usage text), `power` 0x117430b0 (not in the usage text), `ver` 0x11742430, `polarity` 0x11741e20.
- Usage text, condensed: `init <phyid> <num> [fw_load=0|1|2] [breakout=0|1] [speed=] [mode=]` or `init all
  [mode=]`; fw_load 0 = none, 1 = one by one, 2 = broadcast (default); mode 0 = MACsec bypass, 1 = bypass with
  break-out, 2 = MACsec enabled (power test). `cfg <phyid> [lane=] [nphy=] [speed= if_line= [if_sys=]] [fec=
  [breakout=]] [loopback=] [lt=] [flowctrl=]`. `eyescan <phyid> <lane> [sys]`. `polarity <phyid> [<lane> <tx>
  <rx>] [if_side=0|1]`. `pre <phyid> [<lane> [<pre> <main> <post> <post2> <post3> <amp>]]`. `prbs
  config|clear|status <phyid> [<lane> ...]`. `reg|reg64 chkecc|cmp|get|set <phyid> <regaddr> [...] [devad=N]
  [lane=] [if_side=]` (if_side 0 = line, 1 = system). `status <phyid>|all [lane=] [breakout=]`, `status <phyid>
  dsc|ecc|lts|pmdiag`. `ver <phyid>`. Parser defaults (0x12d9de30): fw_load 2, lane 4 (= all), speed 100000, nphy 1.
- Interface names -> values (table 0x12d9db60, 0x18-byte entries; only the tail was dumped): LR4 0x1c, SR 1,
  SR4 2, SFI 0x10, XFI 0xf, XLAUI 0x18, ZR 0x2a, **CAUI 0x1a**, CAUI4 0x24, **CAUI4_C2C 0x28**, CAUI4_C2M 0x29.
- All chip access is **plain clause-45 MDIO through the switch's CMIC**: the diag hands the PHY library two
  callbacks, read 0x11737db0 and write 0x11737e20, which call `soc_miimc45_read/write(unit 0, phy_id & 0xffff,
  devad = (reg >> 16) & 0x1f, reg & 0xffff, data16)` (0x10b82560 / 0x10b82530; the names are my identification
  from the argument shape, I). Writes are 16 bits; the library's 32-bit "value | mask << 16" words lose the
  mask at this layer. `platform_ctxt` is the constant 5 (only tested for non-zero).
- **phy id = SDK phy_id**, passed unchanged. Table 0x12331120 (32 x 8 bytes: bus, index, phy id) lists
  bus *b* (0-7) -> ids 0x00-0x03, 0x24-0x27, 0x48-0x4b, 0x6c-0x6f, 0x110-0x113, 0x134-0x137, 0x158-0x15b,
  0x17c-0x17f: MDIO address 4*b*+0..3 on external bus *b* (bits 6:5 and bit 8 = bus). `init all` walks this
  table in groups of 4. Port table: [extphy.md](extphy.md).

## 2. Chip identification (V)
- `bcm_plp_init` for Evora (0x117f77e4, slot +8 of the API table 0x12dc348c) probes, in order: dev 1 reg 0x0002,
  0x0003 (**values ignored**), then **dev 1 reg 0x8b00 and 0x8b01**. It accepts 0x8b00 == **0x2391** or 0x2392.
  The full id is `((reg 0x8b01 >> 12) & 0xf) << 16 | reg 0x8b00` = 0x82391 / 0x82392 (0x11835910, used by
  core_identify 0x1181e9ac). If 0x8b00 does not match it reads 1.8000 and returns -26.
- Confirmed live: 0x8b00 = 0x2391, 0x8b01 = 0x80a0 on all 32 ids; 1.0002/1.0003 read 0x0000.
- The vendor does this read as its first access to a PHY, with no write before it and no reset, GPIO, CPLD or
  I2C step anywhere on the path (section 5). Confirmed live: the PHYs answer after power-up with only the CMIC
  MDIO set up by misc init.

## 3. Firmware (V unless marked)
Extracted to `dumps/x/evora-fw/` (git-ignored; `SHA256SUMS` there):
| file | VA / file offset | length | sha256 |
| --- | --- | --- | --- |
| `evora-master-fw.bin` | 0x12daaaf8 / 0x2d9aaf8 | 0xbbc0 (48064; length word at 0x12dc4834) | 2468566c1db6…0d17 |
| `evora-falcon-ucode.bin` | 0x12dbac88 / 0x2daac88 | 0x7ca4 (31908; length at 0x12dc483e) | 5f069688271e…d33a |
- Master image (the only one referenced, by 0x118375cc): magic "Evor", then bytes f0 c4 28 a0 ff ff **23 91**
  d0 0b 00 1f; 16-bit LE word 0x0d = 0x3b44 = payload size. Layout I: 0x40-byte header, section A 0x40-0x3bbf
  (0x3b44 rounded up to 0x40), section B 0x3bc0-0xbbbf (0x8000 bytes, starts with an ARM vector table
  f00f0020 3d020000). Version/CRC fields in the header are **not decoded**; the driver reports version from
  reg 1.8215 and a status word from 1.8245 after boot.
- Falcon SerDes microcode: referenced by the tscf/falcon core_firmware_load functions (0x118a6d9c, 0x118719a0).
  The words next to its length (0x12dc483c = 0xd10b, 0x12dc4840 = 0xa192) are I: version D10B and CRC A192.
  How and when it is loaded into the two port macros was not traced.
- **MDIO download** (delays assume the sleep helper 0x11920420 takes microseconds, I)
  (`plp_evora_download_prog_eeprom` 0x11835cf0, called with words = len/2, each word
  `fw[2i] | fw[2i+1] << 8`). All registers are dev 1; "set"/"clear" are read-modify-write:
  1. 0x8a4c set 0x0a00. 0x82ff clear bits 15:14. 0x8202 bits 3:0 = 0xc. Poll 0x8213 bit 1 == 0 (500 us x 1000).
  2. 0x8210 set 0x31; read back, need bits 5:4 = 3 and bit 0. (EEPROM method only: 0x8210 set bit 1, 0x8203 set bit 10.)
  3. 0x82ff: clear bits 15, 12, 7, 6, 1, 0 and set bit 14. 0x8202 set bit 1, then set bit 0. Poll 0x8213 bit 1 == 1.
  4. Wait message-out reg **0x8221** == 0xf1ac, write message-in reg **0x8222** = 0. Wait 0x8221 == 0x0ead, write 0x8222 = 0.
     Write 0x8230 = 0, 0x8232 = 0, 0x8236 = 0.
  5. For each word *i*: wait 0x8221 == expected, then write 0x8222 = word. Expected starts 0x0ead (i = 0),
     becomes 0x1000 at i = 0x20, 0x2000 at the start of section B, and increments by 1 every 32 words otherwise
     (a mismatch aborts). At i = 0x1f: read 0x8230 and 0x8236; if (0x8230 & 0xff) == 1 and 0x8236 != 0, write 0x8230 = 2.
  6. Verify (0x11836eec): poll 0x8251 & 0xd == 0xd (100 ms x 20; "Line/Sys TSC is not active" on timeout), read
     0x8252 ("GPREG2"), need 0x8221 == **0x0303** (0x0404 after an EEPROM load), read 0x8230 and 0x82ff, then
     0x8245 + 2n must read **0x600d** for each n below a count taken from 0x8230.
- Around it (`evora_core_init` stage function 0x11820008, called four times by 0x118114f4 with flags 0x100,
  0x80, 0x2, 0x200): before the download 0x8201 clear bit 0, a delay of 30 units, 0x8b10 set 0x1 then 0xa, 0x8b02 = 4,
  0x9011 = 0; with method 3 (EEPROM) it refuses when 0x82ff bit 14 ("serboot pin") is set. Stage 0x80 clears
  0x8204 bit 0. Stage 0x2 verifies and writes 1 to internal register 0x40020210. Stage 0x200 was not read.
- Broadcast download (`bcm_plp_init_fw_bcast`, API slot 0x12dc35a4, steps CoreReset, Enable, FirmwareExecute
  once, Verify, End per PHY) is what `init all` uses by default; its register sequence was **not traced**.
  `fw_load=1` uses the per-PHY path above.

## 4. Init and config for 100G MACsec bypass (V for the call order, not the register writes)
`extphy init` body 0x1173ad60, per group of PHYs, each call with access {ctx 5, phy id, if_side, lane map 0xf}:
1. Firmware: broadcast steps, or `bcm_plp_init(access, mdio_read, mdio_write, method)` per PHY.
2. Per PHY, egress then ingress (0x1173aa80): `bcm_plp_cfye_device_init` (slot 0x12dc3674), `bcm_plp_secy_device_init`
   (0x12dc36f8). The MACsec engine is initialised **even for bypass**.
3. Per PHY and side: `bcm_plp_secy_config_set` (0x12dc371c), then `bcm_plp_secy_bypass_set(access, 1)` (0x12dc3718).
4. `bcm_plp_mode_config_set` via 0x117387e0: line side (if_side 0) then system side (if_side 1), speed
   100000, interface 0x1a (CAUI) on both by default (40G: 0x1a line, 0x18 system; 10G/25G: 0xb).
`extphy cfg ... speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1` (0x11741a40) then repeats step 4 with line
0x28 and system 0x1a for each of `nphy` PHYs and calls `bcm_plp_fec_enable_set(access, 1)` (slot 0x12dc354c).
- Register level: only the access layer was read. Besides direct dev-1 registers, the library reaches the
  internal blocks (two PM4x25-style port macros = CLMAC + TSCF PCS + Falcon PMD per side, and the MACsec core)
  through an indirect "LMI" interface: status reg 1.900a (bit 0 busy/done, bits 8:3 errors), command/data FIFO
  reg 1.9005, side select reg 1.8b90 (0x1183723c). Internal addresses carry a type in bits 31:24 (0x40-0x46,
  0x49; classifier 0x1182593c, LMI access 0x11826108). The mode-config writes themselves run through the
  embedded portmod/tefmod/falcon code and were **not** extracted statically; they are in the re-host traces
  ([evora-rehost.md](evora-rehost.md); LMI protocol decoded in `evora-rehost/lmi.py`, MACsec calls in
  [macsec.md](macsec.md)).

## 5. Reset / power (negative result)
- `bcm.user` has no `/dev/i2c` string and no CPLD string; the Evora init path does no GPIO or board access before
  the ID read. The only reset it does is in-band (reg 0x8201 bit 0).
- No script in `usr/local/bin` or `usr/local/sbin` writes CPLD2/CPLD3; `cpldutil` has no PHY option. The strings
  "1.1_EXTPHY" and "1.0V(EXTPHY_CORE)" in `bcm.user` (0x1197637c, 0x1197694c) belong to a voltage-monitor name
  table, i.e. rails that are monitored, not switched (I: not traced to code).
- CPLD2/3 0x20-0x2b therefore stay undecoded from the vendor side.

## Not verified / open
- The MDIO function names, the firmware header layout, the Falcon version/CRC words and the interface-enum
  meaning beyond the name table are inferences.
- Broadcast download, stage 0x200, the Falcon microcode load path and every write below `bcm_plp_mode_config_set`
  and `fec_enable_set` were not traced statically (they run correctly under the re-host).
- The per-PHY download steps of section 3 were read from the code, not compared with a trace.

# 400G + MACsec on the cage 1 <-> 16 loop (2026-11-15)

Result: **fpp1 <-> fpp16 at 400G through both Credo PHYs, error-free at
~386 Gb/s per direction, and the same with static-key MACsec
(GCM-AES-128 and -256) on the line between them.** Encryption is proven
on the wire (ciphertext with SecTAG seen when one end is in bypass) and by
a wrong-key test (that direction drops, counted as InPktsNotValid).
Everything below ran by hand on the vendor stack (SDK 0.1.7, Credo FW
1.92.2); nothing persists. Evidence: `analysis/macsec-400g-20261115/`
(git-ignored; the `*.cmd` and `keys*.json` there hold the throwaway test
keys, do not publish them).

## Module

One cable, both ends read the same: **Mellanox CAB-Q400-P1M rev 01, S/N
MEN11Q80008, 2026-08-11, 1 m passive copper QSFP-DD DAC** (CMIS 5.0, flat
memory, so no pages 01h/11h; media type 03h passive copper, connector 23h
no separable connector, length byte 41h = 1 m, max power 1.5 W, module
state ModuleReady). Application 1: host 1Dh **400G CR8**, 8 lanes
(53G PAM4, RS-544); also 200GBASE-CR4 and 100GBASE-CR4 (x2), 50GBASE-CR and
25GBASE-CR CA-L (x8), IB EDR / HDR. Codes decoded with the SFF-8024 table
in sonic-platform-common. Dump: `qsfp-info-1-16.txt` (sha256 64b7b861…).
Reading it needs `cs8210-qsfp --enable` (after a power cycle), `--reset P 0`
and `--init P` (what the vendor probe does) on the two cages.

## Bring-up (bypass)

- Mode: Credo line side **`copper_1x400G`** (the loader then sets
  `PORT_LINE_SIDE_ANLT+PORT_AUTONEG_DISABLE`: line link training, no AN,
  RS-544 both sides); ASIC 1-16 `copper_1x400G` as always (host LT). Runtime
  drop-in used: `CS8210_CREDO_PORTS=1:copper_1x400G:fec-def;2-15:optics_1x400G:fec-def;16:copper_1x400G:fec-def`,
  `CS8210_INNV_PORTS=1-16:copper_1x400G:fec-def;17-32:optics_1x400G:fec-def`.
  Link came up on the first start, four times out of four.
- Credo: host and line PCS up on slices 1 and 16, lanes ready. Credo FEC
  analyser not enabled, so line-side BER is not measured.
- ASIC (host side) RS-544: 0 uncorrectable; corrected codewords ~2/s on
  devport 1 and ~50-100/s on devport 16 at line rate (order 1e-6 of
  codewords). Slice 16's host link is the noisier one.
- devport 16 always shows **5 CRC errors right after start**, never more:
  the frames before `cs8210-switch-up` turns double_CRC off on slice 16.

## Traffic

| path | bypass | MACsec |
| --- | --- | --- |
| ping -f, 1400 B | 100k/100k | 50k/50k (256), 20k/20k (128) |
| iperf3 TCP, CPU path | 2.77 Gb/s (1 stream), 2.5 Gb/s (-P4) | 2.76 / 2.52 Gb/s |
| iperf3 UDP 8900 B | 3.91 Gb/s, 0.04% loss (CPU RX) | 3.82 Gb/s, 0.7% |
| ASIC loop, 1500 B, one way | ~390 Gb/s (5 s sample) | — |
| ASIC loop, 1500 B, both ways | 386 + 386 Gb/s | 385 + 385 (128), 386 + 386 (256) |
| ASIC loop, 64 B, one way | — | 219 Gb/s L2, ~403 Mpps (256) |

All rows 0 frame errors, 0 FEC uncorrectable, 0 MACsec transform/
not-valid errors; ASIC tx frames = far-end rx frames. Files:
`bypass-cpu-path*.txt`, `bypass-hwloop.txt`, `macsec-hwloop.txt`,
`macsec256-hwloop.txt`.

- **CPU path:** fpp1 / fpp16 in netns `lpa` / `lpb` (198.18.0.1/.2).
  **fpp MTU 9462 breaks TCP**: TSO sends frames >= 9216 B, which the ASIC
  counts as `tx_frames_err_over` and drops (TCP stalls at 140 kb/s). With
  MTU 9000 it works. The SDK sets 9462; it should be <= 9216 - 18.
- **ASIC loop** = [`scripts/loop400_fwd.py`](scripts/loop400_fwd.py), run
  inside innovium.user (`cls_shell 'run loop400_fwd add|drop|fwd|del'`):
  two static FDB entries in the knet L2VNIs make a frame leaving port 1
  come back out of port 1 after crossing the cable (and the same for 16).
  [`scripts/l2frames.py`](scripts/l2frames.py) seeds it from Linux
  (300 x 1500 B per direction fill the pipe; one frame alone gives 12 Gb/s,
  i.e. ~1 us loop time). `drop` stops it at once. The vendor `diagtest
  snake` was not usable on top of knet: it creates L2VNIs 1..N, which
  collide with knet's.
- The rates are exactly what the vendor's **48-byte IPG** (devport YAML
  `ipg: "48"`) allows: 1508 B frames -> 400 x 1508 / (1508 + 8 + 48) =
  385.7 Gb/s; 68 B frames -> 400e9 / (124 x 8) = 403 Mpps. MACsec's 32 B
  per frame fit into the 36 B of extra gap, so encryption costs nothing on
  top, and nothing drops. Without MACsec a 12 B IPG would give ~2% (1500 B)
  to ~40% (64 B) more; not tried.

## MACsec

- The vendor stack configures MACsec itself: `c-phy/auto_load_user.sh -e
  GCM-AES-128|GCM-AES-256|GCM-AES-XPN-*` keeps the port in **MacsecBypass**
  mode and adds `macsec_datapath_init`, `macsec_enable` and one ingress +
  one egress SA per slice (`macsec_allocate_sa` sci/an, `macsec_sa_set`
  flags/sak/ssci/salt/auth, `macsec_vport_add` match-all rule) with a fixed
  built-in key and one SCI for everything. Engine: EIP-164/163 via
  `libdriver_164_u.so`; counters `SecY_SA_Statistics_Get`,
  `SecY_SecY_Statistics_Get` (phy_shell).
- **`auth` is the GCM hash subkey H = AES_K(0^128)** (V: matches the
  vendor's key/auth pairs for 128 and 256).
- [`scripts/credo_macsec.py`](scripts/credo_macsec.py) writes the same
  command list with our own random keys and one SC per direction (SCI =
  port MAC + 0001, SAK_AB / SAK_BA); loaded at runtime with `phy_shell
  load cfgfile=…` on a running bypass stack: no link drop, no SDK restart.
  double_CRC stays **off** (as in bypass) and MACsec works.
- On the wire: SecTAG with SC=1 (SCI included), E=1 C=1 (confidentiality),
  AN 0, PN from 1; +32 B per frame (16 SecTAG + 16 ICV).
- **One-sided check** (slice 1 MACsec, slice 16 bypass): fpp16 receives
  ethertype 0x88E5 frames, SCI = fpp1's MAC + 0001, PN 1, 2, 3, payload
  unreadable (`macsec-one-sided.txt`).
- **Wrong key** (slice 16 ingress SAK changed): 1 -> 16 drops completely,
  slice 16 InPktsNotValid = every frame sent; 16 -> 1 keeps working
  (`macsec-wrong-key.txt`).
- **Rekey with the vendor CLI (V):** `macsec_sa_set` after `macsec_vport_add`
  changes nothing on the live SA, and `macsec_datapath_flush` + a new SA does
  not replace the old one (traffic kept using the old key; the new SAs show 0).
  Why, and the hitless path the engine has: [macsec.md](macsec.md).
- The CLI prints only the low 32 bits of the (64-bit) SA counters, so its
  octets wrap every ~0.1 s at line rate; `cs8210-macsec status` shows all 64.
- These runs used the vendor's `SAB_MACSEC_FLAG_ROLLOVER` (PN may wrap, I),
  with throwaway keys; [macsec.md](macsec.md) "Other findings".

## Not done

- Hitless rekey, MKA, XPN, counters: implemented offline, hardware pending,
  in [macsec.md](macsec.md).
- Interop: only Credo <-> Credo was tested; the line-side FEC analyser was not
  used.
- Persistence: `hardware.cs8210.vendorSdk.phyPostLoad` (new, not deployed)
  loads such a command file after every bring-up; the file lives on the
  box (it holds keys).

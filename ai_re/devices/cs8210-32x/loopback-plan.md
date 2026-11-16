# Loopback test plan (DACs between front ports)

Status: superseded 2026-11-15 by the 400G DAC loop on cages 1 and 16: results in
[macsec-400g.md](macsec-400g.md). Written 2026-11-10 before the cables
arrive. No script yet on purpose: the Credo/SAI tooling is still changing
(`credo-sai/`). Turn this into `cs8210-loopback` once that settles.

Cables ordered: 2x 100G QSFP28 DAC, 1x 400G QSFP-DD DAC, QSFP28->4x25G and
QSFP+->4x10G breakouts, QSA + 10G SFP+ DAC.

## What a MACsec-port loop covers

    fpp1 -> ASIC -> Credo slice A (encrypt) -> line SerDes -> DAC
         -> line SerDes -> Credo slice B (decrypt) -> ASIC -> fpp2

- Physical layer: Credo line-side SerDes, EQ and FEC over a real cable.
  Credo PRBS (`set_tx_prbs` / `set_rx_prbs` / `get_rx_prbs_ber`) measures
  BER with no framing.
- I2C FPGA: presence/reset bits, DAC EEPROM read over our I2C engine
  (`cs8210-qsfp`, first hardware use; confirms the present/reset polarity).
- ASIC: link up, frames through the `fpp` netdevs.
- MACsec: A's egress SA and B's ingress SA use the same key.

## Port choice

- Chips pair up as ports 1+2, 3+4, ... (one CMS50216, two slices).
  **Port 1 <-> port 3** crosses two chips; 1 <-> 2 stays in one chip.
- Baseline: the same loop on two non-MACsec ports (17-32, ASIC -> cage
  directly). If 1-16 fail and 17-32 pass, look at the Credo path.
- First cable: 100G QSFP28 DAC (fits QSFP-DD cages). Both ends need the
  same mode on the ASIC and Credo side (1x100G, RS-FEC). Then the 400G
  QSFP-DD DAC.

## Order

1. Plug in. Presence bit in BAR0 `0x140` flips, release ResetL (`0x178`),
   read the DAC EEPROM through the channel. Also gives the first
   channel <-> cage mapping for the FPGA netlist (`ch00..31` are pad order).
2. Link up in MACsec bypass.
3. Credo PRBS BER on the line side.
4. Linux: put `fpp1` and `fpp3` in separate network namespaces (else the
   kernel delivers locally and skips the wire), then ping/iperf between them.
   The CPU path (10G internal link + DMA) is far below line rate. Use PRBS or
   the ASIC's snake test for full-rate checks.
5. MACsec on A only, B in bypass: `tcpdump -i fpp3` must show ethertype
   **0x88E5** with scrambled payloads. This proves the engine is in the path
   and answers whether the vendor default ever encrypted.
6. MACsec on both, same key: plain frames arrive, SA counters move
   (`macsec_sa_database_dump`, later SAI stats).
7. Negative control: mismatched keys -> drops + error counters.

Use our own throwaway key, never the vendor's published default (fine on a
closed loop, but keep the habit).

## Limits

- Same chip family on both ends: symmetric non-standard behaviour would not
  show. Real interop needs a different MACsec peer.
- Linux-side throughput is CPU-path limited (see step 4).

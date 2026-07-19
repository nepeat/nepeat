# SOIC-8 SPI flash dump — first-timer how-to (targeting U118 = W25Q64JV)

Dumping the 8-pin SPI NOR (`U118`, Winbond W25Q64JV, 8 MiB) in-circuit with a clip + CH341A.

## Gear needed (a 4-pin clip will NOT work — SPI needs 6+ contacts)
- **SPI programmer**: CH341A USB (cheapest/common), or Raspberry Pi + `flashrom`, or FT2232H.
- **SOIC-8 test clip** (8-pin): Pomona 5250 is the gold standard; cheap "SOIC8 SOP8" clips work for a few dumps.
- Female-female dupont jumper wires (if the clip has flying leads rather than a 1:1 adapter board).
- A host running `flashrom` — already in our `ai_re` devshell.

## ⚠️ #1 safety rule: 3.3V only
W25Q64JV is a **3.3V** part. The common black CH341A boards can drive the SPI lines at **5V**, which can damage a
3.3V flash. Before clipping: use a 3.3V-safe CH341A (the "3.3V mod"), a 1.8/3.3V adapter board, or a Pi (3.3V native).
If unsure, measure the programmer's VCC pin with a multimeter first — it must read ~3.3V, not ~5V.

## W25Q64JV SOIC-8 pinout (standard 25-series)
Pin 1 is marked by a dot/dimple on the chip corner.
| Pin | Signal | | Pin | Signal |
|----:|--------|--|----:|--------|
| 1 | CS# | | 8 | **VCC (3.3V)** |
| 2 | DO (MISO) | | 7 | HOLD#/IO3 |
| 3 | WP#/IO2 | | 6 | CLK |
| 4 | **GND** | | 5 | DI (MOSI) |

CH341A→clip wiring (only if flying leads; a 1:1 clip-adapter maps these automatically):
CS#→CS, DO→MISO, CLK→CLK, DI→MOSI, VCC→3.3V, GND→GND. WP#(3) and HOLD#(7) pull to VCC (usually fine left as-is).

## Steps
1. **Board OFF and unplugged.** Do all clipping with no power to the appliance.
2. **Find pin 1** on U118 (dot/dimple corner). The clip's **red wire = pin 1** — align them.
3. **Clip on squarely.** The clip's tiny teeth must land on all 8 legs. Clamp straight down, gently rock to seat.
   This is the fiddly part — first-timers often get one row misaligned. Check both rows contact the legs.
4. Connect clip → programmer → USB into the host.
5. **Detect (read-only, safe):**
   ```
   flashrom -p ch341a_spi          # or -p linux_spi:dev=/dev/spidev0.0,spispeed=2000 on a Pi
   ```
   Expect it to name a W25Q64JV (or list a few candidates with the same JEDEC id `EF 4017`).
   - "No EEPROM/flash device found" → bad clip contact or wrong orientation. Re-seat, check pin 1.
   - Garbage/rotating ids → contention from the board (see below).
6. **Read twice and compare (proves good contact before you trust the dump):**
   ```
   flashrom -p ch341a_spi -r u118-dump1.bin
   flashrom -p ch341a_spi -r u118-dump2.bin
   cmp u118-dump1.bin u118-dump2.bin      # must be identical
   ```
   Keep the dumps here in the device dir.

## In-circuit gotchas (x86 board)
The flash shares its SPI bus with the platform PCH/EC, which can fight the reads even with the board off (leakage
back-powering rails). If detection is flaky or the two reads differ:
- Try again — reseat the clip, it's usually contact.
- Some boards need the PCH held in reset, or a lower `spispeed`.
- Last resort: **desolder U118** (hot air) and read it in the programmer's ZIF socket — 100% reliable, more work.

## After a good dump
- `binwalk u118-dump*.bin` and `strings` to see what it holds (coreboot? U-Boot? EC fw? TAm data?).
- Compare U118 (8 MiB) vs a later U97 (16 MiB) dump to map the platform-fw layout.

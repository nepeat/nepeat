# The unlock codes are an RPMB-backed per-boot nonce — credentials are NOT portable

This closes the portability question opened in [unlock-scheme.md](unlock-scheme.md),
and it closes it against us. All key claims below were **independently verified**
by direct read of the binaries, not taken on report.

## Where the codes come from

The ten 32-byte codes reach LK as a **Linux-style ATAG in the preloader→LK
hand-off**, not from flash and not built at runtime.

| step | evidence | verified |
| --- | --- | --- |
| boot-arg magic is `"LPLP"` (MTK `BOOT_ARGUMENT_MAGIC`) | literal `0x11078` = `0x504c504c` | ✔ |
| blob pointer is `r4` at LK entry | literal `0x70` = `0x56000020`; `0x24: ldr r6,[pc,#0x44] / str r4,[r6]` before MMU-off/self-relocate | ✔ |
| record layout `{u32 size_words; u32 tag; payload…}` | loop tail `0x111b2 ldr r6,[r4] / add.w r4,r4,r6,lsl #2`; terminator `0x10d9a cmp r1,#0` | — |
| codes tag = `0x886100A7` | literal `0x11098`; `0x10f08 beq.w 0x11184` → `0xe534` | ✔ |
| preloader emits that tag | single occurrence in `yacht_preloader.bin` at `0x200b4` | ✔ |
| record size `0x53` words = `0x14C` = 8 hdr + `0x144` | `0x20032 movs r3,#0x53`, `0x20036 mov.w r2,#0x144` | ✔ |

> Note the earlier "16-bit tag at +2" reading in `unlock-scheme.md` was a
> mis-split of the 32-bit tag word. Payload at `+8` was right.

**It is not reachable from any writable partition.** Swept every dump for both
markers — `boot_para`, `misc`, `nvcfg`, `gpt`, `idme_boot1`, `kb`, `dkb`, `keys`
all contain **zero** `LPLP` and **zero** aligned `0x8861xxxx` words. The tag
family appears only in `preloader_boot0.img`, `lk.img`, `recovery.img`, `boot.img`
and the TEE images.

## How they are derived — and why that ends it

The preloader generates them at `0x252c` from a secret in **eMMC RPMB block 1**:

- 4-byte magic at the head of a 256-byte buffer resolves to **`"AZTU"`**
  (`0x2ef73`, verified) — the same magic as the IDME cert container.
- RPMB access built at `0x1bfdc` / `0x1bde8` / `0x1c0dc` with JEDEC request
  types **4 / 2 / 3** (Authenticated Data Read / Read Write Counter /
  Authenticated Data Write), each with a fresh 16-byte nonce from the RNG at
  `0x2ba24`; address field = block 1.
- On first use the device mints **32 random bytes** via its own RNG and sets
  counter = 0.
- **Every boot**: `[r4+4]++`, block rewritten to RPMB.
- Codes: for `c = base … base+9`,
  `HMAC-SHA256(S_device[32], LE32(c)) -> code[i]`, output length asserted `0x20`.
  Count word written as `0` (failure) or `0xa` — matching LK's `count <= 10`.

LK's own string closes the loop: **`"Device is temporarily unlocked, %d reboots
remaining"`** (`0x48235`, verified), and `0x1fda`–`0x1fe0` passes `r1 = r4`, the
**index of the matching code**, as that `%d`.

### Verdict: not portable, and self-expiring

`code = HMAC(S_device, counter)` where `S_device` is 32 bytes of that unit's own
RNG output, sealed in RPMB. Two units collide with probability 2⁻²⁵⁶. The
counter advances **once per boot**, so the ten-code window slides forward
continuously.

Therefore:

* a recovered `t_unlock_cert` + `t_unlock_code` pair **cannot unlock a different
  device**;
* on the originating device it expires after at most **10 reboots**.

Amazon's temp unlock is a genuine **online challenge/response** — read the
current codes off the unit, have Amazon sign one, write it back — not a portable
credential. **One leaked pair does not unlock the family.**

This reverses the optimistic framing added earlier today. Installing a
credential is easy (`flash:tucert` + `flash:tucode`, both allowlisted); there is
simply nothing durable to install.

### Why the write primitives cannot reach the secret

RPMB writes require a MAC under the RPMB key, i.e. preloader/TEE-level code
execution. `flash tucert`/`tucode` write **IDME** fields and cannot touch RPMB.
LK contains no RPMB code at all — its only trace is passing
`androidboot.rpmb_state=%d` through. Overwriting IDME `unlock_version` does not
help either: it is a separate field with no role in tag `0x886100A7`.

## Anti-rollback on this unit is RPMB-backed, not eFuse-backed

Directly relevant to safety, because the eFuse variant would be irreversible.
`strings` on `yacht_preloader.bin`:

```
[RPMB] Failed to initialize anti-rollback block
[RPMB] Valid anti-rollback block with %s magic exists
[ANTI-ROLLBACK] image versions in RPMB block:
[ANTI-ROLLBACK] Need to update version
[ANTI-ROLLBACK] %s version mismatch!   /   L: %x R: %x
[ANTI-ROLLBACK] Updating RPMB block...
[ANTI-ROLLBACK] Unable to update RPMB block (get wc error)
```

Unambiguously **RPMB-backed**. No fuse-burning ARB routine is indicated. (Some
Amazon devices — e.g. koboreru's `crumpet` config, which carries the comment
*"seriously, ARB on this device USES FUSES"* — are the other variant. `yacht` is
not, on this evidence.)

**Consequence:** ARB still blocks downgrade, and ARB-clearing is a *product* of
preloader code execution rather than a route to it. Downgrade is not an entry
vector.

## koboreru marker: ABSENT

The `koboreru` preloader bug is identified by the string
`check_part_overlapped done`. Grepped both `yacht_preloader.bin` and
`preloader_boot0.img`:

```
(marker ABSENT)   — and no partition-overlap strings of any kind
```

**Interpretation, carefully:** absence is evidence *against* koboreru applying,
but it is **not proof the code is patched** — the string could have been
compiled out while the routine remained. Confirming it properly means diffing
against the public **`maverick`** (Fire HD 10 2019, KFMAWI, MT8183) preloader,
which is available because that device is fully unlocked.

## Scene survey — the negative is now clean

Family-name corrections that matter for searching:

* **`mustang` is the wrong family** — it is Fire 7 9th gen (2019), **MT8163**,
  fully unlocked. Searching it lands on the wrong SoC entirely.
* The correct MT8183 sibling is **`maverick`** (Fire HD 10 2019, KFMAWI), which
  k4y0z unlocked via a BROM/kamakiri port. Dead as an exploit for us (BROM
  fused), but it means a **public Amazon-signed MT8183 preloader exists as diff
  material**. A `trona` dump also exists (`testandroidtests/amazon_trona_dump`).
* Fire HD 8 12th gen and HD 10 13th gen (`tungsten`) are **MT8169** — nothing
  cross-flashes.

Independent corroboration and closed doors:

* **ConnorLabsYT independently reproduced our unforgeable-unlock finding** from
  outside this project: `fastboot flash unlock` returns `signature length error`
  / `unlock signature verify failed, do nothing!` rather than the restricted-command
  error, concluding there is an unlock-token parser that only accepts
  Amazon-signed tokens. Matches our `dumps/unlock-length-probe.txt` exactly.
* **No hardware BROM fallback.** On the sibling HD 8 board every test point was
  shorted — "some do nothing, some return you to preloader, and some just
  completely prevent powerup" — and **none produced BROM**. A hidden UART exists
  on USB-C pins 3/6 but the RXD/TXD pads are disabled in firmware.
* **`fastbrick`** (used on Echo Shows): Rortiz2 states MT8183 is not vulnerable,
  and notes Amazon used **different signing keys per device family** — so no
  `maverick`-signed image will validate on `yacht`.
* **HeapB8** (DA2 heap overflow, survives DAA+SLA with BROM fused — sounds
  ideal) targets **V6 XML-protocol DAs**; MT8183 is **V5 XFlash**, and with DAA
  enforced you would need an Amazon-signed DA just to reach the bug.
* 4pda/Russian scene: nothing independent.

Firmware archive for diffing: Softpedia mirrors the historic chain for this
device (7.3.1.9 → 7.3.3.1); `update-kindle-*.bin` ZIPs contain `preloader.bin`,
`preloader_prod.img`, `lk.bin`, `tz.img`.

# amzn-pinnacles — Amazon "yacht" / KFYAWI

Unidentified Amazon Android slab. Not in Amazon's public Fire tablet model
table. Working theory: a non-retail SKU (enterprise / logistics / kiosk).

**Status (2026-08-20): device is on USB, ADB is already enabled, but
`unauthorized` — blocked on someone unlocking the screen and accepting the
RSA prompt.** See [Next step](#next-step).

Detail lives in siblings: **[identification.md](identification.md)** — public
-source research, the build-number decode, and what's been ruled out.

## Identity

Everything in this table came off the device screens in a prior chat-only
session (photographs), *not* from a shell. Treat as unverified until
`collect.sh` output confirms it.

| Field | Value |
| --- | --- |
| Build fingerprint | `Amazon/yacht/pinnacles 9/PS7401.3594N/0025535842816 user/amz-p,release-keys` |
| `ro.product.model` | `KFYAWI` |
| Serial / DSN | `G002G402413303RL` |
| ASIN (rear label) | `B0BPK28DW2` |
| Wi-Fi MAC | `EC:A1:38:3E:C6:7D` |
| Android | 9 (API 28) → Fire OS 7 |
| Security patch | 2022-01-01 |
| Kernel | `4.4.146+ #1 Tue Dec 5 20:17:12 UTC 2023` |
| Baseband | Unknown |

Confirmed independently in the prior session:

- OUI `EC:A1:38` → Amazon Technologies Inc., checked against the IEEE registry
  directly. Genuine Amazon hardware.
- `KFYAWI` is absent from Amazon's official build-model table, which otherwise
  lists every retail Fire tablet 2011 → 2024.
- Naming convention `KF` + first two letters of codename + `WI` (Wi-Fi) holds
  against KFTUWI→tungsten, KFKAWI→karnak, KFAUWI→austin, KFDOWI→douglas.
  So `KFYAWI` ⇒ codename **yacht**, board/platform **pinnacles**.
- `user` build + `release-keys` + `amz-p` ⇒ production-signed shipping
  firmware, not an engineering build.

## USB (confirmed this session, 2026-08-20)

First facts obtained from the hardware rather than a photo:

```
idVendor         0x1949  (6473)  Amazon
idProduct        0x0644  (1604)
USB Product      "Fire"
USB Serial       G002G402413303RL      <-- matches the DSN on the rear label
UsbDeviceSignature <4919 4406 2302 "G002G402413303RL" 00000006 0101 ff4201>
```

Two interfaces are published in that signature:

- `06 / 01 / 01` — Still Image class, i.e. **PTP/MTP**
- `ff / 42 / 01` — vendor-specific, the canonical **ADB** interface triple

That second one is the useful finding: **USB debugging is already enabled in
settings.** Somebody turned on developer options before this unit reached the
surplus channel. That is not the default state and it is consistent with the
"provisioned for a job, then dumped" theory.

`adb devices` sees it but reports `unauthorized`, so the daemon is running and
talking — it just has not been handed an authorized key yet.

## Interfaces

- **USB / ADB**: enabled, unauthorized. Amazon VID `0x1949`.
- **Serial**: not yet located. No UART pads identified; case not opened.
- **fastboot**: not yet reached. Prior session reports stock Android Recovery
  is available and exposes *reboot to bootloader*, which retail Fire tablets
  generally hide.
- **Telephony**: framework is live (lockscreen renders "No SIM card — No
  service"). Whether real modem hardware exists is unresolved; `Baseband:
  Unknown` argues against but is not decisive.

## Next step

The device is connected and ADB is enabled, so the whole playbook is one tap
away. On the device:

1. Unlock the screen.
2. Accept the **Allow USB debugging?** prompt.
3. Verify the fingerprint shown matches this laptop's key:

   ```
   C0:49:C1:BF:B6:16:FD:48:6F:FE:C7:B6:08:22:56:37
   ```

   (from `~/.android/adbkey.pub`; recompute with
   `awk '{print $1}' ~/.android/adbkey.pub | tr -d '\n' | md5`)

Then, from inside the devshell:

```bash
./devices/amzn-pinnacles/collect.sh
```

That writes the full playbook — getprop, package lists, `dumpsys
device_policy`, partition map, telephony, features — into `dumps/`.

If the screen lock turns out to be a credential nobody has, fall back to
fastboot, which bypasses the lockscreen entirely:

```bash
# from recovery: select "reboot to bootloader"
fastboot -i 0x1949 getvar all 2>&1 | tee dumps/fastboot-getvar.txt
```

## What the research settled (2026-08-20)

Full writeup in [identification.md](identification.md). The headline:

**`PS7401` decodes to Fire OS 7.4.0.1.** The build band is `PS` + the four Fire
OS version digits, now confirmed against FTVDB rather than inferred. Sweeping
every Amazon device family for a `PS74xx` build returns **exactly one hit** —
Echo Show 15 2nd Gen (`cypress`, model `AEOCY`, 7.4.6.6). No retail Fire tablet
and no Fire TV device uses the 7.4 branch.

So `yacht` has a Fire *tablet* model number (`KF__WI` convention) but runs a
software branch otherwise seen only on a wall-mounted Echo Show — and at a much
earlier build than the 2024 Echo Show. That tension is the best identification
lead available, and `pm list packages -s` tests it directly: Echo/Alexa shell
packages would corroborate, Fire tablet packages would refute.

Otherwise the device is genuinely undocumented — absent from the codename
wikis, from FTVDB, and from Amazon's GPL source pages; and ASIN `B0BPK28DW2`
has **no Keepa price history on .com or .co.uk**, meaning it was essentially
never offered at retail.

Also worth knowing: the XDA "4 GB RAM + rear flash" detail is weaker than the
handoff suggested. It traces to one garbled sentence in a two-post thread that
nobody answered. Search engines now echo it as if it were a spec sheet; that's
an LLM artifact, not a source. Measure it here.

## Open questions

- What is the marketing name / intended product for **yacht**? *(unresolved —
  no public source names it)*
- Does the package list look Echo/Alexa or Fire tablet? **← best next test**
- What is **pinnacles**? Unattested publicly; inferred to be an Amazon-internal
  board name (Amazon's tablet boards use California minerals/places, and
  Pinnacles is a California national park). The MediaTek guess from kernel
  4.4.146 is unverified — `/proc/cpuinfo` settles it.
- Is the bootloader locked? Is `flashing unlock` permitted?
- Real modem hardware, or telephony framework only?
- Was this provisioned through an enterprise/MDM path? (`dumpsys
  device_policy` answers this.)
- Actual RAM and whether a rear flash exists (`/proc/meminfo`,
  `pm list features`).

## Cautions

- **Do not factory reset before dumping state.** If this shipped through
  enterprise provisioning, a wipe may land on an enrollment screen with no
  available credentials.
- **Do not accept OTA updates.** Firmware is frozen at a Jan 2022 patch
  baseline; an update may close whatever access currently exists.
- The DSN is Amazon's registration/blacklist identifier — keep it out of
  public posts.

## Leads

All of the handoff's public-source leads have now been chased and came back
negative — see [identification.md](identification.md) for each, with sources
and the tooling notes needed to re-run them (most of these sites bot-wall
automated fetches).

Remaining, in rough order of value:

- **On-device package list.** The Echo-branch hypothesis above is testable and
  cheap. Highest-value single artifact.
- **Fire OS 7.4.x siblings.** If other 7.4-branch devices can be enumerated
  from firmware archives, `yacht`'s cohort becomes visible even though `yacht`
  itself is unlisted.
- **Amazon source tarballs by market name.** Tarballs are named for the market
  name, not the codename, so once the marketing name is known the GPL page
  becomes checkable again. Blocked on identification, not the reverse.
- **Teardown.** Chip markings would settle the SoC question independently of
  anything Amazon publishes. Case not yet opened.

## Log (newest first)

- **2026-08-20**: Public-source research pass — see
  [identification.md](identification.md). Decoded the build band (`PS7401` =
  Fire OS 7.4.0.1) and found 7.4.x is used by no retail Fire tablet or Fire TV,
  only Echo Show 15 2nd Gen. Ruled out: codename wikis, FTVDB, Amazon GPL
  source pages, Keepa/camel (ASIN never retailed), the XDA thread (two posts,
  no ID). Downgraded the "4 GB RAM + rear flash" claim to a single garbled
  forum sentence.
- **2026-08-20**: Created project. Added Android tooling to the RE flake
  (`android-tools`, `scrcpy`, `jadx`, `apktool`, `simg2img`,
  `payload-dumper-go`; `abootimg` skipped — Linux-only, use `binwalk` for
  boot.img on darwin). Confirmed the device on USB as `0x1949:0x0644` with
  serial matching the rear-label DSN, and read the USB descriptor to establish
  that **ADB is already enabled** (`ff/42/01` interface present). `adb devices`
  reports `unauthorized`. Wrote `collect.sh`. Blocked on the on-device RSA
  prompt.

# amzn-pinnacles — Amazon "yacht" / KFYAWI

Unidentified Amazon Android slab. Not in Amazon's public Fire tablet model
table. Working theory: a non-retail SKU (enterprise / logistics / kiosk).

**Status (2026-08-20): device is on USB, ADB is already enabled, but
`unauthorized` — blocked on someone unlocking the screen and accepting the
RSA prompt.** See [Next step](#next-step).

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

## Open questions

- What is the marketing name / intended product for **yacht**?
- What is **pinnacles** — SoC, reference board, or Amazon-internal platform
  name? Kernel 4.4.x hints MediaTek, unconfirmed.
- Is the bootloader locked? Is `flashing unlock` permitted?
- Real modem hardware, or telephony framework only?
- Was this provisioned through an enterprise/MDM path? (`dumpsys
  device_policy` answers this.)

## Cautions

- **Do not factory reset before dumping state.** If this shipped through
  enterprise provisioning, a wipe may land on an enrollment screen with no
  available credentials.
- **Do not accept OTA updates.** Firmware is frozen at a Jan 2022 patch
  baseline; an update may close whatever access currently exists.
- The DSN is Amazon's registration/blacklist identifier — keep it out of
  public posts.

## Leads not yet chased

- **Amazon GPL kernel source release.** Amazon must publish kernel sources per
  device, named by codename — look for a `yacht` tarball.
- **Keepa on ASIN `B0BPK28DW2`.** Keepa retains listing metadata after Amazon
  pulls a product page, which is the situation here. Plain web search cannot
  resolve a bare ASIN token.
- **`pinnacles` as a shared board name.** Worth grepping firmware archives and
  codename wikis for other devices on the same platform.
  `bitbyte.miraheze.org/wiki/Amazon_device_codenames` is relevant but blocks
  automated fetches — needs a human browser.
- **XDA thread** ["Plz help find what kindle this is"](https://xdaforums.com/t/plz-help-find-what-kindle-this-is.4787518/)
  (May 2026) posts a byte-identical fingerprint — a £5 UK car-boot unit. The
  poster claimed 4 GB RAM and a rear camera flash, which retail Fire tablets
  lack. Unverified hearsay about a *different* unit; confirm on this one. A
  rear flash would fit barcode/inventory scanning duty, which is the main
  support for the enterprise theory.

## Log (newest first)

- **2026-08-20**: Created project. Added Android tooling to the RE flake
  (`android-tools`, `scrcpy`, `jadx`, `apktool`, `simg2img`,
  `payload-dumper-go`; `abootimg` skipped — Linux-only, use `binwalk` for
  boot.img on darwin). Confirmed the device on USB as `0x1949:0x0644` with
  serial matching the rear-label DSN, and read the USB descriptor to establish
  that **ADB is already enabled** (`ff/42/01` interface present). `adb devices`
  reports `unauthorized`. Wrote `collect.sh`. Blocked on the on-device RSA
  prompt.

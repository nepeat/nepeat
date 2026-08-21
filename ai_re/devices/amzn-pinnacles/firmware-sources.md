# Obtainable firmware — diff targets

Exhaustive hunt for a `yacht`/`pinnacles`/`KFYAWI` image, and what to diff
against instead.

**No `yacht` image is obtainable** — not by guessing, not by manifest request,
not from any public index. **But a genuine Fire OS 7.4-branch image *is*
downloadable**, which corrects an earlier conclusion here.

## ✅ Confirmed downloadable (HTTP 200, verified by range read)

**1. `cypress` — Fire OS 7.4.6.6 — the best structural match**

```
https://d1s31zyz7dcc2d.cloudfront.net/2025/5/1/
  ff67a01a-0ae6-4b09-a0db-33acc60155ce/
  update-kindle-cypress-PS7466_user_5256_0032918243460.bin
```

1,229,888,044 bytes. Range read confirms:
`Amazon/cypress/cypress:9/PS7466.5256N/0032918243328:user/amz-p,release-keys`,
`post-sdk-level=28`, `ota-type=BLOCK`.

This is a real **7.4-branch, Android 9, `amz-p` release-keys** image — the same
firmware train as our `PS7401.3594N`. **No Fire tablet is on 7.4 at all** (all 22
on FTVDB are 7.3.x), so this Echo Show 15 gen-2 build is the only public 7.4
artifact in existence. It is the right target for diffing the **7.4-specific
framework** (`fosframework`, `fosservices`) rather than 7.3.

Caveat carried forward: cypress is **u-boot, not MTK LK**, so it is useless for
bootloader comparison — framework only.

**2. `trona` time series — 11 builds, 7.3.1.9 (2021-04) → 7.3.3.1 (2025-06)**

All direct-downloadable with MD5s. We already hold `PS7326` and `PS7331`;
additionally available: `PS7319`, `PS7321`, `PS7322`, `PS7323`, `PS7324`,
`PS7327`, `PS7328`, `PS7329`. Useful for an over-time diff of Amazon's changes on
the closest **MT8183** relative — including its LK.

**3. Public vanity redirects need no auth**

`GET https://www.amazon.com/update_Fire_HD10_11th_Gen` → 302 → S3 `.bin`.
About 21 device slugs exist. There is **no slug for this device** —
`update_{yacht,pinnacles,KFYAWI}` all 404.

## The real OTA endpoint — found, and gated

`POST https://softwareupdates.amazon.com/software/inventory` (and
`/software/inventory2`), an Amazon Coral RPC service. Distinguished from fakes by
probing: those two paths return `403 AccessDeniedException` while every other
`/software/*` path returns `UnknownOperationException`.

**A manifest cannot be fetched with public identifiers.** Unauthenticated POSTs
are rejected at the identity gate regardless of body, device UA, or fabricated
`x-adp-token`/`x-adp-signature` headers — it never reaches deviceType validation.
It needs real FIRS-registration device keys. Independent corroboration: the
best-maintained public Fire OS tracker (`fireos-archive`) **never touches the OTA
service** — it works entirely off public redirects, because it cannot
authenticate either. No repo anywhere implements a working manifest client.

`device-firmware.amazon.com` and `otav3.amazon.com` are **NXDOMAIN** — those
names, which circulate in older writeups, are simply wrong.

## Two corrections to earlier notes

**1. Token directories are per-(device, build), NOT shared.** Earlier notes
recorded a working premise that "a single token directory serves multiple devices
of the same build", which suggested filename substitution might work. **Disproved:**
in the anchor directory, `Fire_HD10` returns 200 but `Fire_HD10_Plus` of the *same
build* returns 403. Confirmed again on `fireos-tablet-src`, where HD10 and
HD10_Plus of `PS7331` sit in near-identical tokens differing only in the final
character (`…npabcf` vs `…npabce`) — adjacent, but distinct.

**2. Brute-forcing is impossible, and not for the obvious reason.** A garbage-key
control request also returns **403, not `NoSuchKey`** — S3 masks 404 as 403 on
these buckets, so filename guessing yields **zero signal**. There is nothing to
hill-climb on.

**3. `identification.md` said no PS74xx artifact exists publicly.** That is now
outdated — the cypress OTA above is a downloadable PS74xx image. The narrower
claim it was based on (no Fire *tablet* is on 7.4) still holds.

## Why no OTA host exists on our image — confirmed harder

The endpoint hunt was rerun properly: all 90 APKs and 50 JARs **decompressed**
(412 MB — the previous pass grepped them as compressed ZIPs and saw almost
nothing), plus the recovery ramdisk unpacked from `recovery.img`.

**Zero hits** for `softwareupdates`, `otav3`, `edgesuite`, `cloudfront`,
`fireos-tablet`, `update-kindle`, `amzdigitaldownloads` across `.oat`/`.vdex`/
`.art`, native libs, decompressed DEX, `/vendor`, `/etc`, and all four
bootloader/recovery images.

- `lk.img` and `preloader_boot0.img` contain **no URLs at all**.
- **`sbin/recovery` is sideload-only** — stock AOSP plus Amazon's `QUIESCENT-OTA`
  screen-state code, using `/sideload/package.zip` and `/tmp/update-binary`. **No
  network stack, no server strings.**
- The only OTA artifacts in `fosservices.vdex` are **consumer-side hooks**
  (`OTA_GROUP_NAME`, `SILENT_OTA_APPLIED`,
  `com.amazon.intent.action.OTA_INSTALLATION_COMPLETED`) — broadcast receivers,
  not a request builder.

So the "no OTA client" finding survives a much harder search. This is a stripped
kiosk/enterprise build with the entire OTA and account stack removed at build
time; the MAP/DCP identity library survives only because it is bundled inside the
timezone app.

## Amazon hosts present on the image (none OTA)

| Host | Source |
| --- | --- |
| `firs-ta-g7g`, `dcape-na`, `api.amazon.com`, `{na,eu,apac}.account.amazon.com`, `development`, `pre-prod` | `AutomaticTimeZone` APK `classes.dex` |
| `arcus-uswest.amazon.com` | `AWSRemoteConfigurationAndroidClient.vdex`, AmazonWebView |
| `kindle-time.amazon.com`, `ant.amazon.com`, `spectrum.s3.amazonaws.com` | `fosservices.vdex` |
| `alexa-comms.device-metrics-us-2.amazon.com` | `libAlexaCommsLib.so` |
| `alexa.na.gateway.devices.a2z.com` | `libAVSGatewayManager.so` |
| `global.{cmcs,safebrowsing}.service.amazonsilk.com` | `libwebviewextchromium.so` |
| `schemas.amazon.com` | `boot-fosframework.vdex` |

Note `ant.amazon.com` in `fosservices.vdex` — consistent with the RAFT Kerberos
realm `ANT.AMAZON.COM` in [network-behavior.md](network-behavior.md).

## Why `pinnacles` can never appear on FTVDB

FTVDB is populated by a crowd-sourced submit-URL API — i.e. from OTA URLs
captured off real devices. Our device **has no OTA client to capture from**, so
`com.amazon.pinnacles.android.os` can never be indexed there. That 404 is
structural, not a gap in coverage.

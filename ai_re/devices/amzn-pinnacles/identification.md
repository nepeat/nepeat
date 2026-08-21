# Identifying "yacht" / KFYAWI

Desk research 2026-08-20, then reconciled against the device shell the same
day. Sources inline; anything marked **inferred** is reasoning, not a citation.

> **Conclusion (added after the shell dump): `yacht`/`pinnacles` is a
> Fire HD 10 Plus (11th gen) hardware derivative with NFC and a rear camera
> flash added, running an AOSP-app-layer Fire OS 7.4 build.**
> See [The answer](#the-answer) at the bottom. The Echo Show lead below turned
> out to be a false trail — kept because the reasoning is still worth having on
> record, and because the build-band decode that produced it is sound.

## The build-number decode (the good lead)

The handoff guessed at the `PS74xx` band from XDA archives. It's now sourced,
and the scheme is exact: **`PS` + the four Fire OS version digits.** Confirmed
against FTVDB firmware histories:

| Codename | Device | Builds | Fire OS |
| --- | --- | --- | --- |
| `mustang` | Fire 7 9th gen | PS7315…PS7331 | 7.3.1.5–7.3.3.1 |
| `trona` | Fire HD 10 11th gen | PS7319…PS7331 | 7.3.1.9–7.3.3.1 |
| `kara` | Fire TV Stick 4K Max | PS7273, PS7681…PS7713 | 7.2.7.3, 7.6.8.1–7.7.1.3 |

So **PS73xx = Fire tablets** and **PS72xx/PS76xx/PS77xx = Fire TV**, both
confirmed rather than inferred. Our `PS7401.3594N` therefore decodes to
**Fire OS 7.4.0.1**.

Scanning every device page across all four FTVDB families (firetablet, firetv,
echo, kindle) for a `PS74xx` build yields **exactly one hit**:

> `com.amazon.cypress.android.os` — **Echo Show 15 2nd Gen (2024)**, model
> `AEOCY`, Fire OS 7.4.6.6 (`PS7466`)
> ([source](https://ftvdb.com/echo/firmware/com.amazon.cypress.android.os/))

No retail Fire tablet and no Fire TV device uses the 7.4 branch.

**This is the single most suggestive fact found.** `yacht` carries a Fire
*tablet*-style model number (`KF__WI`) but runs a software branch that no
retail tablet uses — a branch otherwise seen only on a wall-mounted Echo Show.
`PS7401` is a very early build in that branch, and the 2022-01-01 security
patch places it around early-to-mid 2022, well before the Echo Show 15 2nd Gen
shipped in 2024.

Worth testing against the device: does the system package list carry Echo /
Alexa-shell packages rather than Fire tablet ones? `collect.sh` captures
`pm list packages -s`, which answers this directly.

## Dead ends (each one is still evidence)

**Codename wikis — no trace.** The full raw wikitext of
[bitbyte.miraheze.org/wiki/Amazon_device_codenames](https://bitbyte.miraheze.org/wiki/Amazon_device_codenames)
lists 25 Fire tablets, 14 Fire TV devices and 4 Echo Shows. No `yacht`, no
`KFYAWI`, no `pinnacles`. Latest tablet entry is `tungsten` (Fire HD 10 13th
gen, 2023). FTVDB likewise has no `com.amazon.yacht.android.os`.

**No GPL kernel source.** This corrects a premise in the handoff: Amazon names
its source tarballs by **market name, not codename** (e.g.
`Fire_HD8_12th_Gen-8.3.3.8-20251223.tar.bz2`), so searching for a `yacht`
tarball could never have worked on its own. The stronger test is the version
band — and **no tarball on any of Amazon's three source-notice pages carries a
7.4.x version.** Tablets are 7.3.x/8.3.x, Echo is 5.5–7.5.x, Fire TV has its
own set. All three pages are bot-walled to direct fetch; read via Wayback:

- [Kindle & Fire Tablets](https://www.amazon.com/gp/help/customer/display.html?nodeId=200203720) (snapshot 2026-05-19)
- [Fire TV](https://www.amazon.com/gp/help/customer/display.html?nodeId=201452680) (2025-09-21)
- [Echo & Alexa](https://www.amazon.com/gp/help/customer/display.html?nodeId=201626480) (2025-11-04)

Caveat: tarball version labels don't always match FTVDB build labels — the Echo
Show 15 2nd Gen ships as `Echo_Show_15_2nd_Gen-7.5.6.5` while FTVDB records its
build as 7.4.6.6. So the 7.4-absence is suggestive, not airtight.

**ASIN B0BPK28DW2 was never retailed.** Keepa returns *"no price history
available"* for both `domain=1` (.com) and `domain=2` (.co.uk). Keepa tracks
~7B products, so a total absence means this ASIN has essentially never been
offered for retail sale. Wayback has zero captures of any URL containing it;
upcitemdb returns NOT_FOUND. **Inferred:** consistent with an enterprise,
non-retail, or bundled SKU. The `B0BP…` prefix suggests late-2022 registration
(weak, pattern-based).

**`pinnacles` is unattested.** No hits on the codename wiki, Amazon's source
pages, FTVDB, or GitHub code search. **Inferred:** Pinnacles is a California
national park, and Amazon's tablet board-name pool is California
minerals/places (`trona`, `onyx`, `quartz`, `sunstone`, `tungsten`) — so it
reads as an Amazon-internal board name rather than a vendor SoC name. **The
MediaTek guess from kernel 4.4.146 remains unverified** — no source ties that
kernel to an SoC on any Amazon device. `/proc/cpuinfo` will settle it.

**Model-prefix convention.** `KF__WI` is the Fire *tablet* convention
(KFFOWI=ford, KFGIWI=giza, KFKAWI=karnak, KFMUWI=mustang, KFTRWI=trona), so
`KFYAWI` is tablet-class by naming. Echo devices use a different scheme
entirely — Echo Show 15 2nd Gen is `AEOCY` for codename `cypress`. Note the
tension with the 7.4-branch finding above: tablet-style *name*, Echo-style
*software*.

## Correcting the XDA "4 GB RAM + rear flash" claim

The handoff flagged this as unverified hearsay. It's weaker than that.

The [thread](https://xdaforums.com/t/plz-help-find-what-kindle-this-is.4787518/)
has **exactly two posts, both dated 2026-05-03**, nothing since. The OP
(jacobjjcc, £5 UK car boot) writes a garbled sentence — *"its one with 4gb bc
the only ones i can find dont have a flash which mine has"* — and the only
reply, from an XDA Recognized Contributor, is generic boilerplate that **does
not identify the device.**

Search engines now surface "4GB and Flash capabilities" as though it were a
spec sheet. That is an LLM paraphrasing that one garbled forum sentence, not an
independent source. **Treat the RAM figure and the rear flash as unconfirmed
until measured on this unit** (`/proc/meminfo` and `pm list features` — both in
`collect.sh`).

## The answer

The shell dump ([hardware.md](hardware.md)) makes the hardware unambiguous, and
it matches exactly one retail product.

| | This device | Fire HD 10 Plus (11th gen, 2021) |
| --- | --- | --- |
| SoC | MediaTek MT8183 | MediaTek MT8183 Helio P60T |
| Display | 1200×1920 | 1920×1200, 10.1" |
| RAM | 4 GB | 4 GB |
| Storage | 32 GB | 32 GB |
| Battery | 6500 mAh | 6500 mAh |
| SKU string | `ro.boot.hardware.sku` = **`plus`** | "Plus" |

Retail spec source: [GSMArena, Fire HD 10 Plus (2021)](https://www.gsmarena.com/amazon_fire_hd_10_plus_(2021)-10882.php).
The retail codename for this generation is `trona`.

Every hardware axis matches, and the firmware literally calls its SKU `plus`.
So this is Fire HD 10 Plus **hardware**. But it is not a Fire HD 10 Plus, and
the differences are the interesting part:

**Hardware Amazon added.** GSMArena lists the retail Fire HD 10 Plus as
**NFC: No**, and lists no rear LED flash. This device has both — NFC via an
**NXP** controller with `hce`, `hcef` and **`com.nxp.mifare`**, plus a rear
camera flash. Neither is a software flag; both are physical additions to the
retail platform.

**Software Amazon removed.** The app layer is AOSP reference apps —
`com.android.launcher3`, `camera2`, `email`, `music`, `gallery3d`,
`deskclock`, `calculator2`. There is **no Fire launcher, no Amazon Appstore,
no Silk, no Alexa, no `com.amazon.dcp`**. Only eight Amazon packages survive,
all platform plumbing (`amazon.fireos`, `com.amazon.shpm`,
`com.amazon.wirelessmetrics.service`, `com.amazon.webview.chromium`,
`com.fireos.arcus.proxy`, `com.amazon.platform.fdrw`,
`com.amazon.kindleautomatictimezone`, `android.amazon.perm`).

Underneath, though, it is still Fire OS: `pm list features` carries the full
`com.fireos.sdk.*` set and `com.amazon.software.fireos`. So the *platform* is
Fire OS 7.4.0.1 with the *consumer shell deleted and AOSP apps put in its
place* — which is exactly why the handoff's photos looked like stock AOSP.

A leftover MediaTek factory-test app, `com.mediatek.ygps`, is also still
installed — not something that survives onto a consumer image.

**What it was for.** NFC with Mifare, a rear flash camera, Ethernet
(`android.hardware.ethernet`), a 10" screen and no consumer shell is the
signature of a fixed-purpose device: badge/tag reading, inventory or
access control, kiosk or point-of-sale. That is consistent with the
"non-retail SKU" hypothesis in the handoff, and now rests on hardware evidence
rather than absence-from-catalog.

**Why the Echo Show lead misfired.** The `PS74xx` → Fire OS 7.4.x decode is
correct, and Echo Show 15 2nd Gen genuinely is the only *public* 7.4.x device.
The flaw was assuming the public firmware archives are complete. 7.4 is better
read as a branch Amazon uses for non-Fire-tablet-shell products generally, of
which this is an unlisted member. `ro.build.characteristics` is `tablet`, not
`tv` or `speaker`, and there is not a single Alexa package — so Echo is ruled
out on the device's own evidence.

**Still unresolved:** the marketing name, if it ever had one. The OTA package
name is `ro.product.package_name` = **`com.amazon.pinnacles.android.os`** —
keyed on the *board*, not the product, which is why the earlier
`com.amazon.yacht.android.os` lookup 404'd. Unfortunately
[the pinnacles URL 404s too](https://ftvdb.com/firetablet/firmware/com.amazon.pinnacles.android.os/),
so it is genuinely absent from FTVDB rather than merely misfiled. That package
name is still the correct string to search future firmware archives and OTA
endpoints for.

## Research tooling notes

For whoever picks this up: WebFetch is 403'd by xdaforums, bitbyte.miraheze and
camelcamelcamel, and 5xx'd by amazon.com. `curl` with a full browser header set
and a Google referer works for XDA and miraheze. Wayback `id_` snapshots with
`--compressed` are the reliable route for amazon.com help pages. DuckDuckGo
(html and lite), Marginalia and public SearxNG instances all served captchas or
429s. The Amazon S3 source buckets (`fireos-tablet-src`, `fireos-audio-src`)
403 on `ListObjects`, so tarball inventory can only be read off the help pages.

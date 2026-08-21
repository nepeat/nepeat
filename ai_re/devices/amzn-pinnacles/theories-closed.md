# Theories evaluated and closed

Running list of routes to flashing a custom ROM that have been **tested and
ruled out**, so effort is not spent twice. Live leads are in
[da-validation.md](da-validation.md) and [brom-recovery.md](brom-recovery.md).

## Cryptographic verification — all sound, no logic flaws

| Theory | Verdict |
| --- | --- |
| Forge an unlock cert | **Closed.** RSA-2048 device-bound signature; Amazon's private key required. |
| Flaw in LK's unlock check | **Closed.** LK only *reads* a boot-arg byte; it does not decide. |
| Flaw in the preloader's unlock check | **Closed.** Reads IDME at fixed offsets, composes a 26-byte message, one RSA verify, `clz(r0)>>5` exact zero-test. Memoised via a `-255` sentinel in preloader RAM. Tight at every step. |
| Flaw in `AMZN_PL_VERIFY` (image auth) | **Closed.** Textbook RSA-PSS: null-pubk check, `siglen > 0xff`, RSA public decrypt, HW SHA-256, PSS decode, result must equal 1. *(It does repeat the sloppy "minimum not exact" length check seen in `flash unlock`, but RSA-2048 then requires exactly 256 bytes.)* |
| Engineering vs production key confusion | **Closed as an entry point.** The preloader carries both (`Succeed to verify %s with eng key`, `Only try verify %s with prod key on locked production device`), but the verification routine itself is sound and key *selection* happens in the caller. Choosing the eng key would still need an eng-signed image — also Amazon's. |
| Recovery sideload with our own package | **Closed.** `/system/etc/security/otacerts.zip` holds one cert: Amazon, self-signed, **RSA-2048/SHA-256**, `CN=Amazon`, `emailAddress=kindle-cs-support@amazon.com`, valid 2022-01-18 → 2049. Sideload requires that key. *(Incidental: the zip entry path is `dev/shm/platform/0/releasekey.x509.pem` — a leaked build-system path, not exploitable.)* |

## Memory-safety / parser attacks

| Theory | Verdict |
| --- | --- |
| `flash tucert` buffer overflow | **Closed.** Bounded exactly at the declared 1024 bytes; 1025 → `write tucert failed!`, nothing written. |
| LibTomCrypt DER parser → code execution | **Closed.** Version pinned to 1.18.2. Real unbounded-recursion bug (298 levels in 1021 B, ~12–17 KB stack) and a novel 32-bit `_fetch_length` integer overflow giving an OOB *read* — but **every sub-decoder bounds its writes**. 900k guard-page fuzz iterations found only CVE-2019-17362, a useless 1-byte OOB read. Crash yes, code execution no. |
| Oversized `flash unlock` | **Closed.** >1024 B returns `OKAY` but writes nothing (verified with printable bytes). A real logic bug — a success path that skips both verification and the write — but not usable. |

## Access paths

| Theory | Verdict |
| --- | --- |
| `ro.oem_unlock_supported=1` means something | **Closed twice over.** Fastboot refuses everything with *"restricted on locked hw"*, **and** there is no FRP / `persistent_data_block` / `seccfg` partition, so AOSP's `OemLockManager` has no backing store. Inert at both ends. |
| Other fastboot commands | **Closed.** Allowlist is exhaustively `getvar product/serialno/max-download-size`, `flash:unlock`, `flash:tucert`. `fastboot boot` is refused even with the device's own `boot.img`, and the allowlist is checked *before* partition lookup. |
| `boot_para` selects download mode | **Closed.** Magics are `METAMETA`, `FACTFACT`, `ADVEMETA`, `FACTORYM`, `FASTBOOT`, `METAFORB` (plus byte-reversed pairs). **No USBDL magic exists.** |
| `adb reboot edl` → download mode | **Closed.** Performs an ordinary reboot; device returns as `0x1949:0x0642`, not `0e8d:0003`/`0e8d:2000`. |
| Tardis Key USB system update | **Closed.** `tardiskey/SystemUpdateService` routes through `OTAControllerFactory`, and `com.amazon.device.software.ota` is not installed. Would be a signed sideload regardless. |
| An app-layer route | **Closed.** All 87 APKs audited. Only three reference IDME/flags (`Shipmode`, `kor.demo`, stock `Settings`). No Amazon OOBE exists. |
| Fetch a `yacht` OTA to diff | **Closed for `yacht`.** The OTA endpoint (`softwareupdates.amazon.com/software/inventory`) is identity-gated; S3 masks 404 as 403 so filename brute-forcing has zero signal; token dirs are per-(device,build). A `cypress` 7.4-branch image *is* downloadable as a framework diff target — see [firmware-sources.md](firmware-sources.md). |

## dm-verity

Runtime state confirmed: `dm="system … android-verity"`,
`androidboot.veritymode=eio`,
`veritykeyid=id:f3530e18f64d11fc25eb2dd762979f078de990bf`, and debugfs
`android_verity/{target_added,verity_enabled}` both `Y`.

This is **AVB 1.0** style — the `android-verity` dm target reads verity metadata
from the partition and checks its signature against a key in the kernel keyring.
Writing `/dev/block/mmcblk0p19` directly with root is possible, but the metadata
is signed, so a modified `/system` fails verification. AOSP's `android-verity`
skips that check only when the device is unlocked, and unlock state arrives via
bootargs from LK. **So verity defeat requires either an unlock or kernel code
execution** — it is not independently bypassable.

Noted for the live lead: the kernel exports **`masp_hal_set_dm_verity_error`**.

## Kernel module build — blocked, but not closed

The plan (insmod an unsigned module to read eFuse `0x11f10060`) hit a toolchain
wall rather than a conceptual one:

- The only public MT8183 Amazon tree
  (`amazon-mt8183-devs/android_kernel_amazon_mt8183`) is **4.4.302** on all three
  branches, including `trona-maverick-unify`. Our device is **4.4.146+**.
- Our kernel was built with **clang 6.0.2** (per `/proc/version`); nixpkgs offers
  clang 21 and GCC 15, neither of which will build a 4.4 tree.
- `CONFIG_MODVERSIONS=y` with `CONFIG_MODULE_FORCE_LOAD` **not** set means symbol
  CRCs must genuinely match — `insmod --force` is unavailable.

**The way through is CRC patching, not tree matching:** extract the real
`__kcrctab`/`__ksymtab` from our own `boot.img` kernel, build a trivial module
however it compiles, then patch its `.modinfo` vermagic to `4.4.146+` and its
`__versions` entries to the device's actual CRCs. For a module that only touches
`printk` and `ioremap`, ABI drift between 4.4.146 and 4.4.302 is low risk. This
remains the most promising way to answer the SBC question without UART.

## USBDL memory patch of the `daa_enabled` gate — CLOSED 2026-08-21 (tested live)

Plan was: reach preloader USBDL, `WRITE16` at runtime `0x0022D8B8` to make the
`daa_enabled` query return 0, so `usbdl_verify_da` takes the "DA validation
disabled on non-secure chip" path and accepts an unsigned DA.

**Tested against the device. Denied.**

```
READ32  0x0022D8B8              -> status 0x1000   (address refused)
WRITE16 0x0022D8B8 = 2000,4770  -> status 0x1001   (write refused)
```

The preloader's USBDL memory commands are restricted to an address allowlist —
measured as the eFuse window `0x11f10000`–`0x11f10100` plus the WDT
`0x10007000`, 5 of 15 probed addresses. SRAM, DRAM, and all preloader code are
outside it. No signature or SLA is involved; the address filter alone closes it.

This also **retracts** the earlier claim of a root-free arbitrary memory write
in the preloader — that came from misreading mtkclient's host-side DA-patching
log lines. See [preloader-usbdl.md](preloader-usbdl.md).

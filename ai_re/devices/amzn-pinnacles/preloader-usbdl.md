# Preloader USBDL — reachable, but memory access is address-filtered

> **⛔ CORRECTION 2026-08-21 — this file previously claimed a "CONFIRMED
> root-free arbitrary memory write in the preloader". That claim was WRONG and
> has been retracted. It was based on misreading mtkclient log lines. Tested
> live on the device: the preloader's USBDL memory commands are restricted to a
> small address allowlist, and the patch target is not in it. `WRITE16` to
> `0x0022D8B8` returns status `0x1001` (denied).**

## What was actually demonstrated (live, 2026-08-21)

**Good news, and it is real:**

1. **Preloader USBDL is reachable purely from software, no buttons.** Start a
   poller, then `adb reboot`. The device enumerates as `0e8d:2000`
   "MT65xx Preloader".
2. **The handshake succeeds on a locked, unrooted device**, and reports
   `HW code: 0x788` — our MT8183.
3. **`READ32` works** on allowed addresses, with no SLA and no DA.
4. **`efuse 0x11f10060 = 0x00000946` read live off the device**, independently
   confirming the value previously derived from `atag,devinfo`. Two independent
   methods, same answer.
5. **Command mode persists** after a completed handshake. The port can be
   reopened and reused without re-handshaking, provided the byte stream is
   resynced first (see below).

**Bad news, and it is decisive:** memory access is **address-filtered**.

## The address allowlist (measured)

Probed read-only, one `READ32` per address:

| address | region | result |
| --- | --- | --- |
| `0x11f10000` | eFuse base | **ALLOW** `0x80000001` |
| `0x11f10020` | eFuse +0x20 | **ALLOW** `0x00040000` |
| `0x11f10060` | eFuse +0x60 (sbc/daa) | **ALLOW** `0x00000946` |
| `0x11f10100` | eFuse +0x100 | **ALLOW** `0x00000000` |
| `0x10007000` | WDT | **ALLOW** `0x00000030` |
| `0x11f11000` | eFuse +0x1000 | DENY `0x1000` |
| `0x08000000` | chipid | DENY `0x1000` |
| `0x11002000` | UART | DENY `0x1000` |
| `0x00100A00` | BROM payload addr | DENY `0x1000` |
| `0x00201000` | DA payload addr | DENY `0x1000` |
| `0x00200D00` | preloader base | DENY `0x1000` |
| `0x0022D8B8` | **the daa stub — our target** | **DENY `0x1000`** |
| `0x40000000` | DRAM base | DENY `0x1000` |
| `0x10212000` | CQ_DMA | DENY `0x1000` |
| `0x00000000` | null | DENY `0x1000` |

**5 of 15 allowed**, and the allowed set is just the eFuse controller window
plus the watchdog. Status `0x1000` is what mtkclient's error table calls
`CRYPTO_INIT_FAIL`; here it plainly means *address refused*.

Writes are refused separately and with a distinct code:

```
WRITE16 0x0022D8B8 = 0x2000,0x4770   ->  write status 0x1001
```

So the `daa_enabled` patch cannot be applied over USBDL. **That plan is dead.**

⚠️ **Do not attempt writes anywhere in `0x11f1xxxx`.** It is the eFuse
controller window, it *is* in the allowlist, and a stray write there could
program fuses irreversibly. Reads only.

## The address filter is sound — no count-overflow bug (tested 2026-08-21)

Address filters commonly validate the *start* address but not `addr + count*4`.
This one does not have that bug. Probed read-only from the allowed base
`0x11f10060`:

| count | last address covered | result |
| --- | --- | --- |
| 256 | `0x11f1045c` | OK |
| 512 | `0x11f1085c` | OK |
| 1004 | `0x11f11000` | **DENY `0x1000`** |
| 2048 | `0x11f12060` | **DENY `0x1000`** |

`0x11f11000` is exactly where the single-address probe also flipped to DENY, so
the permitted window is the **4 KB eFuse page `0x11f10000`–`0x11f10fff`**, and
the check covers the whole requested range. A large count cannot be used to walk
out of the window.

Even if it could, it would not reach the target: reads and writes run *upward*
from the base, and every allowed base (`0x11f1xxxx`, `0x10007000`) sits far
**above** the preloader at `0x00200D00`. Reaching `0x0022D8B8` by wraparound
would need ~1e9 words pushed over a 115200-baud serial link — roughly 4 GB of
traffic, which is not a practical primitive even if the arithmetic allowed it.

**Conclusion: USBDL memory access cannot be levered into a preloader patch.**

## How the wrong claim happened

The earlier run produced these lines, which I read as writes into the device:

```
XFlashExt  - Patching da1 ...
Mtk        - Patched "hash_check" in preloader
Mtk        - Patched "get_vfy_policy" in preloader
```

They are **host-side**. In `mtkclient/Library/mtk_class.py`:

```python
def patch_preloader_security_da1(self, data):
    data = bytearray(data)
    ...
    self.info(f'Patched "{patchval[2]}" in preloader')
```

It takes a `data` bytearray — the **Download Agent blob being prepared for
upload** — and patches it in the host's memory. The message says "in preloader"
because the byte patterns originate from preloader-security work, but nothing is
sent to the device. The device was never written to.

**Lesson for these notes: a tool's log line is not an observation of the
device.** The claim should never have been recorded as confirmed without a
read-back proving the device's memory had changed.

## Opcode correction

Earlier notes listed the USBDL dispatcher as `0xa1 WRITE16 / 0xa2 READ32`, and
`0xd0 SEND_DA / 0xd5 JUMP_DA`. **Wrong.** From `mtk_preloader.py`:

```
CMD_READ16_A2 = 0xA2      # legacy 16-bit read
READ16        = 0xD0
READ32        = 0xD1
WRITE16       = 0xD2
WRITE16_NO_ECHO = 0xD3
WRITE32       = 0xD4
```

This was caught empirically: `0xA2` against `0x11f10060` returned **two** bytes
`0946`, not four — because it is a 16-bit read.

## Protocol notes (for whoever picks this up)

**The serial path works on macOS; use `serialportname=`.** The preloader appears
as `/dev/cu.usbmodem1101` and mtkclient's serial path handshakes fine.

> **⚠️ Correction 2026-08-21 (same day).** This section originally asserted that
> *"the AppleUSBCDC kernel driver claims the interface, so libusb/pyusb never
> enumerate `0e8d:2000`."* **That explanation was wrong.** The scan that
> "returned nothing" was actually raising
> `usb.core.NoBackendError: No backend available` — the venv simply had no
> libusb shared library — and my probe caught the exception and reported it as
> "not present".
>
> Proven wrong later the same day: with an explicit backend
> (`usb.backend.libusb1.get_backend(find_library=lambda _: "…/libusb-1.0.dylib")`)
> libusb enumerates an Amazon USB device on this same Mac and
> `usb.util.claim_interface()` **succeeds**. So macOS was never the obstacle.
>
> Whether the USB path *specifically* reaches the preloader is now untested. It
> changes nothing about the outcome — the write was refused by the preloader's
> **address filter**, which is transport-independent — but the stated reason was
> not a measurement, and is retracted as such.

**Timing.** Measured across three reboots: the preloader VCOM appears **~4 s
after `adb reboot`** and is present for only **~2–3 s**. mtkclient's
`Port.handshake()` sleeps 300 ms per attempt, which is too coarse to land
reliably; poll for the tty with no sleep instead.

**`preloader.init()` crashes on this device** in `get_blver()` with
`unpack requires a buffer of 1 bytes`. Nothing past the hwcode is needed for
read/write, so stop there:

```python
mtk.port.serial_handshake(maxtries=200)
pl.echo(pl.Cmd.GET_HW_CODE.value)
val = pl.rdword()            # -> 0x07880000
cfg.init_hwcode((val >> 16) & 0xFFFF)
```

**Read framing**, after the three echoes (cmd 1B, addr 4B, count 4B):

```
status(2) + data(4 * count) + status2(2)
```

e.g. `0x11f10060` returns `00000001 | 0000 | 00000946 | 0000`.

**Resync.** If a command is left half-issued the stream desyncs. Send 8 zero
bytes (finishing any pending addr+count harmlessly), drain, then verify with
`0xFD` — a correct `fd` echo followed by 4 bytes of hwcode means command mode is
live again.

## Recovery

The device stays wedged in preloader after a session ends. **Hold power ~10 s**
to get back to Android. Nothing software-side recovers it — mtkclient (USB and
serial), a raw handshake, raw USB bulk endpoints, and a USB bus reset have all
been tried and all fail, because the handshake window is consumed at
enumeration.

## What this leaves

The bootloader-level route is now closed at every layer that has been examined:
unlock cert forging, LK and preloader verification logic, the fastboot
allowlist, DA loading, BROM, the DER parser, and now **USBDL memory access**.
See [theories-closed.md](theories-closed.md).

What survives is the *soft* route in [da-validation.md](da-validation.md): root
plus an unsigned kernel module (`CONFIG_MODULE_SIG` is not set) to neuter
dm-verity at runtime. That is root-dependent, so it does not satisfy the
root-free goal — but note there is **no OTA client on this image**
([network-behavior.md](network-behavior.md)), so the "an update will patch your
exploit" risk does not actually apply here.

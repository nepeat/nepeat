# Recovery mode USB — a stub HID channel

Device entered **recovery** 2026-08-21. It enumerates as:

```
1949:064b  "Amazon" / "KFYAWI"  serial = <DSN>   bcdDevice 0x0223
  intf 0  class=0x03 (HID) sub=0x00 proto=0x00
    ep 0x81 IN   interrupt  max 4  interval 4
    ep 0x01 OUT  interrupt  max 4  interval 4
```

**Neither `adb` nor `fastboot` matches it.** fastboot needs a bulk interface
with `class=0xff sub=0x42 proto=0x03`; this is HID with 4-byte interrupt
endpoints, so the host tools ignore it entirely.

## The HID descriptor is a degenerate stub

```
05 01     Usage Page (Generic Desktop)
09 00     Usage (0x00, undefined)
a1 01     Collection (Application)
c0        End Collection
```

Seven bytes, an empty application collection with an undefined usage. It
declares no reports at all. This is the classic "fake HID" shape — a USB channel
that needs no host driver, with the real protocol (if any) carried as raw
4-byte interrupt transfers.

String descriptors are unhelpfully generic: `iInterface="HID Interface"`,
`iConfig="HID"`. No mode name, no vendor protocol hint.

## What was tested

| test | result |
| --- | --- |
| `fastboot devices` | empty |
| `adb devices` | empty |
| `usb.util.claim_interface(d, 0)` | **succeeds** — macOS does not hold it |
| read ep `0x81`, 4 bytes, 1.5 s × 3 | `USBTimeoutError` every time |

So the channel is **passive**: it sends nothing unprompted and waits to be
driven. Nothing was written to the OUT endpoint — the protocol is unknown, and
blind 4-byte writes into a recovery binary are not a safe guess.

## Assessment

Most likely this is simply the USB gadget recovery brings up when no adb
function is enabled, rather than a hidden service interface. The 4-byte
interrupt pair is too narrow for bulk data (no flashing or dumping through it)
and would only suit a small command/status protocol.

Worth noting it is **root-free** and sits **below Android**, which is the right
shape for the standing goal — but an unknown protocol on a 4-byte channel is a
poor blind-fuzzing target, and recovery is where a wrong command could trigger a
factory reset.

**Before spending effort here, check the recovery menu on-screen.** If
*"apply update from ADB"* is available, that switches USB to `adb sideload` and
is a far better-understood surface — though note package verification against
Amazon's RSA-2048 `otacerts` already closed unsigned sideload
(see [theories-closed.md](theories-closed.md)).

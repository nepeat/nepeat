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

---

# Recovery internals (static analysis of `recovery.img`, 2026-08-21)

Unpacked the dumped `recovery.img` (Android boot image v1, 2048-byte pages;
kernel 10004528 B, ramdisk 5414054 B) and read the init scripts and
`/sbin/recovery` (ELF 32-bit ARM, static, stripped, Android 28).

## Why USB shows up as HID

`init.recovery.mt8183.rc` builds the gadget in configfs and **defaults to HID**:

```
mkdir /config/usb_gadget/g1/functions/hid.gs0
mkdir /config/usb_gadget/g1/functions/ffs.adb      <-- adb function is created too
setprop sys.usb.config hid                          <-- but hid is the default

on property:sys.usb.config=hid && property:sys.usb.configfs=1
    write .../idProduct 0x064B
    symlink .../functions/hid.gs0 .../configs/b.1/f1
```

That matches what the device presents exactly (`1949:064b`, HID). The commented-out
lines are notable — the 4-byte report length and the exact 7-byte descriptor we
read off the wire are *literally in the script*, disabled:

```
#write /config/usb_gadget/g1/functions/hid.gs0/report_length 4
#write /config/usb_gadget/g1/functions/hid.gs0/report_desc \x05\x01\x09\x00\xa1\x01\xc0
```

So the HID function is a **placeholder to keep a USB gadget bound** when adb is
off. It is not a hidden service interface, and there is no protocol behind it.
That closes the idea of fuzzing those interrupt endpoints.

## The adb switch, and why it never fires

The gadget flips to adb on a trigger that is **not** `sys.usb.config=adb`:

```
on property:sys.usb.ffs.ready=1
    write .../UDC "none"
    rm .../configs/b.1/f1
    write .../idVendor 0x18d1
    write .../idProduct 0xd001
    symlink .../functions/ffs.adb .../configs/b.1/f1
    write .../UDC "musb-hdrc"
```

`sys.usb.ffs.ready=1` is set by **adbd itself** when it opens the functionfs
endpoints. So the sequence is *adbd starts → ffs.ready → gadget becomes adb*.
And adbd is gated shut in `init.rc`:

```
service adbd /sbin/adbd --root_seclabel=u:r:su:s0 --device_banner=recovery
    disabled

on property:ro.debuggable=1
    start adbd
```

`ro.debuggable=0` here (the recovery rc has an `on property:ro.debuggable=0`
block), so **adbd never starts** and the gadget stays HID.

Worth noting for anyone who finds a way to start it: `--root_seclabel=u:r:su:s0`
means recovery adbd would run **as root**. That is a designed capability, not a
kernel bug, so unlike the Mali chain it is not something an update "patches".
The obstacles are `ro.debuggable` (baked into the ramdisk, so changing it means
flashing) and `service.adb.root=1` (needs root/system to set).

## Sideload exists but is unreachable from the menu

`/sbin/recovery` still contains the whole minadbd sideload stack —
`bootable/recovery/minadbd/minadbd.cpp`, `sideload-host:`,
`/sideload/package.zip`, `Install from ADB complete (status: %d)`. The visible
menu, confirmed against the device, has no entry for it:

```
reboot system now / reboot to bootloader / wipe data/factory reset
wipe cache partition / view recovery logs / mount / power down
Factory data reset
```

Amazon removed the menu item, not the code. The arg parser is trimmed hard —
the only `--` strings left are `--adbd` and `--retry_count`.

**`adb reboot sideload` writes the BCB and needs no root**, so sideload mode is
reachable from a normal, unrooted Android boot. That makes the package verifier
the surface — and it does not give way:

## `res/keys` — exactly one key

```
v3 {64, 0x7e758ca5, {...}, {...}}     # 1400 bytes, ONE key block
```

A single **v3, 64-word (RSA-2048)** key. **No AOSP testkey, no second key, no
dev key left in.** So the classic "sign with the public testkey" route is out,
and sideload only accepts Amazon-signed packages. This re-confirms
[theories-closed.md](theories-closed.md) from the recovery side.

## Why none of this reaches the goal

Even a root shell in recovery would not produce a booting custom ROM. LK
verifies `boot`/`recovery` against Amazon's production key on a locked device
(`amzn_image_verify`, *"Only try verify %s with prod key on locked production
device"*). Writing a modified image is not the hard part — **passing LK's
verification at the next boot is**, and that still requires the unlock
certificate signed with Amazon's RSA-2048 private key.

So recovery is thoroughly characterised and adds no new route. The gate remains
exactly where [unlock.md](unlock.md) put it.

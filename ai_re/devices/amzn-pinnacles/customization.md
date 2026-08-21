# What's actually custom on this thing

Short version: **a factory reset only wipes `/data`.** Everything below lives in
`/system` and `/vendor`, which are mounted read-only, sealed with dm-verity
(`ro.boot.veritymode=eio`, `verifiedbootstate=green`), and signed with Amazon's
release keys. A reset doesn't remove any of it — it restores the device to
*exactly* this state. That's why a wiped unit still boots into a Kerberos
corporate lockscreen.

And this is not an app skin over stock Android. Amazon's code is in the boot
classpath, inside `system_server`, in the HAL layer, and in the SELinux policy
language itself.

## Amazon code runs in every process

```
BOOTCLASSPATH          ... framework.jar ... fosframework.jar  webviewext.jar
SYSTEMSERVERCLASSPATH  services.jar ... fosinit.jar  fosservices.jar
```

`fosframework.jar` sits on the **boot classpath**, so it is loaded into every
single app process next to `framework.jar`. `fosinit.jar` and `fosservices.jar`
are on the **system server classpath**, so Amazon code runs inside
`system_server` itself.

It's AOT-compiled into the boot image, not sideloaded:

```
13564300  boot-framework.oat        8884224  boot-framework.art
  692620  boot-fosframework.oat      761856  boot-fosframework.art
```

Also present in `/system/framework/`: `fosservices.jar`, `aspclient.jar`
(audio signal processor), `retaildemolibrary.jar`, `webviewext.jar`,
`AWSRemoteConfigurationAndroidClient.jar`, `android.amazon.perm`, and a
`fireos-res` resource package.

## ~10% of the system service surface is Amazon's

17 of the 175 registered binder services:

```
fireospowersupportservice     amazonpowermanagerservice
raft_kerberos                 amazonthermalservice
SmartSuspend                  amazonpackagemanager
amazonlockscreen              audiosignalprocessor
AmazonWifiService             amazonusermanagerservice
amazondropbox                 arcusservice
amazonactivitymanager         amazonaccessibilitymanager
amazonstoragemanagerservice   FireOsDisplayPowerController
fosdebug                      amazonfiled
```

Note the pattern: `amazonactivitymanager`, `amazonpackagemanager`,
`amazonusermanagerservice`, `amazonpowermanagerservice` — Amazon shadowed the
core AOSP managers with their own parallel services rather than patching the
originals. `amazonlockscreen` is the service side of the RAFT keyguard in
[raft-lockscreen.md](raft-lockscreen.md).

## A custom SELinux object class

This is the part that surprised me most. Amazon didn't just add SELinux *types* —
they added an object **class**, which means extending the policy language:

```
/sys/fs/selinux/class/amazon_policies/perms/
    access_clear_app_user_data      access_get_intent_sender_intent
    access_launcher_navigation      access_power_manager
    access_remove_tasks             access_surface_flinger
    access_system_window            allow_duplicate_permissions
    grant_amazon_permissions        grant_bind_settings_permission
    log_internal_metric             see_home_task
```

AOSP has no equivalent. You can watch it enforce in logcat:

```
avc: denied { see_home_task } for scontext=u:r:untrusted_app_25:s0:c512,c768
     tcontext=u:r:system_server:s0 tclass=amazon_policies permissive=0
```

## Custom HAL interfaces

Amazon defined their own HIDL namespace, `fireos.hardware.*`, parallel to
`android.hardware.*`:

| HAL | Purpose |
| --- | --- |
| `fireos.hardware.idme@1.0` | factory identity block — see below |
| `fireos.hardware.fireosdha@2.0` | Device Hardware **Attestation** — a HIDL signing HAL (`getDhaPublicKey`, `getDhaCertificateChain`, `sign`). *Not* a "health agent", as earlier notes guessed. No network. |
| `fireos.hardware.amazonthermal@1.0` | thermal management |
| `fireos.hardware.audio@2.0` | audio |
| `fireos.hardware.connectivity.networkpower@1.0` | network power |

Plus Amazon init scripts: `amazond.rc`, `amazon_crash_reporter.rc`,
`amazon_logd_old.rc` (their own logd), `fireos_logging.rc`, `fosflags.rc`,
`init.wipe_fos_flags.rc`.

## IDME — the factory identity block

`fireos.hardware.idme@1.0-service` is running and exposes `/proc/idme/`, a
**35-field** factory-programmed identity area. This is Amazon-specific; there is
no AOSP analogue. Full capture in [`dumps/idme.txt`](dumps/idme.txt).

```
DKB            KB             alscal         board_id      bootcount
bootmode       bt_mac_addr    bt_mfg         co_tms_cal    dev_flags
device_type_id fos_flags      front_cam_otp  iaicr         mac_addr
mac_sec        manufacturing  miccal.0       miccal.1      postmode
product_name   productid      productid2     rear_cam_otp  region
sensorcal      serial         t_unlock_cert  t_unlock_code tp_cg_color
unlock_code    unlock_version usr_flags      wifi_mfg      wpc_cal
```

Note `wpc_cal` — wireless power charging calibration. The Fire HD 10 **Plus**
is precisely the variant with Qi charging, which corroborates the
identification in [identification.md](identification.md) and the
`ro.boot.hardware.sku=plus` string. (The field is empty on this unit, so treat
it as "the platform provisions for it", not proof the coil is fitted.)

Values read off this unit:

| Field | Value |
| --- | --- |
| `device_type_id` | **`AJDZ5ML3MICE5`** |
| `productid2` | `1441FFFFFFFFFFFFFFFF` |
| `region` | `US` |
| `bootmode` / `postmode` | `1` / `0` |
| `dev_flags` / `fos_flags` | `0` / `0` |
| `product_name` / `productid` | `0` / `0` (unset) |
| `board_id` | `0060001400000021` |
| `usr_flags` | `0` |
| `manufacturing` | `PSN=P002…` `FSN=7792…` (unit serials) |
| **`unlock_code`** | **empty** — permanent unlock |
| **`t_unlock_code`** | **empty** — temporary unlock |
| **`t_unlock_cert`** | **empty** — temporary unlock cert |
| `unlock_version` | `7e bd 9a 96 0c 71 a1 00` — 8 bytes, **not** empty |

There are **four** unlock-related fields, not two. `unlock_code` is the
permanent-unlock slot and `t_unlock_*` the temporary-unlock pair; all three are
empty, i.e. this device has never been unlocked by either route.
`unlock_version` is the odd one — it holds eight bytes of non-printable data on
an otherwise never-unlocked device, so it is presumably a scheme/version tag
written at the factory rather than an unlock artefact. Worth resolving when LK
is dumped.

`device_type_id` is Amazon's internal device-type identifier, the same class of
string used in their OTA and registration APIs — the most searchable identifier
found so far, and better than the ASIN, which was never retailed.

Also `ro.build.lab126.project` = **`yacht_fireos_ship_7401`**. **Lab126** is
Amazon's own hardware R&D division, and `ship` marks it a production build — so
this was designed in-house, not an ODM white-label.

### Why this matters for the unlock

All three unlock slots being **empty** is the concrete reason
`ro.boot.flash.locked=1`. On Fire hardware the bootloader consults these IDME
fields, and unlocking means getting a valid entry written there.

What's publicly documented about the mechanism (sourced, but note
`xdaforums.com` blocks automated fetching so some is second-hand):

- The official path is `fastboot flash unlock unlock.bin`, and the image **must
  be signed by Amazon**, who do not issue signed unlock images to consumers.
- The signed value is **device-bound**, derived from the eMMC manufacturer ID
  and production serial number — so a cert is valid for one unit only, not a
  reusable key. (Both `/sys/block/mmcblk0/device/manfid` and `serial` are
  `Permission denied` to an unprivileged shell here.)
- 2019-and-newer Fire bootloaders added the **Temporary Unlock** path, good for
  a limited number of reboots — LK carries symbols
  `amzn_get_temp_unlock_idme_data` / `_cert` / `_code` and a runtime string
  *"Device is temporarily unlocked, %d reboots remaining"*. Those accessors are
  almost certainly the backing store for the `t_unlock_*` fields here.
- `amonet` / `cuber` cover only **pre-2020** Amazon hardware. There is a claim
  that newer Amazon units blow e-fuses to disable MediaTek download mode at the
  BootROM level — if that holds for this 2022 unit it would close the mtkclient
  route, so it is the first thing to test.

`/proc/idme/*` is `r--r--r--` (read-only even to root via procfs); writes go
through the `IIdme` HAL on the vendor side. And given `rpmb_state=2` plus
separate `tee1`/`tee2` partitions, unlock state may be anchored in RPMB rather
than IDME alone — in which case writing IDME directly would not be sufficient.
Unresolved; being researched.

The counterweight to all of this is `ro.oem_unlock_supported=1`, which retail
Fire tablets ship as `0`. That difference is real and unexplained, and is the
most promising thread.

## Shipmode (`com.amazon.shpm`) — a factory wipe, and a live footgun

Not a battery/transport mode, despite the name. **`Shipmode.apk` returns the
device to out-of-box state**: it is the routine Amazon runs before a unit ships
(or is decommissioned).

It runs as `sharedUserId="android.uid.system"` in the `system` process, holding
`MOUNT_FORMAT_FILESYSTEMS`, `CLEAR_APP_USER_DATA`, `FORCE_STOP_PACKAGES`,
`WRITE_SECURE_SETTINGS`, `REBOOT` and `SHUTDOWN`.

**Trigger:** a broadcast, `com.amazon.kindle.otter.shipmode`, on a receiver with
**no `android:permission` guard**. Extras: `ship_mode` selects
`mode_factory` / `mode_demo` / `mode_default`, plus an optional `rebootStatus`.
The receiver hands off to `ShipModeService` and then **disables itself**, so it
is one-shot.

`FactoryShipMode.populateTaskList()` runs, in order:

```
PreVerificationTask -> RemovePkgSettingsTask -> DeleteWiFiData -> ResetPropTask
  -> ReenableTask -> PostVerificationTask -> ShipModeFinishTask
  -> RebootTask (if com.amazon.hardware.multimodal) else ShutdownTask
```

What the individual tasks actually do:

- `DeleteWiFiData` → wipes `/data/misc/wifi`
- `RemoveDeviceFiles` → deletes `/data/system/locksettings.db`,
  `/data/system/password.key`, `/data/system/FACTORYMODE`
- **`ResetPropTask` → strips `adb` out of `persist.sys.usb.config`**, i.e.
  **turns USB debugging off**, and sets
  `SystemProperties.set("vendor.amazon.fos_flags.wipe", "1")`
- `ShipModeFinishTask` → sets `shipmode_status=shipmode_complete` and waits for
  a `shipmode_complete_acked` reply ("Failed to get ShipMode complete
  acknowledgement from the remote")

> ⚠️ **The safety guard does not work.** `PreVerificationTask.executeImpl()`
> checks `device_provisioned` and `user_setup_complete`, logs
> *"ShipMode called when device was provisioned"* / *"…when user setup was
> complete"* — and then **`return true` on every path**. It never aborts. So on
> this device, which *is* provisioned, a shipmode broadcast would proceed and
> wipe anyway.
>
> **Consequence for this project:** an accidental or malicious
> `am broadcast -a com.amazon.kindle.otter.shipmode --es ship_mode mode_factory`
> would disable ADB, delete the Wi-Fi config, and shut the device down —
> costing us the ADB authorization, developer options and the root foothold in
> one go. Treat that intent string as radioactive. It is worth considering
> `pm disable com.amazon.shpm` as a precaution.

Incidentally this is probably how the unit was cleaned before it reached the
surplus channel, and `RemoveDeviceFiles` deleting `locksettings.db` and
`password.key` explains why the RAFT credential files were absent — see
[raft-lockscreen.md](raft-lockscreen.md).

`mode_demo` routes to `DemoShipMode`, which pairs with the `com.amazon.kor.demo`
retail-demo package below.

## `com.amazon.kor.demo` — "Amazon Retail Demo"

Present on disk (5.3 MB in `/system/priv-app`) but **not registered** —
`pm list packages` does not know it, same as the stripped `redstone`. So it
cannot run without being installed first.

It is the **in-store display mode** for Fire tablets: `KioskHome`,
`PageflipperActivity`, `DemoConfigurationHubActivity`, `DemoStoreInfoActivity`,
`ResellDemoActivity`, `DOOBEInitiationActivity` (demo out-of-box experience).
Strings include *"Do you wish to activate demo mode? This is for retail store
use only."*, *"Alexa is enabled/disabled for this demo"*, and a "Resell Device"
flow that strips demo content before a unit is sold.

Three entry paths, which is the interesting part:

- **Cloud** — `com.amazon.dcp.messaging.topic.KindleDemoCloudApproach.ActivateDemo`
  / `.GetAvailableDemos`, i.e. remotely activated over DCP push.
- **Easter egg** — `SearchCommandBroadcastReceiver` listens for
  `com.amazon.kindle.unifiedSearch.EasterEgg`: the classic type-a-magic-string
  -into-search trick to drop a shop-floor unit into demo mode.
- **"Tardis Key"** — a **physical USB key** retail staff insert to load demo
  content. Strings: *"please insert a Tardis Key"*, *"Tardis Key might be
  corrupted"*, *"Incompatible Tardis Key"*, and a guard, *"Tardis Key does not
  work with devices that are provisioned and not in demo mode"*.

### The Tardis Key can carry system updates — but not usefully for us

`tardiskey/` contains `SystemUpdateService`, `SystemUpdater`,
`OTAControllerFactory`, and the string *"No system update or content update to
the device from this Tardis Key"* — so a Tardis Key is a **USB-delivered system
update** channel, which would be an attractive flashing path.

It is **dead on this image.** `SystemUpdateService.onHandleIntent()` copies the
file then calls `systemUpdater.install()`, which goes through
`OTAControllerFactory` and can throw `OTAUnavailableException` — and
`com.amazon.device.software.ota` **is not installed** here
(`E/RuntimePersistent: Package com.amazon.device.software.ota not found`, see
[network-behavior.md](network-behavior.md)). Even with it present, the install
would be a signed OTA sideload verified against Amazon's `otacerts`, so it is
not a signature bypass.

## Absent by design

Worth stating what *isn't* here, since it's equally deliberate: no Google Play /
GMS at all, and none of Amazon's consumer layer either — no Fire launcher, no
Appstore, no Silk, no Alexa, no `com.amazon.dcp`. Only eight Amazon *packages*
survive, all plumbing, while all the customization above sits below the app
layer. See [identification.md](identification.md).

`com.amazon.redstone` is the odd one out: its APK was stripped but its native
libraries were left behind (`libblueshift-opus.so`,
`libblueshift-audioprocessing.so`, `libgesture_sibyl.so`) — the voice stack
matching the `amazon.hardware.voicedsp` feature. Something was deliberately
removed from this image, the same way the Kerberos authenticator is missing.

## The takeaway

A factory reset was never going to help. The customization isn't installed on
top of Android — Android here *is* Amazon's build, verified at boot, with their
framework in every process, their services inside `system_server`, their own HAL
namespace, and their own SELinux class. Changing any of it requires breaking
verified boot, which is why everything routes back to the bootloader unlock in
[PROGRESS.md](PROGRESS.md).

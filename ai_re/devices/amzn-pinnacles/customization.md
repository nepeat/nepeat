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
| `fireos.hardware.fireosdha@2.0` | device health agent |
| `fireos.hardware.amazonthermal@1.0` | thermal management |
| `fireos.hardware.audio@2.0` | audio |
| `fireos.hardware.connectivity.networkpower@1.0` | network power |

Plus Amazon init scripts: `amazond.rc`, `amazon_crash_reporter.rc`,
`amazon_logd_old.rc` (their own logd), `fireos_logging.rc`, `fosflags.rc`,
`init.wipe_fos_flags.rc`.

## IDME — the factory identity block

`fireos.hardware.idme@1.0-service` is running and exposes `/proc/idme/`, a
26-field factory-programmed identity area. This is Amazon-specific; there is no
AOSP analogue.

```
bootcount      bootmode       bt_mac_addr    bt_mfg        co_tms_cal
dev_flags      device_type_id fos_flags      front_cam_otp iaicr
mac_addr       mac_sec        manufacturing  miccal.0      miccal.1
postmode       product_name   productid      productid2    rear_cam_otp
region         sensorcal      serial         t_unlock_cert t_unlock_code
tp_cg_color
```

Values read off this unit:

| Field | Value |
| --- | --- |
| `device_type_id` | **`AJDZ5ML3MICE5`** |
| `productid2` | `1441FFFFFFFFFFFFFFFF` |
| `region` | `US` |
| `bootmode` / `postmode` | `1` / `0` |
| `dev_flags` / `fos_flags` | `0` / `0` |
| `product_name` / `productid` | `0` / `0` (unset) |
| `manufacturing` | `PSN=P002…` `FSN=7792…` (unit serials — kept out of notes) |
| **`t_unlock_code`** | **empty** |
| **`t_unlock_cert`** | **empty** |

`device_type_id` is Amazon's internal device-type identifier, the same class of
string used in their OTA and registration APIs — the most searchable identifier
found so far, and better than the ASIN, which was never retailed.

Also `ro.build.lab126.project` = **`yacht_fireos_ship_7401`**. **Lab126** is
Amazon's own hardware R&D division, and `ship` marks it a production build — so
this was designed in-house, not an ODM white-label.

### Why this matters for the unlock

`t_unlock_code` and `t_unlock_cert` being **empty** is the concrete reason
`ro.boot.flash.locked=1`. On Fire hardware the bootloader consults these IDME
fields, and unlocking means getting a valid entry written there — historically
an Amazon-signed certificate bound to the device serial, which is why community
Fire unlocks have gone through exploits rather than `fastboot flashing unlock`.

`/proc/idme/*` is `r--r--r--` (read-only even for root, via procfs); writes go
through the `IIdme` HAL on the vendor side. So this is a lead to chase, not a
door that's currently open. Research on `AJDZ5ML3MICE5` and on the documented
unlock-cert mechanism is pending.

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

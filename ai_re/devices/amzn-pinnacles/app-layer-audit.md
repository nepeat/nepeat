# App-layer audit — provisioning, setup, and everything non-stock

Full sweep of all 87 APKs in `fw/app/` + `fw/priv-app/`, plus the two
framework-only packages (`amazon.fireos`, `android.amazon.perm`). Method: `aapt2
dump xmltree` on every manifest through a component/permission parser,
`classes.dex` keyword sweep across all 49 dex-carrying APKs, `jadx` on the Amazon
ones, `strings` on the AOT-only ones.

**Verdict for the flashing goal: the app layer is closed.** Nothing here advances
it, and one prior avenue is now positively ruled out.

## The app-layer search space is closed

- **There is no FRP partition.** The `by-name` listing shows `boot boot_para
  cache cam_vpu1-3 dkb kb keys lk metadata misc nvcfg recovery spmfw sspm_1
  system tee1 tee2 userdata vendor` — **no `frp`, no `persistent_data_block`, no
  `seccfg`, no `proinfo`.** So AOSP's `OemLockManager` / `PersistentDataBlockManager`,
  which Settings' "OEM unlocking" toggle drives, have **no backing store**. That
  toggle is inert at *both* ends: LK ignores it, and the framework has nowhere to
  record it. `oem_unlock_enable` is absent from `settings-global.txt`. This
  closes out the `ro.oem_unlock_supported=1` thread for good.
- **`Settings` is stock AOSP** — zero `com/amazon/*` or `com/fireos/*` classes in
  its dex. Its `oem_unlock` / `OemLockManager` / `MasterClear` / `RecoverySystem`
  code is unmodified upstream. Same for `SettingsIntelligence` and
  `ManagedProvisioning`.
- **Only three packages on the entire image reference IDME / flags / USB config**,
  and all three were already known: `Shipmode`, `com.amazon.kor.demo`, and
  (stock) `Settings`. **No other APK touches `idme`, `fos_flags`, `dev_flags`,
  `usr_flags`, `oem_unlock`, `fastboot`, or `bootloader`.**
- **There is no Amazon OOBE/setup wizard on this image at all** (no
  `com.amazon.kindle.otter.oobe`), so there is no setup-wizard surface — which
  independently corroborates the "never enrolled" finding.

## `com.amazon.platform.fdrw` — Factory Data Reset **Whitelist**

Not a generic key/value store, as earlier notes assumed. It is the mechanism
that decides **what survives a factory reset**.

The store itself is an ordinary SQLite `items.db` in the app's data dir and does
*not* survive a wipe. Persistence is a handoff into recovery's own directory,
driven from the boot classpath (`boot-fosframework.vdex`):

```
com.amazon.android.os.factoryresetwhitelist.ResetWhitelistGenerator
com.amazon.android.os.factoryresetwhitelist.FactoryResetWhitelistCallback
    (an android.os.VendorRecoverySystemCallback)
RESET_WHITELIST_FILE_CACHE = /cache/recovery/fdrw.conf
RESET_WHITELIST_FILE_DATA  = /data/cache/recovery/fdrw.conf
format tag: "fdrw.conf V1.0"
hook: getExtraFactoryResetBootCommand
```

On a reset, `system_server` queries `content://com.amazon.platform.fdrw/v1/items`,
serialises the rows to `fdrw.conf`, drops it in `/cache/recovery/`, and tells
recovery about it via a vendor callback that **appends extra arguments to the
recovery boot command**.

A second mechanism in `fosservices.vdex` (`PackageWhitelisterCallback`,
`APK_WHITELISTING_META_DATA`, `populateWhitelistedPackagesToConf`) writes
`/data/system/fdrw_apks.conf`, letting packages opt in via manifest `meta-data`
to have their APK path preserved across a wipe.

**Toward the goal:** this is a *persistence* primitive, not a flashing one. It
gets influenced content into `/cache/recovery/`, but always under the fixed
filename `fdrw.conf` — it cannot write `/cache/recovery/command`, so it is not a
route to `--update_package` sideload.

**The one genuinely open question:** `getExtraFactoryResetBootCommand`. If the
string it returns is influenced by `fdrw.conf` contents rather than hardcoded,
that is **argument injection straight into recovery's command line**. That could
not be resolved — it is AOT code in the boot image. It is the highest-value
unresolved item from this audit.

*Currently carrying nothing:* `/system/etc/fdrw_default.conf` is absent from this
image, `fdrw_apks.conf` is absent, and no package opts in.

## `com.fireos.arcus.proxy` — exported binder with zero permission checks

The one clear privesc/abuse finding. `com.fireos.arcus.proxy.ArcusProxy` is
`exported="true"` with **no `android:permission`**, and `ArcusProxyBinder extends
IArcusService.Stub` contains **no `checkCallingPermission`, `getCallingUid`, or
`enforce*` call anywhere**. Any app on the device can bind it and call:

- `openConfiguration(appId)` → full remote-config JSON for **any** Arcus app id
- `register` / `addAttribute` / `backupAttributes` → sets the **targeting
  attributes** the cloud uses to choose which config to return, so an
  unprivileged caller can steer what a privileged consumer receives
- `sync()` / `syncId()` → forces a network fetch under the proxy's uid
- **Broadcast injection:** `sendBroadcastToReceiver()` builds
  `new Intent("amazon.arcus.sync." + packageAppId)` from the **caller-supplied**
  id and fires `sendBroadcastAsUser(..., UserHandle.ALL)` — a privileged,
  `INTERACT_ACROSS_USERS_FULL`-holding app emitting an attacker-chosen action
  (prefix-constrained) to every user

It is privileged (`/system/priv-app`) with `INTERNET`, `ACCESS_NETWORK_STATE`,
`INTERACT_ACROSS_USERS_FULL`, though **not** `android.uid.system`.

**None of the reachable config keys touch lock state, IDME or flags.** The
consumers here are suspend policy, WebView JS blocking, game mode, and the
AppCompat whitelist. Nothing toward flashing.

## Smaller flags

- **`android.amazon.perm`** — a permission-declaration shim (`android.uid.system`,
  no components). Most of its ~100 permissions are signature-level, but a few are
  declared **`normal` or `dangerous`** and so are requestable by any third-party
  app: `amazon.permission.PREPARE_SHUTDOWN`, `PAIRING_MODE`,
  `BYPASS_P2P_AUTHENTICATION`, `ACCESS_NETWORK_MONITOR`, and a few provider read
  perms. Latent on this image (the enforcing components aren't installed), but
  `PREPARE_SHUTDOWN` at `normal` is a poor choice regardless.
- **`com.dolby.daxservice`** — privileged, holds `WRITE_SECURE_SETTINGS` +
  `MANAGE_USERS` + `INTERACT_ACROSS_USERS_FULL`, and exports `.DaxService` with
  no permission. Read it: the exported surface is Dolby audio parameter handling
  against the vendor HAL, exposing no settings-write API. Bad shape, not a
  working privesc.
- **`com.amazon.webview.chromium`** — exports `AWVArcusContentProvider` and
  `AWVArcusService` with no permission; config disclosure only, unprivileged
  package.
- **`com.amazon.kor.demo`** — additionally exports `EnableTardisKeyReceiver` on
  the **unprotected** action `com.amazon.kor.demo.ENABLE_TARDIS_KEY`, and holds
  `android.permission.RECOVERY` + `com.amazon.dcp.ota.permission.CONTROLLER`.
  Same shape as the Shipmode bug — but moot, the package is unregistered.

## Null results (checked, unremarkable)

Stock AOSP with no Amazon modifications: **`com.android.provision`** (the classic
3-line stub — sets `device_provisioned=1`, `user_setup_complete=1`, disables
itself; confirmed to have run once), **`com.android.onetimeinitializer`** (inert —
it rewrites a Launcher **2** favourites row, and this build ships Launcher3),
**`com.android.managedprovisioning`**, **`TabletPackageInstaller`** (no
silent-install path, no `EXTRA_NOT_UNKNOWN_SOURCE`/`INSTALL_ALL_USERS`/
`allowDowngrade` additions), **`TabletDeskClock`**, **`SettingsProvider`** (only
Amazon addition is a read cache), **`amazon.fireos`** (all components correctly
guarded by `BIND_JOB_SERVICE`), **`com.amazon.wirelessmetrics.service`**, and
**`com.amazon.redstone`** (confirmed dead — APK stripped, only native libs
remain, absent from the package list).

Everything else in `fw/app/` and `fw/priv-app/` is unmodified AOSP.

## Gaps worth closing

- **`fdrw` and the framework `.vdex` files were only `strings`-analysed.** They
  are CompactDex (`cdex001`); jadx and apktool both fail. Reading
  `getExtraFactoryResetBootCommand` and `ResetWhitelistGenerator` properly needs
  `compact_dex_converter` or `vdexExtractor` — neither is in nixpkgs, so this
  wants a manual build.
- **The `fw/etc` and `fw/bin` pulls are incomplete.** `fw/etc` has only 15 files
  and contains **no `permissions/`, no `sysconfig/`, no `init*.rc`**; `fw/bin`
  has no `idme` binary. `sysconfig/` is where privapp-permission whitelists live,
  so the privilege analysis above is manifest-derived and not cross-checked
  against the platform whitelist. Now that we have root, re-pull
  `/system/etc/permissions/`, `/system/etc/sysconfig/`, `/system/etc/init/` and
  `/vendor/etc/` before treating it as final.

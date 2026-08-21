# What this device does on Wi-Fi

Assessment before putting the tablet on a WLAN. **Headline: RAFT transmits
nothing, there is no OTA client on this image at all, and what does leave is
three low-sensitivity beacons.**

Evidence is from the pulled trees in `fw/` and the live captures in `dumps/`.

## RAFT is inert — confirmed null result

**`RaftSystemUI` cannot open a socket.** `aapt dump permissions` returns **zero**
INTERNET permissions. It declares `sharedUserId="android.uid.systemui"`, and no
other installed package shares that UID, so the systemui UID never gets the
`inet` group.

**Its metrics dispatcher targets a package that does not exist.**
`com/amazon/raft/dispatch/Event.java`:

```java
private static String servicePackageName = "com.amazon.raftsystemservice";
intent.setPackage(packageName);   // explicit — cannot fall through elsewhere
context.startService(intent);     // SecurityException caught, logged, returns
```

`com.amazon.raftsystemservice` appears nowhere in `fw/priv-app`, `fw/app`, or the
live package list. Every `sendMetrics()` is a caught exception.
`RaftDispatchMetricsDispatcher` contains no HTTP or socket code — it builds a
JSON blob and fires a local intent. `sendLoginBroadcast`/`sendLogoutBroadcast`
are sticky broadcasts of `com.amazon.raft.LOGIN` with no receiver installed.

**Passwords are never in the payload.** `SessionEvent.getEventPayload()` emits
only `status`, `timeElapsed`, `statusMessage`, `userName`; the password is never
passed to an event object. The DSN *is* embedded in the event ID
(`timestamp + "_" + Build.SERIAL + "_" + md5`) — it just never leaves the device.

### The one real RAFT network artifact: a DNS leak, user-triggered

`KerberosService` is **not** in the APK — it runs inside **system_server**,
which does have INTERNET. Live proof in `dumps/logcat-d.txt`:

```
I/SystemServiceManager: Starting com.amazon.raft.kerberos.service.KerberosService
I/KerberosService: Using directory /data/system/kerberos
```

It is backed by a real bundled MIT krb5 (`fw/lib/libkerberosapp.so`, JNI
`nativeKinit`/`nativeKvno`/`nativeKdestroy`) with a hardcoded config template:

```
default_realm = ANT.AMAZON.COM
.amazon.com   = ANT.AMAZON.COM
_kerberos._tcp.ant.amazon.com
```

KDCs are found by **DNS SRV lookup**. If that ever fires, your resolver sees a
query for `_kerberos._tcp.ant.amazon.com` — an unmistakable "Amazon corp device"
fingerprint.

**But it only fires if a human types credentials at the lockscreen.**
`createTicket()` is reached solely from `RaftKeyguardAccountView.verifyAccount()`;
nothing calls `initKerberosConfig`/`getKdcList` at boot. *Honest uncertainty:*
`fosservices.vdex` is CompactDex and could not be decompiled, so a boot-time SRV
lookup cannot be fully ruled out — the boot logcat shows none, but the device was
offline when captured.

**So: don't type anything into the RAFT login screen while on Wi-Fi.** That is
the whole RAFT risk.

## What actually leaves on WLAN join

| # | What | Where | Contains | Sensitivity |
| --- | --- | --- | --- | --- |
| 1 | Captive-portal probe | `http://tabletcaptiveportal.com/generate_204` (Amazon-operated) | bare GET + your public IP | low, but Amazon-branded |
| 2 | **Arcus remote config** | **`https://arcus-uswest.amazon.com`**, every **24 h** | appId ARNs, `_applicationIdentifier`, `_applicationVersion`, locale, `_hardware`, `product=pinnacles buildConfig=raft`. **No DSN, no account.** | low data, but fingerprints an Amazon-internal RAFT build |
| 3 | NTP | `time.android.com`, fallback `kindle-time.amazon.com` | time query | negligible |

Arcus (`com.fireos.arcus.proxy`) is the notable one — it is a config **pull**, not
a metrics upload, and logcat shows it has never synced
(`W/ArcusCacheHelper: file not found, Arcus probably yet to sync`).

**AutomaticTimeZone is the sharpest edge, currently blunt.**
`com.amazon.kindleautomatictimezone` *has* INTERNET, `auto_time_zone=1`, and the
`atz_prediction_on_wifi_connect` feature is present, so it fires on Wi-Fi connect:

```
GET https://dcape-na.amazon.com/getCustomerTimezone?...&dsn=<deviceType>
    auth: ADPAuthenticator, directedId = MAPAccountManager.getAccount()
```

Good news: **your SSID is never transmitted** — it is only a local SQLite cache
key, and `maskPii()`'d in logs. With 0 accounts, `getAccount()` returns null and
the request fails through the retry policy. **Gotcha:** the `auto_time_zone`
check happens *before applying* the timezone, not before making the request, so
setting it to 0 may not stop the call.

That APK bundles the entire MAP/DCP stack (`dcape-na`, `firs-ta-g7g`,
`api.amazon.com`, `*.account.amazon.com`, `/FirsProxy/registerDevice`).
**Register an Amazon account and all of it activates. Don't.**

## OTA: there is no client

This matters most, because an OTA would patch CVE-2022-38181 and cost us root.

- **No OTA package installed.** The framework wants one and fails every boot:
  `E/RuntimePersistent: Package com.amazon.device.software.ota not found`.
- **No update endpoint URL exists anywhere on the image.**
- The download engine is dead too: `com.android.providers.downloads` failed to
  install (`Required FireOS Feature "com.fireos.sdk.metrics:1" not on device`);
  only the UI shell is present.
- No MAP/account framework to authenticate a download.

Remaining OTA risk is **manual only** — BCB in `misc`, recovery, or sideload
into `/data/ota_package/`, all needing physical or root access.

## Confirmed inert

- `com.amazon.wirelessmetrics.service` — **no INTERNET** permission; writes to
  SharedPreferences. Its Minerva forwarder fails outright.
- `com.amazon.shpm` (Shipmode) and `com.amazon.platform.fdrw` — no INTERNET,
  purely local.
- **`fireosdha` is not a "device health agent"** — it is **Device Hardware
  Attestation**, a HIDL signing/attestation HAL (`getDhaPublicKey`,
  `getDhaCertificateChain`, `sign`). No network. *(Corrects an earlier guess in
  [customization.md](customization.md).)*
- `amazonfiled`/`amazondropbox` — a real PII-redacting crash/log pipeline
  (`fw/etc/privacy_filter.conf`, 165 redact patterns) writing to **local**
  DropBox with **no uploader installed to drain it**.
- Minerva/DCM/KDM are API façades only; they degrade to `NullMetricsFactory`.
- Orphaned Alexa AVS SDK 1.22 in `fw/lib` (`alexa.na.gateway.devices.a2z.com`,
  `alexa-comms.device-metrics-us-2.amazon.com`) with **no consumer package**.

## How to stop it

Most robust — block by name at the router:

```
arcus-uswest.amazon.com        tabletcaptiveportal.com
fireoscaptiveportal.com        kindle-time.amazon.com
dcape-na.amazon.com            firs-ta-g7g.amazon.com
api.amazon.com                 *.account.amazon.com
_kerberos._tcp.ant.amazon.com   <-- watch for this; it means RAFT tried
```

On-device (needs root; both are system-UID priv-apps):

```sh
pm disable-user --user 0 com.fireos.arcus.proxy
pm disable-user --user 0 com.amazon.kindleautomatictimezone
settings put global captive_portal_mode 0
settings put global auto_time_zone 0     # may not stop the request — see above
```

**Leave `com.android.systemui` (RAFT) alone** — it does nothing on the network.

Worth noting for later: the Arcus fetcher ships a `TrustAllManager` and honours
`com.amazonaws.sdk.disableCertChecking` — a clean MITM point if you ever want to
see the real sync payload rather than infer it.

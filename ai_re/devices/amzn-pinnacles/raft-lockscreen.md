# RAFT — the Kerberos shift-login lockscreen

**This is why setting a PIN drops you at a username/password login.** The
device does not run stock SystemUI. It runs Amazon's **RAFT** SystemUI, which
replaces the AOSP keyguard with a two-stage corporate shift-login: authenticate
against a Kerberos KDC with a username and password, then unlock with a
short-lived *session PIN* for the rest of the shift.

Traced 2026-08-20 by decompiling the on-device APK. Artifacts in
[`apks/`](apks/); `RaftSystemUI.apk` is 20 MB, pulled from the device.

## SystemUI is replaced wholesale

```
package:/system/priv-app/RaftSystemUI/RaftSystemUI.apk=com.android.systemui
```

It keeps the AOSP package name, so `pm list packages` looks stock — you only
see it by listing `/system/priv-app`. Inside, `com.amazon.raft.*` subclasses the
AOSP keyguard classes:

| RAFT class | AOSP class it overrides |
| --- | --- |
| `RaftKeyguardSecurityModel` | `KeyguardSecurityModel` |
| `RaftKeyguardSecurityContainer` | `KeyguardSecurityContainer` |
| `RaftKeyguardAccountView` | (new) username/password view |
| `RaftKeyguardSessionView` | `KeyguardPinBasedInputView` |
| `RaftLockPatternUtils` / `RaftLockPatternChecker` | `LockPatternUtils` |

## The redirect, exactly

`KeyguardSecurityModel.getSecurityMode()` is the AOSP hook that decides which
unlock view to draw. RAFT overrides it and remaps password *quality* to two
custom security modes:

```java
int security = this.mLockPatternUtils.getActivePasswordQuality(userId);
if (security == 131072 || security == 196608)          // NUMERIC, NUMERIC_COMPLEX
    return SecurityMode.Session;
if (security == 262144 || security == 327680
    || security == 393216 || security == 524288)       // ALPHABETIC, ALPHANUMERIC,
    return SecurityMode.Account;                       // COMPLEX, MANAGED
return super.getSecurityMode(userId);
```

The trap is in the *first line*. `mLockPatternUtils` is
`RaftLockPatternUtils`, which also overrides `getActivePasswordQuality()` —
and it does not report the quality you chose:

```java
public int getActivePasswordQuality(int userId) {
    int adminQuality = getAdminPasswordQuality(userId);   // the real AOSP quality
    if (adminQuality == 0) return adminQuality;           // no credential set → 0
    return (int) getLong("lockscreen.enterprise_password_type", 393216L, 1000);
}
```

So the moment **any** credential exists, the real quality is discarded and RAFT
substitutes `lockscreen.enterprise_password_type`, which on a fresh device is
unset and **defaults to 393216 (`PASSWORD_QUALITY_COMPLEX`) → `SecurityMode.Account`**
— the username/password login.

That is the whole answer. It is not that "PIN maps to Session". It is that
**setting any lock at all — PIN, password, or pattern — puts you on the
Kerberos account login**, because the enterprise password type has never been
initialised. Only `adminQuality == 0`, i.e. no credential whatsoever, falls
through to stock AOSP behaviour.

`lockscreen.enterprise_password_type` is only ever written in one place:
`verifyTemporaryToken()` sets it to 131072 (NUMERIC) *after* a successful login
and session-PIN enrolment. So the Session PIN pad is unreachable until you have
logged in at least once — which is exactly the shift-login model.

Note these values live under **userHandle 1000**, a hardcoded pseudo-user, in
the locksettings database — not in `Settings.Secure` and not under user 0.

`RaftKeyguardSecurityContainer.showNextSecurityScreenOrFinish()` spells out the
intended order:

```java
case Session:  strongAuth = true; finish = true;  break;   // session PIN → unlocked
case Account:  strongAuth = true; finish = false;
               showSecurityScreen(SecurityMode.Session);    // login → now set a session PIN
```

Account login **does not unlock the device** — it advances you to the session
PIN screen. Login, then set/enter a session PIN, then you're in.

The user-facing strings confirm the shift model:

```
keyguard_hint_username     "Enter Username"
keyguard_hint_password     "Enter Password"
keyguard_login             "Login"
keyguard_create_pin        "Create new session pin."
keyguard_enter_pin         "Enter session pin."
keyguard_verify_pin        "Confirm session pin."
keyguard_session_invalid   "Session is no longer valid. Please logout."
keyguard_session_clock_skew "Session is invalid. System clock needs to be synced.
                             Please contact your local administrator."
```

`RaftKeyguardSessionView` also wires a **`logout_button`** into the PIN pad
(`terminateSession()`), and logs `LoginSessionEvent`, `LogoutSessionEvent`,
`UnlockSessionEvent` metrics. That is a device meant to be handed between people
on shift, not owned by one person.

## The emergency credential — hardcoded, and it works offline

`RaftLockPatternUtils.verifyAccount()` checks a hardcoded credential **before**
it ever contacts Kerberos:

```java
public boolean verifyAccount(String username, String password) throws VerificationException {
    ...
    terminateSession();
    if (username.equals("") && password.equals("letmein")) {
        setString("USERNAME_KEY", "backdoor", 1000);
        sendMetrics(this.loginEvent, false, "");
        return true;                       // authenticated — no Kerberos involved
    }
    if (!isAccountValidKerberos(username, password)) {
        return false;
    }
    ...
```

> **Username: *(leave blank)* — Password: `letmein`**

Because the check sits above `isAccountValidKerberos()`, it needs no KDC, no
network, no `com.amazon.kerberos` authenticator, and no valid clock. It works on
exactly this device, in exactly this stripped state.

It then stores the literal username **`backdoor`**, and that value is special-
cased again in `verifySession()`:

```java
if (!getString("USERNAME_KEY", 1000).equals("backdoor")) {
    if (isSessionExpired())  throw new VerificationException(SESSION_EXPIRED);
    if (!isAccountVerified()) throw new VerificationException(SESSION_CLOCK_SKEW);
}
```

So a `backdoor` session **skips the ticket-expiry and clock-skew checks
permanently** — which matters here, because this unit's RTC is wrong and those
checks would otherwise fail forever.

### The full unlock sequence

1. At the RAFT login screen, leave **Username empty**, enter **`letmein`** as
   the password, tap **Login**.
2. `showNextSecurityScreenOrFinish()` advances you to the session PIN screen
   rather than unlocking.
3. `verifyTemporaryToken()` runs a two-pass enrolment: the first PIN you type is
   stashed in `TEMP_TOKEN` and throws `VERIFY_TOKEN` → *"Confirm session pin."*
   Type the **same PIN again**; it matches, gets promoted to `TOKEN_KEY`, and
   `lockscreen.enterprise_password_type` is set to 131072 (NUMERIC).
4. You're in. From now on `getSecurityMode()` returns `Session`, so the
   lockscreen shows the PIN pad and your session PIN works normally.

Note the session PIN is stored via `ILockSettings.setString` as **`TOKEN_KEY`
in cleartext** and compared with `String.equals` — no hashing, no gatekeeper.

This is a hardcoded static credential in production-signed, release-keys Amazon
firmware. It is not a debug build. Worth flagging as a finding in its own right,
not just as a way in.

## Why a *real* login can never succeed here

The emergency credential above works. A genuine Kerberos login does not, and
this is why. Two halves, and only one is present.

**The service half is alive.** `raft_kerberos` is a registered binder service:

```
$ service list | grep raft
20  raft_kerberos: [com.amazon.raft.kerberos.service.IKerberosService]
```

So `IKerberosService` (`CreateTicketResult`, `GetAuthTokenResult`,
`KerberosConfigInfo`, `KerberosTicketInfo`) is compiled into the framework
itself, and the feature flag `com.amazon.raft.feature.kerberos` is declared in
`/system/etc/permissions/raft_kerberos_feature.xml`.

**The authenticator half is missing.** `RaftLockPatternUtils.registerAccount()`
walks `AccountManager.getAuthenticatorTypes()` looking for type
`com.amazon.kerberos`:

```java
for (AuthenticatorDescription authDescription : accountManager.getAuthenticatorTypes()) {
    if ("com.amazon.kerberos".equals(authDescription.type)) { authenticatorAvailable = true; break; }
}
if (!authenticatorAvailable)
    throw new VerificationException(VerificationError.SERVICE_UNAVAILABLE,
                                    getString(R.string.authenticator_unavailable));
```

On this device that loop finds nothing — the only registered authenticator is
`com.android.email`, and no package matching `kerberos` is installed. So it
throws, and the string it throws is:

> **"Kerberos AccountAuthenticator not installed."**

There is no realm, KDC hostname or URL anywhere in the APK — all of that would
have come from `KerberosConfigInfo` at runtime, supplied by the missing app.

**And the clock is wrong anyway.** `keyguard_session_clock_skew` exists because
Kerberos rejects tickets outside a few minutes of skew. This unit's RTC falls
back to the 2023 kernel build date with no NTP sync, so even a fully provisioned
device in this state would fail authentication.

## What this says about the device

This is the strongest evidence yet for the fixed-purpose reading in
[identification.md](identification.md). A shared tablet with:

- Kerberos corporate login at the lockscreen, with per-shift session tracking
- a logout button on the unlock screen
- NFC with Mifare (badge/tag reading)
- a rear camera with flash (barcode/label scanning)
- Ethernet support (dock or cradle)
- no consumer shell, no Appstore, no Alexa

…reads as **an Amazon internal / enterprise shared-use device** — warehouse,
logistics, retail floor or similar, where staff badge in, work a shift on a
pooled tablet, and log out.

Note it authenticates against *Kerberos*, i.e. a corporate directory, not
Amazon's consumer account system. Nothing here talks to an Amazon retail
account at all.

## Practical consequences

- **Every lock type lands on the account login.** PIN, password *and pattern*
  all set a non-zero `adminQuality`, at which point
  `getActivePasswordQuality()` substitutes the uninitialised
  `lockscreen.enterprise_password_type` (default `COMPLEX`) and you get
  `SecurityMode.Account`. There is no "safe" lock type while RAFT is in place —
  only *no lock at all* falls through to stock AOSP.
- **It is not a lockout, though**, thanks to the emergency credential above:
  blank username + `letmein` gets you in without Kerberos, and the `backdoor`
  username then suppresses the expiry and clock-skew checks that would
  otherwise defeat you on this dead-clock unit.
- Right now the device has **no credential set at all**
  (`gatekeeper.password.key` absent), so nothing is engaged. Simplest to keep it
  that way until the bootloader is unlocked and RaftSystemUI can be replaced.

Replacing `RaftSystemUI.apk` with stock AOSP SystemUI would restore normal
PIN/password unlock, but that needs a writable `/system` — so it is gated on
the bootloader unlock in [PROGRESS.md](PROGRESS.md).

## Escape hatch — tested and working

**If a lock gets set and RAFT strands you at the login screen, clear it over
ADB:**

```bash
adb shell locksettings clear --old <the credential you set>
```

This works because RAFT only overrides the **keyguard UI**. The credential
itself is a stock AOSP gatekeeper credential written by the unmodified
`ChooseLockPassword` flow, and `locksettings` talks straight to the
`lock_settings` binder service (`ILockSettings`) — it never goes near
`RaftKeyguardSecurityModel`. So the layer that traps you is bypassed entirely.

Verified on the device 2026-08-20 by setting a pattern and clearing it again.
(The pattern was chosen believing it bypassed RAFT — per the corrected analysis
above it does not, so the escape hatch was in fact exercised against a live RAFT
lock, which makes the result stronger, not weaker.)

```
$ locksettings set-pattern 1236
Pattern set to '1236'
$ ls -l /data/system/gatekeeper.*.key
-rw------- 1 system system 0 ... /data/system/gatekeeper.password.key
-rw------- 1 system system 0 ... /data/system/gatekeeper.pattern.key
$ locksettings clear --old 1236
Lock credential cleared
$ ls /data/system/gatekeeper.*.key
No such file or directory
```

Device was left with no credential set.

**You must know the credential.** A wrong one is rejected outright
(`Old password '9999' didn't match`), and `locksettings clear` with no `--old`
throws inside `LockPatternUtils.checkCredential` — don't rely on it. In the
realistic case this doesn't matter: you get stranded by setting a PIN
*yourself*, so you know what it is.

**The safety property that makes this work: ADB authorization survives the
keyguard.** A locked screen does not revoke the ADB key, and `adb shell` keeps
working while the device sits at the RAFT login. Verified — the login screen is
an app-layer trap, not a debug-access one.

### Fallback ladder

1. **On the device itself:** blank username + `letmein` at the login screen,
   then enrol a session PIN. No host, no cable needed.
2. **`adb shell locksettings clear --old <credential>`** — above; removes the
   lock entirely and returns to stock behaviour.
3. Recovery → factory reset. Guaranteed, and `/data` holds ~133 MB of nothing,
   so no data is lost. **But it wipes developer options and the ADB
   authorization**, which is this project's actual crown jewel — you'd be back
   to the setup wizard and re-enabling USB debugging by hand. Last resort, not
   a convenience.
4. `fastboot flashing unlock`, once the bootloader work in
   [PROGRESS.md](PROGRESS.md) happens. Also wipes.

Two independent routes — one on-device, one over ADB — so being stranded would
take both failing at once.

### Rules of thumb

- **No lock type avoids RAFT.** Don't reach for pattern thinking it's safe; it
  isn't. Only "None"/"Swipe" leaves stock behaviour intact.
- Before setting anything, confirm ADB still works and write the credential
  down — `locksettings clear` needs the correct `--old` value.
- If you *want* a working lock today, the intended path is to log in with the
  emergency credential once and enrol a session PIN. After that
  `enterprise_password_type` is NUMERIC and the lockscreen behaves like a normal
  PIN pad.

## Other non-stock system apps

Found by listing `/system/priv-app` and `/system/app` rather than
`pm list packages`, which hides them behind AOSP names:

| APK | Package | Notes |
| --- | --- | --- |
| `RaftSystemUI` | `com.android.systemui` | the above |
| `Shipmode` | `com.amazon.shpm` | ship/transport low-power mode |
| `ArcusListener` | `com.fireos.arcus.proxy` | "Arcus" listener service |
| `fdrw` | `com.amazon.platform.fdrw` | factory data reset helper |
| `com.amazon.redstone` | — | **APK removed, native libs left behind**: `libblueshift-audioprocessing.so`, `libblueshift-opus.so`, `libgesture_sibyl.so`, `libjni_latinime.so` — Opus audio + gesture recognition, i.e. the voice stack matching `amazon.hardware.voicedsp` |
| `com.amazon.kor.demo` | — | retail demo mode |
| `TabletLockscreenWallpaper`, `YGPS` | — | MediaTek GPS factory test app is still present |

`com.amazon.redstone` having its APK stripped but its libraries left is worth
noting — something was deliberately removed from this image, the same way the
Kerberos authenticator is missing.

Also observed in logcat: a custom SELinux object class,
`tclass=amazon_policies` with a `see_home_task` permission, denied to
`untrusted_app_25`. Amazon extended the SELinux policy with their own class,
not just their own types.

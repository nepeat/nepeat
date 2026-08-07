# Perle IOLAN SCR1618 — `bypass_login` Analysis

**Target:** extracted rootfs at `extracted/rootfs` (Marvell Armada 3700 arm64, VyOS 1.2.0-rolling / Debian jessie, kernel 4.14.235)
**Question:** Is the config-driven `bypass_login` code path a backdoor, a debug feature, or a legitimate mechanism, and how is it triggered?
**Method:** string sweep across the whole rootfs, then reverse of the consuming binaries with radare2 (aarch64). auth.cgi additionally imported into the shared Ghidra instance (`/perle-bypass/auth.cgi`) for cross-check.

**Verdict (up front):** **Legitimate first-boot provisioning + physically-gated password-recovery mechanism. Not a backdoor, not debug-left-in.** It is *not* remotely exploitable on a configured unit and is *not* default-enabled once a config has ever been saved.

---

## 1. Where the flag appears

`grep` for `bypass_login` and variants across the entire rootfs (binaries + text):

| File | Evidence |
|---|---|
| `usr/www/root/auth.cgi` | ELF aarch64 CGI. Contains symbols `config_get_bypass_login`, strings `DS.Config.auth.bypass_login`, `DS.Auth.bypass_login`, `DS.System.express_setup_mode`, `DS.System.password_recovery_mode`. **This is the only binary that consumes the web `bypass_login`.** |
| `usr/www/templates/page-login.html` | ClearSilver template. `<?cs if:!DS.Auth.bypass_login ?>` gates the username/password form (lines 29, 61); `<?cs if:DS.Auth.bypass_login ?>` (line 112) focuses the login button. |
| `usr/www/templates/logout.html` | line 10: `<?cs if:!DS.Config.auth.bypass_login && !Query.noredirect ?>`. |
| `usr/www/templates/fast_setup.html` | `DS.System.express_setup_mode && DS.System.is_factory` (line 3), etc. |
| `lib/libglobals.so`, `lib/libiol_globals.so` | Define `get_express_setup_mode`, `cfg_common_is_factory`, `cfg_common_get_password_recovery`; hold literal strings `Bypass-Login`, `Bypass Login`, `Bypass-password`, `Bypass Password` (see §5 — a *separate* CLI feature). |
| `usr/bin/portctl` | references `express_setup_mode` (I/O port control daemon). |
| `sbin/prodinit`, `usr/bin/iol_perleinit` | boot-time product init: reference `/product/factory_default`, RESET-button detection. |

`config/` and `product/nvram/` in this extracted image are **empty** (runtime-populated dirs), so no static default enables anything.

No hits for `backdoor`. `skip_login` / `bypasslogin` / `nologin` / `autologin` produce only unrelated Debian package text.

---

## 2. What `bypass_login` resolves to (the web path)

### `config_get_bypass_login` — `auth.cgi` @ `0x0000df20`
Reverses to a one-liner:

```c
int config_get_bypass_login(ctx) {
    return htmlui_get_int_var(ctx, "DS.System.express_setup_mode", /*default*/ 0);
}
```
Disassembly evidence: `adrp/add x1 -> 0x2e698 "DS.System.express_setup_mode"`, `mov w4,1; mov w3,0; mov w2,0`, tail-call `b sym.htmlui_get_int_var`.

So **`bypass_login` is an alias for the dataset variable `DS.System.express_setup_mode`.**

### Where `DS.System.express_setup_mode` is set — `htmlui_page_common_init` @ `0x0000d94c–0x0000d9c8`
```c
have_startup_config = file_exists("/product/nvram/startup-config");   // 0x0000d954
set_int("DS.System.have_startup_config", have_startup_config);
host = get_string("HTTP.Host");                                       // 0x0000d984
is_factory = have_startup_config ^ 1;                                 // 0x0000d998  (= !have_startup_config)
express = (strcmp(host, "192.168.0.1") == 0) ? is_factory : 0;        // 0x0000d990 / d9a0 csel
set_int("DS.System.express_setup_mode", express);                    // 0x0000d9ac
set_int("DS.Config.auth.bypass_login", config_get_bypass_login());   // 0x0000d9b4 / d9c8
```
i.e.:

> **`bypass_login = 1` iff the unit has NO saved startup-config AND the browser reached it on the factory default IP `192.168.0.1`. Otherwise `bypass_login = 0`.**

### `htmlui_get_webmgr_mode` @ `0x0000de80`
```c
int webmgr_mode() {
    if (get_int("DS.System.password_recovery_mode")) return 2;  // password recovery
    if (get_int("DS.System.express_setup_mode"))     return 1;  // express/fast setup
    return 0;                                                   // normal
}
```

---

## 3. What the bypass actually skips — `htmlui_page_receive_login` @ `0x0000edc0`

This is the POST handler for the login form. Control flow (radare2):

```
0x0000eec0  bl config_get_bypass_login      ; w22 = bypass_login
0x0000eecc  cbz w22, 0xf168                 ; bypass==0 -> NORMAL path
            ; --- bypass != 0 : build PAM user from BSS buffer 0x4e1a8, pam_start("html", ...) ---
0x0000eeb8/f168 (NORMAL):
            htmlui_get_and_save_string_var("Query.username")   ; read creds from request
            htmlui_get_and_save_string_var("Query.password")
0x0000eff4  pam_set_item(..., PAM_AUTHTOK/creds)
0x0000effc  cbz w22, 0xf1c0                 ; NORMAL path -> authenticate
0xf1c0:     pam_authenticate(pamh, 0x8000)  ; <-- real password check
0xf1c8/f1dc pam_acct_mgmt(pamh, 0x8000)
            ; bypass path (w22!=0) jumps straight to 0xf000/get_auth_env, NEVER reaching pam_authenticate
```

**Key result:** when `bypass_login != 0`, the code path that calls `pam_authenticate()` / `pam_acct_mgmt()` is **not executed**. The handler goes directly to `get_auth_env` → `cfgdb_get_username` → session/redirect. No password is verified.

**But what identity does it grant?** In the bypass branch the PAM username is built by `snprintf` from a global BSS buffer at `0x4e1a8` which is **zero/empty** in the image, and `cfgdb_get_username()` is then looked up. Combined with the templates, the exposed surface is scoped, not a root shell:

- `page-login.html` lines 30/64/67–69: when `is_factory` → renders **"Factory Mode. Please use fast setup to configure a user"** with a single **"Get Started"** link to `/manage.cgi/fast-setup-initial`.
- lines 64–65: when `password_recovery_mode` → renders a **"Recover Password"** link to `/manage.cgi/recover_password`.

So the bypass replaces the credential prompt with the **fast-setup wizard** (to create the first admin) or the **password-recovery** flow — the two legitimate mechanisms for (re)establishing admin credentials. It is the intended UX for a box that currently has no usable credentials.

---

## 4. The underlying triggers (libglobals.so + prodinit)

### `cfg_common_is_factory` — `libglobals.so` @ `0x002284b0`
```c
int cfg_common_is_factory() {
    return !file_exists("/product/nvram/startup-config");
}
```
Factory = there is no saved configuration.

### `cfg_common_get_password_recovery` — `libglobals.so` @ `0x00227890`
```c
int cfg_common_get_password_recovery() {
    FILE *f = fopen("/product/password-recovery", "r");
    if (!f) return -2;
    fgets(buf,3,f); fclose(f);
    if (buf[0]=='1') return 1;
    if (buf[0]=='0') return 0;
    return -1;
}
```
Password-recovery mode is driven by an on-flash **flag file** `/product/password-recovery`.

### `get_express_setup_mode` — `libglobals.so` @ `0x00228504`
Reads `cfgdb_get_common()->field@0x20` (a persisted config-DB "express setup" flag). (Note: `auth.cgi` computes its own `express_setup_mode` directly from `is_factory && host==192.168.0.1`, §2; the CLI/portctl side uses this cfgdb field.)

### Physical trigger — `sbin/prodinit` @ `fcn.000025a4`
At boot prodinit:
- checks `file_exists("/product/factory_default")` (0x2cfc/0x2d00) and, in a poll loop, reads the **physical RESET button** via `SAM_get_user_status` (helper `fcn.00001af0`, called at 0x2d14/0x2d6c).
- Prints `"***** Detected RESET button pressed  on bootup *****"` vs `"***** NO RESET button being pressed, continue on *****"`.
- On RESET/`factory_default` → `"***** Performing FACTORY DEFAULT *****"`, removes `/opt/vyatta/etc/config/config.boot*`, and boots factory-default (no startup-config) — which is exactly the state that makes `is_factory`/`express_setup_mode` true.

So the two ways to reach `bypass_login=1`:
1. **Factory / never-configured unit** (no `startup-config`) reached at `192.168.0.1` → express/fast-setup. Reaching this state on a deployed unit requires the **physical RESET button** (or an explicit factory reset).
2. **Password recovery**: `/product/password-recovery` flag file present (set by the physically/boot-triggered recovery procedure).

Both require either a virgin unit or **physical access**.

---

## 5. The `libglobals` "Bypass-Login" strings are a *different*, unrelated feature

`lib/libglobals.so` / `lib/libiol_globals.so` contain literals `Bypass-Login` (`0x2a5208`) + display `Bypass Login` (`0x2a5218`), and `Bypass-password`/`Bypass Password`. These are entries in a **CLI keyword pointer table** at `0x3764d0`, adjacent to `Set-Bits`, `Clear-Bits`, `Register`, `Page`, `Module`, `Session-Timeout`, `Details`, `Authorized-Hosts`. This is the console-server / I/O-module CLI grammar (per-line/session settings), **not** the web `DS.Auth.bypass_login`. It is a documented console-line configuration knob (bypass the login prompt on a direct serial/console connection), separate from the express-setup web path analyzed above, and equally operator-configured rather than hidden.

---

## 6. Assessment

**Confirmed:**
- Web `bypass_login` == `DS.System.express_setup_mode`, which in `auth.cgi` is `(!file_exists("/product/nvram/startup-config")) && (HTTP.Host == "192.168.0.1")`.
- When set, `pam_authenticate`/`pam_acct_mgmt` are skipped, and the UI is redirected to the fast-setup wizard (create first admin) or, under `password_recovery_mode`, to the password-recovery flow.
- `password_recovery_mode` is gated by the on-flash `/product/password-recovery` flag; factory state is gated by absence of `startup-config`, which on a deployed box requires the physical RESET button (`prodinit` + `SAM_get_user_status`).
- On any unit that has ever saved a config, `express_setup_mode=0`, so the normal PAM-authenticated login form is served. Not default-enabled in the field.

**Hypothesis / not fully pinned:**
- Exact record `cfgdb_get_username("")` returns in the empty-buffer bypass branch (image ships with empty user DB); behavior is inferred from the templates' factory/recovery UX rather than executed. The scoped surface (fast-setup / recover-password only) is strongly supported by the template gating but was not dynamically executed.

**Backdoor vs feature:** This is a **legitimate mechanism**, matching standard appliance behavior (Perle IOLAN and comparable console servers ship a "fast setup" first-boot wizard and a physical-reset / password-recovery path). Reasons it is not a backdoor:
1. Requires no-config state **and** the factory IP, or a physical/boot recovery flag — none reachable remotely on a configured unit.
2. Grants access to *provisioning/recovery wizards*, not a direct root shell or admin session with pre-set creds.
3. The enabling conditions are computed from legitimate state (`startup-config` presence, RESET button, recovery flag file), not a hardcoded secret username/password/knock.
4. No hidden hardcoded credential or magic value is involved; the "bypass" is the intended empty-credential-store UX.

**Attacker prerequisites to abuse it:** physical access (hold RESET → factory default, or trigger password-recovery). An attacker with that level of access already owns the device; this is the same trust boundary as any appliance factory-reset. No network-only path exists on a configured unit.

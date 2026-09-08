# ipxe_iscsi_mgr — design plan

Manage iSCSI boot volumes for netbooted hosts. A Cloudflare Worker is the
control plane and the iPXE config server; a local CLI is the only thing that
understands ZFS and LIO, and it is the only thing that validates.

Status: **M0-M3 built, deployed and validated on real hardware** (2026-08-25,
keyboard-free hardening 2026-08-26).
M4 (validating CLI) is built bar declarative `sync`; M5 (general pool) is
designed, not implemented.
See section 10 for what is live and section 11 for what the hardware corrected.

## 1. Shape of the system

```
  ZFS/iSCSI host (sea69-hv-puppy)        laptop / admin                Cloudflare
  ------------------------------         --------------                ----------
  rpool/iscsi0  ->  LIO target      <--  ipxectl validate         push  Worker (Hono)
  rpool/iscsi1  ->  LIO target           ipxectl sync            ----->   D1  (hosts, volumes,
                                                                          assignments, secrets)
                                                                        KV  (boot-secret cache)
                          ^                                               |
                          |  iSCSI session                                | HTTPS + secret
                          |                                               v
                      +---------------------------- booting host (iPXE) --+
                      |  ipxe.efi w/ embedded script -> chain /v1/boot     |
                      |  <- iPXE script w/ initiator-iqn + target-uri      |
                      |  sanhook -> sanboot loop over EFI filenames        |
                      +----------------------------------------------------+
```

Trust split, stated once because it drives everything else:

- **The Worker never validates.** It does not know what a zvol is, cannot reach
  the iSCSI portal, and will happily store a target IQN that does not exist. It
  stores rows and renders templates.
- **The CLI is the gate.** `ipxectl` runs where it can see ZFS and `targetcli`,
  checks that the mapping is real, and only then pushes. Every write path to the
  Worker goes through it. A `--force` flag exists for when you know better, and
  it gets recorded in the audit log.

## 2. What we are importing (the baseline)

### 2.1 The hand-rolled iSCSI config

This is the config we have today and the thing the generator must reproduce
byte-for-byte for the `debian` host:

```ipxe
set boot-server http://10.36.75.7
set initiator-iqn iqn.2026-02.local.client:debian
set target-uri iscsi:10.36.75.5:tcp:3260:0:iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.80c84b6cc10c
```

Decomposed into the data model:

| Field | Value | Lives in |
| --- | --- | --- |
| initiator IQN | `iqn.2026-02.local.client:debian` | `hosts.initiator_iqn` |
| portal host | `10.36.75.5` | `volumes.portal_host` |
| portal port | `3260` | `volumes.portal_port` |
| LUN | `0` | `volumes.lun` |
| target IQN | `iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.80c84b6cc10c` | `volumes.target_iqn` |
| backing store | `/dev/zvol/rpool/iscsi0` | `volumes.zvol_path` |
| boot server | `http://10.36.75.7` | `settings.boot_server` (kept, unused by the SAN path) |

The `target-uri` form is iPXE's: `iscsi:<host>:<protocol>:<port>:<lun>:<target-iqn>`.
Note the target IQN itself contains colons — the generator must place it last and
never try to split on `:` when parsing.

The initiator IQN's `iqn.2026-02.local.client:` prefix becomes a per-deployment
setting so new hosts get `iqn.2026-02.local.client:<host-name>` by default. The
date-code is frozen at whatever the deployment was created with; it is a naming
authority stamp, not a timestamp, and must never be regenerated for an existing
host or LIO ACLs break.

### 2.2 The EFI multi-filename boot chain

Oracle's OCI iPXE example is the reference for the pattern: `sanhook` the target,
then `sanboot` with an explicit `--filename`, falling through on failure. Oracle
hardcodes a single `\EFI\BOOT\grubx64.efi`; we want the sweep.

iPXE has no arrays or loops over string lists, so the generator emits an
unrolled `goto` chain:

```ipxe
#!ipxe
# keep the session alive across the handoff -- without this iPXE tears down
# the iSCSI session before the OS gets a chance to adopt it
set keep-san 1

set initiator-iqn iqn.2026-02.local.client:debian
set target-uri iscsi:10.36.75.5:tcp:3260:0:iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.80c84b6cc10c

# keep-san makes sanunhook a no-op, so clear it around the unhook (11.9)
set keep-san 0
sanunhook --drive 0x80 ||
set keep-san 1
sanhook --drive 0x80 ${target-uri} || goto failmenu

:try0
sanboot --keep --drive 0x80 --filename \EFI\BOOT\BOOTX64.EFI || goto try1
:try1
sanboot --keep --drive 0x80 --filename \EFI\debian\grubx64.efi || goto try2
:try2
sanboot --keep --drive 0x80 --filename \EFI\debian\shimx64.efi || goto try3
:try3
sanboot --keep --drive 0x80 --filename \EFI\ubuntu\grubx64.efi || goto try4
:try4
sanboot --keep --drive 0x80 --filename \EFI\redhat\shimx64.efi || goto try5
:try5
sanboot --keep --drive 0x80 --filename \EFI\Microsoft\Boot\bootmgfw.efi || goto trydefault
:trydefault
# no --filename: let iPXE pick the UEFI default from the ESP
sanboot --keep --drive 0x80 || goto failmenu

:failmenu
echo Boot failed for ${target-name}
chain --autofree ${mgr-url}/v1/report?status=sanfail&... ||
# Not a dead end and not a blind reboot loop: a menu listing every image
# allocated to this host, plus netboot.xyz for installing onto one. Section 13.
menu ipxe_iscsi_mgr -- boot failed on <host>
item vol0      Boot image: <name> [pinned]
item nbxyz     netboot.xyz installer -- installs onto the selected image
...
item rebootnow Reboot and retry
# 600s and a safe default, so a keyboard-less machine still progresses (11.6)
choose --default rebootnow --timeout 600000 selected && goto ${selected} ||
goto rebootnow
```

Four correctness points that are easy to get wrong:

- **Do not pass `--no-describe`.** `--no-describe` suppresses iBFT publication.
  iBFT is exactly how the OS initramfs discovers the already-open iSCSI session
  and keeps the root filesystem alive after iPXE exits. `--no-describe` belongs
  on *local* disk boots, not this one. Oracle's example uses it on a `sanhook`
  of a non-boot drive, which is a different case.
- **`--keep` on every attempt.** Without it a failed `sanboot` unhooks the drive
  and every subsequent attempt in the chain fails for the wrong reason.
- **No dead end may require a keypress.** See 11.6 — this stranded a real
  machine.
- **`keep-san` makes `sanunhook` a no-op.** See 11.9 — every retry path needs
  the unhook wrapped in `set keep-san 0` / `set keep-san 1`.

The candidate list is per-`boot_profile` and architecture-aware — `BOOTX64.EFI`
on x86_64, `BOOTAA64.EFI` on arm64, keyed off iPXE's `${buildarch}`. Profiles
ship as presets (`generic-efi`, `debian`, `ubuntu`, `rhel`, `windows`,
`bios-legacy`) and are freely editable.

## 3. Host identity and registration

### 3.1 What iPXE can actually tell us

The embedded script collects these before chaining. This is the ceiling on what
"CPU / RAM / serial / MAC information" can mean at boot time:

| Datum | iPXE setting | Notes |
| --- | --- | --- |
| MAC | `${mac:hexhyp}` | current netdev; `hexhyp` avoids colons in the URI |
| SMBIOS UUID | `${uuid}` | strongest identity key; **never** `:uristring`, see 11.3 |
| System serial | `${serial}` | |
| Asset tag | `${asset}` | |
| Manufacturer / product | `${manufacturer}` / `${product}` | |
| RAM (MB) | `${memsize}` | **BIOS only — empty under UEFI**, see 11.4 |
| Platform | `${platform}` | `efi` or `pcbios` |
| Arch | `${buildarch}` | drives the EFI candidate list |
| CPU model | `${smbios/4.0x10.0}` | type 4 Processor Version; QEMU lies here, see 11.5 |

**All of the above has now been verified against iPXE 1.21.1, and two rows were
wrong — see section 11.** `${uuid}` must be used bare (`:uristring` corrupts
it), and `${memsize}` is empty under UEFI, so RAM comes from SMBIOS type 17/16
instead and is recorded as approximate.

Identity matching is ordered: SMBIOS UUID, then serial, then any known MAC. MAC
alone is the weakest key (NICs move, and multi-NIC boxes present whichever port
booted), so `host_macs` is a separate table and a host may own several.

### 3.2 Boot flow

```
  GET /v1/boot?uuid=..&mac=..&serial=..&arch=..&mem=..&cpu=..
        |
        +-- auth: boot secret -> 401 + iPXE script that echoes the reason
        |
        +-- match host?
              |
              no  -> INSERT host state='pending', record all facts
              |      return "pending approval" script: echo, sleep, reboot/retry
              |
              yes -> state='disabled' -> return refusal script
                     state='pending'  -> return pending script
                     state='approved' -> resolve assignment
                                          |
                                          +-- pinned volume -> render boot script
                                          +-- general pool  -> lease a volume (see 4.2)
                                          +-- nothing free  -> return no-volume script
```

Registration is a side effect of `/v1/boot`, not a separate call the host has to
make — an unknown machine that netboots once shows up in `ipxectl host pending`
with its full inventory. `POST /v1/register` exists as an explicit endpoint for
seeding hosts from the CLI without booting them.

Response is always `text/plain` and always a valid iPXE script, including on
errors. An HTTP error body that isn't a script just drops the machine to a
useless prompt.

## 4. Volume pools

### 4.1 Pinned

`assignments.host_id` set. One host, one volume, stable across reboots. This is
the normal case and matches the imported `debian` config.

### 4.2 General — the concurrency problem

"Generally available to all hosts" cannot mean "hand the same block device to
every host that asks." Two hosts with a read-write iSCSI session on one zvol
will destroy the filesystem, quickly and without warning. So `pool='general'`
splits into two modes and the schema forces the choice:

- **`exclusive` (default).** The volume is claimable. On boot the Worker does a
  conditional D1 update — `UPDATE assignments SET lease_host_id=?, lease_expires_at=?
  WHERE volume_id=? AND (lease_host_id IS NULL OR lease_expires_at < ?)` — and
  only renders a boot script if it wrote a row. A lease renews on each boot and
  can be promoted to a pin (`ipxectl assign --promote`) or released
  (`ipxectl release`). Uses: a pool of identical scratch/installer volumes where
  any host takes the next free one.
- **`shared-ro`.** Many hosts, one volume, read-only. Legitimate for a golden
  rescue image. The CLI must verify LIO has `attrib.readonly=1` on the mapped
  LUN before it will accept the mode — an unverified shared-ro volume is the
  same corruption bug wearing a hat.

Assignment resolution order for an approved host: pinned → held lease →
claimable general volume by `priority` → nothing.

## 5. Data model (D1)

```sql
hosts(id PK, name UNIQUE, state, -- pending | approved | disabled
      smbios_uuid, serial, asset_tag, manufacturer, product,
      cpu_model, mem_mb, arch, platform,
      initiator_iqn UNIQUE, boot_profile_id,
      first_seen, last_seen, boot_count, notes)

host_macs(mac PK, host_id, first_seen)

volumes(id PK, name UNIQUE, provider DEFAULT 'zfs',
        zvol_path,                    -- /dev/zvol/rpool/iscsi0
        portal_host, portal_port, lun, target_iqn,
        size_bytes, state, notes, created_at, updated_at)

assignments(id PK, volume_id UNIQUE, mode,   -- pinned | general-exclusive | shared-ro
            host_id,                          -- NULL unless pinned
            lease_host_id, lease_expires_at, priority)

boot_profiles(id PK, name, arch, efi_candidates JSON, extra_ipxe)

secrets(id PK, kind, hash, label, created_at, last_used_at, disabled_at)

boot_events(id PK, ts, host_id, volume_id, outcome, detail JSON)

audit_log(id PK, ts, actor, action, subject, detail JSON, forced BOOL)
```

`volumes.zvol_path` is `UNIQUE` and `assignments.volume_id` is `UNIQUE` — one
zvol, one target, one assignment row. Double-assignment is prevented at the
schema level, not by application logic, because the Worker does not validate.

## 6. Auth

Baseline as specified: a fixed secret in the database, hashed at rest, presented
by the booting host.

Transport: HTTP Basic via URI userinfo (`https://boot:<secret>@mgr.example/v1/boot?...`),
which iPXE supports natively. Prefer it over a query parameter — the secret stays
out of request paths and therefore out of most logging surfaces. The secret is
compiled into `ipxe.efi` as an embedded script, so the binary is a credential:
treat a lost `ipxe.efi` as a compromised secret.

The Worker stores `sha256(secret)` and compares in constant time; KV caches the
hash so the hot boot path avoids a D1 read.

**~~HTTPS gotcha~~ — this prediction was wrong, see section 11.** Stock iPXE
1.21.1 fetched `https://…workers.dev` with no rebuild and no custom trust
anchor. No `TRUST=` build is needed. The failure that *looked* like a cert
problem was a missing `WWW-Authenticate` header; see section 11.2.

Upgrade path once the baseline works, not needed for v1: a shared *enrollment*
secret used only by unknown hosts, and a per-host secret issued at approval, so
one leaked binary doesn't authorize every machine. Admin endpoints sit behind
Cloudflare Access rather than the boot secret from day one — different audience,
different credential.

## 7. Repo layout

npm workspaces, TypeScript throughout, matching `discordbot_housebot`'s stack
(Wrangler + D1 + vitest) so the tooling is already familiar.

```
aislop/ipxe_iscsi_mgr/
  PLAN.md
  README.md
  package.json                  # workspaces root
  packages/
    shared/                     # zod schemas, types, and the iPXE generator
      src/schema.ts
      src/ipxe.ts               # renderBootScript(host, volume, profile)
    worker/
      src/index.ts              # Hono router
      src/routes/{boot,hosts,volumes,assignments}.ts
      migrations/
      wrangler.jsonc
    cli/
      src/index.ts              # ipxectl
      src/validate/{zfs,lio,portal}.ts
```

The generator lives in `shared` deliberately: the Worker renders it for real and
the CLI renders it for `ipxectl render <host>` preview and for golden-file tests.
One implementation, so a preview can never disagree with what a machine gets.

## 8. CLI surface

```
ipxectl host    list | pending | show <h> | approve <h> | rename <h> <name>
                set-iqn <h> <iqn> | set-profile <h> <p> | disable <h>
ipxectl volume  list | show <v> | add <name> --zvol <path> --target-iqn <iqn>
                     --portal <host:port> --lun <n>
                rm <v>
ipxectl assign  <v> --host <h> | --general [--priority N] | --shared-ro
ipxectl release <v>          # drop a general lease
ipxectl profile list | show <p> | add <name> --arch <a> --candidates <f1,f2,...>
ipxectl render  <h>          # print the exact script that host would receive
ipxectl check                # validate local ZFS/LIO state, no network writes
ipxectl sync                 # check, then push to the Worker
```

`check` is the whole value proposition of the CLI. It runs on the storage host
and verifies:

1. `zfs list -t volume` — the zvol backing every volume exists, and its size
   matches `size_bytes`.
2. `targetcli ls` / `/sys/kernel/config/target/` — a LIO target exists with the
   recorded target IQN, and a backstore points at exactly that zvol path.
3. The recorded LUN number is actually mapped in that target's TPG.
4. For every pinned assignment, the target's ACL contains the host's
   `initiator_iqn` with the LUN mapped into it. **This is the failure mode that
   looks like a boot bug**: config is perfect everywhere, LIO's per-initiator ACL
   is missing, and the host gets an empty LUN list.
5. `shared-ro` volumes have `attrib.readonly=1` on the mapped LUN.
6. Portal reachability — TCP connect to `portal_host:portal_port`.
7. IQN syntax per RFC 3720 for both target and initiator.
8. No zvol is backing two volumes, and no volume has two assignments.

`check` exits non-zero on any failure; `sync` refuses to push unless `check`
passes or `--force` is given, and `--force` writes a flagged audit row.

## 9. Milestones

- **M0 — skeleton.** *(done)* Workspaces, tsconfig, wrangler config, D1 created, migrations
  run, `GET /v1/health`.
- **M1 — import the baseline.** *(done — 30 tests green)* Schema + `shared/src/ipxe.ts`. Golden test: given
  the `debian` host and `iscsi0` volume rows, the generator emits exactly the
  section 2.1 config. This is the "we have parity with hand-rolling" gate.
- **M2 — boot path.** *(done — VM 900 registered and booted)* `/v1/boot` with auth, identity matching, autoregistration
  of unknown hosts, pinned assignment resolution, pending/refusal/no-volume
  scripts. Build a `TRUST=`-patched `ipxe.efi` and boot one real machine.
- **M3 — EFI candidate sweep.** *(done — verified a fallback actually fires)* Boot profiles, the unrolled `goto` chain,
  arch-aware defaults. Verify a fallback actually fires on a machine whose
  bootloader is not at `\EFI\BOOT\BOOTX64.EFI`.
- **M4 — CLI.** *(done, except declarative `sync`)* `ipxectl` implements `check`
  (verified against live infrastructure by deliberately deleting an ACL, a zvol
  reference and a target, and confirming each was caught with a working fix
  command), plus the host/volume/assignment/profile verbs, `render`, `events`
  and `gen-chain`. Mutating commands gate on `check` unless `--force`.
  A declarative `sync` from a local state file is deliberately **not** built:
  there is no local desired-state file yet, and faking one would be worse than
  the targeted pre-flight validation that is there.
- **M5 — general pool.** Leases, the conditional-update claim, `shared-ro` with
  its readonly verification. Last because it is the easiest thing to get subtly
  and destructively wrong.

Admin auth via Cloudflare Access lands with M4, when there is finally an admin
API worth protecting.

## 10. Status

M0–M3 are **built, deployed and validated on real hardware** (2026-08-25).
M4 is built bar declarative `sync`; M5 (general pool) is designed, not built.

- Worker: `https://ipxe-iscsi-mgr.butt.workers.dev`, D1 `ipxe-iscsi-mgr`.
- Netboot server `sea69-bootbox` (LXC 103) routes **only** the test MAC to the
  manager; every other machine still gets the original menu byte-for-byte.
  Originals are in `/srv/tftp/.bak-ipxe-mgr/`.
- Proven end to end on Proxmox VM 900: unknown machine → auto-registered as
  `pending` with its inventory → approved → pinned to a volume → iPXE attached
  the iSCSI target (LIO session `LOGGED_IN` under the generated initiator IQN)
  → the EFI candidate sweep skipped two missing bootloaders and booted the
  third. 30 unit tests green.

## 11. What the hardware changed

Six things were wrong or unknown in the original plan. All were invisible to
curl testing and only appeared against real iPXE on real machines.

**11.1 `autoexec.ipxe` is vestigial here.** `ipxe.efi` on the bootbox has an
*embedded* script that chains directly to `http://10.36.75.7/boot.ipxe.cfg`, so
editing `autoexec.ipxe` does nothing. `boot.ipxe.cfg` is the real entry point.
Anything that assumes the TFTP `autoexec.ipxe` path is the hook is wrong on this
network.

**11.2 The HTTPS/CA concern was wrong; the bug was mine.** iPXE reported
`Permission denied (https://ipxe.org/020c618f)`, which reads like a TLS failure.
It is not — `020c618f` is **HTTP 401 Unauthorized**. TLS was fine: iPXE 1.21.1
validated the Google Trust Services chain unaided. The real cause is that iPXE
does **not** send URI credentials preemptively; it waits for a challenge. My 401
had no `WWW-Authenticate` header, so iPXE gave up instead of retrying with
Basic. One header fixed it. Budget the iPXE rebuild only if a future deployment
uses a genuinely untrusted root.

**11.3 `${uuid:uristring}` corrupts the UUID.** `uuid` is already a formatted
string; applying `:uristring` re-reads the underlying 16 raw bytes and yields
mojibake (`\x00\x00\x00�FFFF90…`). Use bare `${uuid}` — it is URI-safe
already. The `normalizeUuid` guard caught it, so nothing corrupt reached the DB,
but the host lost its strongest identity key and matched only on MAC.

**11.4 `${memsize}` is empty under UEFI.** It is a BIOS-era setting. Since
essentially every target is UEFI, RAM now comes from SMBIOS: type 17 offset 0xC
(DIMM 0 size, little-endian colon-hex WORD of MB) then type 16 offset 0x7
(maximum capacity, DWORD of KB). Both are *approximations* — one DIMM, and what
the board could hold — so `hosts.mem_source` records which was used. An
approximation must never be mistaken for a measured total.

**11.5 The CPU offset was right; QEMU is the liar.** `${smbios/4.0x10.0}` is
genuinely Processor Version per the SMBIOS spec, resolving the plan's open
question. QEMU just fills that field with its machine type (`pc-q35-11.0`). Real
hardware puts the CPU model there. `cpu_vendor` is captured separately from type
4 offset 0x07.

**11.6 A physical machine may have no working keyboard at all.** Reported from
hardware 2026-08-26. iPXE can come up with no usable input device — USB HID not
brought up by the firmware, or a BMC/KVM that never presents one. Every script
that dead-ends in a bare `prompt` or a trailing `shell` strands such a machine
**forever**, with no retry, no reboot and no telemetry: the worst possible
failure mode, because it looks like a hang rather than an error.

Four of the generated scripts had this bug (`no-volume`, `refusal`, the
`:sanfail` block, and the chain script's `:shellout`), and so did the legacy
menu — `choose` with neither `--timeout` nor `--default` waits indefinitely.

The rule now enforced by tests: **every generated script must be able to run to
completion with zero keypresses.** Every dead end is a bounded wait ending in
`reboot`, with ESC as an escape hatch for whoever does have a keyboard.
`prompt --timeout` is safe with no input device (it simply expires); a bare
`prompt` is not and must never be emitted. Backoff is 30 s for recoverable
states (pending approval) and 300 s for hard failures, so a broken machine
retries without hammering the network.

This turns approval into a hands-off operation: verified on hardware by leaving
the test VM powered on, flipping it `pending` → `approved` in the Worker, and
watching it pick the change up on its own next retry and boot — no console, no
keyboard, no power cycle.

**11.7 Why the USB keyboard is dead under EFI (root-caused 2026-08-26).** Not a
build mistake — an upstream iPXE design decision that this setup falls foul of.

`config/usb.h` enables `USB_HCD_XHCI`, `USB_HCD_EHCI` and `USB_HCD_UHCI` by
default, so iPXE binds the USB host controllers natively and **disconnects the
firmware's UEFI USB drivers** in the process. The same file then does
`#undef USB_KEYBOARD` on EFI platforms specifically, commented *"Use built-in
EFI keyboard driver"*. That assumption is what breaks: the built-in EFI keyboard
driver is precisely the thing iPXE just disconnected, and there is no iPXE
keyboard driver left to replace it.

Confirmed against the deployed binary: `xhci`, `xhci-pch`, `xhci-skylake`,
`ehci`, `uhci`, `usbblk`, `usbscan` are all present; there is **no** `usbkbd`,
`hid` or `keyboard` string anywhere in it.

This also explains why the VM never reproduced it. QEMU `q35` gives the guest a
PS/2 keyboard via i8042, which iPXE's USB stack never touches — an injected ESC
at the refusal prompt dropped straight to an `iPXE>` shell. Physical machines
use USB HID, so they lose the keyboard. **A VM cannot reproduce this class of
bug; it needs metal or an emulated USB keyboard.**

Fixes, in order of preference:

1. Rebuild with `echo '#define USB_KEYBOARD' > config/local/usb.h`. The local
   config is included *after* the EFI `#undef`, so it wins. iPXE then drives the
   USB keyboard itself. Keeps USB-NIC support.
2. Disable `USB_HCD_XHCI`/`EHCI`/`UHCI` in `config/local/usb.h` if USB NICs are
   irrelevant (they are here — booting is over an onboard NIC via SNP). iPXE
   then never touches USB and the firmware keeps the keyboard. Simplest.
3. `#define USB_HCD_USBIO` — drives USB through the firmware's `EFI_USB_IO`
   protocol rather than natively. iPXE's own comment calls it "very slow".

**Done 2026-08-26.** Rebuilt from upstream master and deployed: iPXE
**1.21.1+ -> 2.0.0+** with `config/local/usb.h` containing `#define
USB_KEYBOARD`. Confirmed linked, not merely compiled: `usbkbd_console`,
`usbkbd_driver`, `usb_keyboards` and `obj_usbkbd` are all present in the link
map, and the image grew 12,800 bytes.

A caveat on the evidence trail: the original "no `usbkbd` string in the binary"
probe was weak -- iPXE emits no bare driver-name string, so that grep returns
empty either way. The real proof is the source (`#undef USB_KEYBOARD` under
`PLATFORM_efi`, verified in the tree at `config/usb.h:28`) plus the link map and
size delta.

Regression-tested on the real PXE path, not just built: VM 900 booted the new
binary over TFTP, chained through the embedded script to the manager and booted
from iSCSI, and ESC still drops to a shell (console input not regressed by the
major version bump).

`/srv/tftp` now holds three binaries:

| file | build | purpose |
| --- | --- | --- |
| `ipxe.efi` | 2.0.0 + `USB_KEYBOARD` | deployed |
| `ipxe-2.0.0-usbkbd.efi` | identical to the above | named copy |
| `ipxe-2.0.0-nousb.efi` | 2.0.0, native HCDs undef'd | fallback: firmware keeps USB entirely |
| `.bak-ipxe-mgr/ipxe.efi.1.21.1` | the original | rollback |

**The USB keyboard fix itself is unverified** -- it cannot be tested in a QEMU
`q35` VM, which is PS/2 only. It needs a physical machine. If iPXE's own HID
driver does not like the keyboard, swap in `ipxe-2.0.0-nousb.efi`, which removes
iPXE's USB stack altogether so the firmware never loses the controller.

**11.8 The keyboard-free menu patch created a data-safety regression (found and
fixed 2026-08-26).** Making the legacy menu keyboard-free (11.6) meant giving
`choose` a `--default`, and I picked `boot_iscsi` because it was the obvious
"normal" action. That was wrong.

`boot_iscsi` in the legacy menu hardcodes `initiator-iqn
iqn.2026-02.local.client:debian` and the `iscsi0` target. Any machine landing on
that menu therefore *impersonates the debian host* and mounts its root
filesystem read-write. Before the patch the menu waited forever for a keypress,
so this could never happen unattended; after it, **any** unrecognised machine
would silently take the production volume 15 seconds after power-on.

Caught by testing a non-opt-in MAC: the VM fell through to the legacy menu and
booted the real Debian install off `rpool/iscsi0`. Post-mortem showed only one
ISID (`0x00023d000001`) ever present for that initiator and the address went
dead when the VM stopped, so the session was the VM's own and there was never a
second concurrent writer — no dual-write corruption. The volume did take an
unclean shutdown (`qm stop`), so expect an ext4 journal replay on its next
mount.

Fixed by adding a `:retry` item and making it the unattended default: it checks
in, explains why it is not booting, and reboots to retry. `boot_iscsi` remains
available as an explicit choice for a human at a keyboard. Verified: a
non-opt-in machine now checks in and opens **no** session on `iscsi0`.

The general lesson, which the design already stated and I violated anyway: a
volume is a single-writer resource, so "boot the default volume" is never a safe
*automatic* action for an unidentified machine. Only the manager knows which
host owns which volume.

**11.9 `keep-san` silently disables `sanunhook` (found 2026-08-26).** Adding a
boot-failure menu meant drive 0x80 had to be re-hookable, to retry a different
image or to attach one for netboot.xyz. `sanunhook --drive 0x80` looked
sufficient. It is not.

`sanunhook` passes `URIBOOT_NO_SAN_DESCRIBE | URIBOOT_NO_SAN_BOOT` but *not*
`URIBOOT_NO_SAN_UNHOOK`, so it reaches `uriboot()`'s cleanup in
`usr/autoboot.c`, which checks `keep-san` first: if set, it prints "Preserving
SAN device" and skips the unhook entirely. The boot script always sets
`keep-san` (11.x: the OS needs the session across handoff), so the unhook was a
guaranteed no-op and every retry path failed with
`Could not open SAN device: Error 0x03232094`.

Reproduced and fixed on hardware in one pass -- a diagnostic script ran the
naive sequence (fails) and the wrapped sequence (succeeds) back to back:

```
set keep-san 0
sanunhook --drive 0x80 ||
set keep-san 1
```

The general lesson: iPXE settings have action at a distance. `keep-san` reads as
"preserve the session across boot handoff" but actually changes the behaviour of
an unrelated-looking command.

**11.10 Chainloading `netboot.xyz.efi` hangs on metal (found 2026-08-26).** The
first cut of the netboot.xyz menu item chained `netboot.xyz.efi`. On the QEMU
test VM that appeared to load; on the UM350 it hung at `Available
interfaces...` and never progressed.

The `.efi` is netboot.xyz's *firmware* entry point. Chainloading it from an
already-running iPXE starts a **second iPXE instance**, which then tries to
claim a NIC the first instance is still holding -- with, in our case, an open
iSCSI session on it. The second instance stalls enumerating interfaces.

netboot.xyz's documented method from an existing iPXE is to chain the script,
not the binary: `chain --autofree https://boot.netboot.xyz/menu.ipxe`. That runs
inside the current instance, so there is no NIC contention and the SAN hook and
its iBFT survive. Verified on hardware: the netboot.xyz v3.x menu renders, and
reports `next-server: 10.36.75.7` -- i.e. it inherited our DHCP context rather
than re-initialising the stack.

Two consequences worth recording:

- The local mirror was deleted. `menu.ipxe` resolves `boot.cfg` and its
  sub-menus by *relative* URL, so a mirrored copy 404s against the netboot
  server. And netboot.xyz streams kernels from upstream anyway, so mirroring the
  bootstrap never removed the internet dependency it was meant to remove.
- While the netboot.xyz menu is open, iPXE stops answering iSCSI NOP-In
  keepalives and LIO drops the session. Harmless: iBFT is an ACPI description,
  not the live connection, and the installer opens its own session from it.

The general lesson: "chainload X" is not one operation. Chaining a *script*
extends the current iPXE; chaining a *binary* replaces it, discarding the
network and SAN state the script had carefully set up.

**Follow-on: surface netboot.xyz's tree in our own menu.** Even with the script
chainload working, netboot.xyz's own menu is three levels deep (top -> Linux ->
distro), and the machines that need an installer are the ones whose keyboard may
not work. netboot.xyz's distro scripts live at the root of `boot.netboot.xyz`
and are self-contained given `${arch}` and `${space}`, so our menu now jumps
straight to a leaf -- `NixOS installer` lands in NixOS's version menu with two
menu levels skipped. Verified on hardware: the NixOS menu renders as
`NixOS - x86_64` with correct item padding, confirming the environment prep.

Pre-setting netboot.xyz's `${menu}` variable looks like it should do this and
does not: `menu.ipxe` then runs `choose --timeout 0 --default ${menu}`, and in
iPXE a timeout of 0 means *wait forever*, not *skip*. Chaining the leaf script
directly is what actually works.

## 12. Open questions

1. **Rollout beyond the test MAC.** `boot.ipxe.cfg` currently routes one MAC to
   the manager. Flipping the default (manager first, legacy menu as fallback) is
   a one-line change, but should wait until the `debian` host's SMBIOS identity
   is recorded — it is seeded by name only right now, so on first contact it
   would fail identity matching and re-register as a new pending host. Capture
   its UUID/serial/MAC first, either from the machine or from one deliberate
   pending registration that then gets merged.
2. **Does the general pool need to exist at all?** If in practice every host gets
   a pinned volume, M5 is a lot of lease machinery and corruption risk for
   nothing. Worth deciding before building it rather than after.
3. **Secret rotation** — the schema supports multiple live secrets so rotation is
   possible. With the secret living in `mgr.ipxe` on the netboot server rather
   than baked into a binary, rotation is now just rewriting that file, which is
   much cheaper than the plan assumed. Per-host secrets remain the real answer.
4. **`mgr.ipxe` holds the boot secret in cleartext** on the netboot server and
   is served over plain HTTP on the LAN. That is the same exposure the existing
   setup already has, but it is worth an explicit decision rather than drifting
   into it. Embedding the script in a rebuilt `ipxe.efi` moves the secret from a
   readable file into a binary — obscurity, not secrecy. Per-host secrets or
   CHAP on the LIO target are the real fixes.
5. **`boot-server`** (`http://10.36.75.7`) is carried through as a setting but
   nothing in the SAN boot path consumes it. Is it needed for a kernel/initrd
   fallback path, or is it vestigial and droppable?
6. **Multi-NIC identity.** `${mac}` and `${net0/mac}` both resolved correctly on
   the single-NIC test VM, but a genuinely multi-NIC machine is still untested.
   SMBIOS UUID stays the primary key until that is checked.

## 13. Inventory check-in

Every machine reports itself at startup, whether or not it goes on to boot via
the manager. `/srv/tftp/checkin.ipxe` is chained from the top of
`boot.ipxe.cfg`, before the opt-in branch, so machines that fall through to the
legacy menu still appear in the inventory.

Design points that are load-bearing:

- **Separate endpoint from `/v1/boot`.** `/v1/boot` returns boot semantics --
  a pending host gets a reboot-retry loop. Reusing it for a check-in would turn
  every unapproved machine into a reboot loop. `/v1/checkin` only ever returns a
  non-terminal acknowledgement.
- **The check-in script is non-terminal.** No `boot`, `reboot`, `shell` or
  `prompt`: it hands control back to whatever chained it. This is the exact
  inverse of the 11.6 keyboard-free rule, and tests assert it both ways.
- **Trailing `||` at every call site**, so a manager outage cannot block booting.
  The cost is an HTTPS round trip on every boot, and an iPXE timeout's worth of
  delay when the worker is unreachable.
- **One shared facts query** (`BOOT_FACTS_QUERY`) for both the boot chain and
  the check-in. If they drift, a host's identity depends on which path it took
  and it re-registers as a duplicate.
- **A check-in is not a boot.** It refreshes inventory and `last_seen` but must
  not increment `boot_count`.
- `refreshHost` uses `COALESCE` on every field, so a fact that came back empty
  this boot never wipes a good stored value. It also learns the SMBIOS UUID if
  the host was matched on a weaker key -- in a separate statement, because
  `smbios_uuid` is uniquely indexed and a collision must not roll back the
  inventory refresh.

Menu item 4, "Refresh config + force check-in", sets `checkin-force 1`, re-runs
the check-in and reloads the config. The force flag travels as an iPXE variable
so one file serves both callers.

## 14. The menu, the 3s interrupt, and the netboot.xyz install path

The menu is reached either by pressing a key during a **3-second** window before
the automatic boot, or automatically when a boot fails. Two design points that
are easy to get wrong:

- The primary image is selected *before* the interrupt prompt. Otherwise
  entering the menu early leaves `${target-uri}` unset and every menu action
  that attaches a LUN hooks an empty target.
- `:failmenu` (says "boot failed", files a `/v1/report`) is separate from
  `:menu` (the same menu, reached deliberately). Merging them means an
  interrupt -- or backing out of a submenu -- files a failure report for a
  failure that never happened. Submenu backs were doing exactly that before
  the split.

A failed boot lands on a manager-generated menu rather than a blind reboot loop.
It lists every image allocated to the host (`listHostVolumes`: the pinned one,
any pooled volume currently leased to it, and any shared-ro image), plus
netboot.xyz, reload, force-check-in, memtest and a shell. Default `rebootnow`
after **600 s**, so a keyboard-less machine still progresses and picks up
whatever changed in the manager.

**The netboot.xyz item is the install path.** It `sanhook`s the selected image
*before* chainloading netboot.xyz, so the LUN is published via iBFT and an
installer booted through netboot.xyz sees it as an ordinary disk and can install
onto it. That closes the "how does an empty zvol get an OS" gap in the runbook
without any manual `iscsiadm`.

netboot.xyz is mirrored on the netboot server and served over HTTP (upstream
HTTPS as fallback), so booting does not depend on the internet or on iPXE's EFI
certificate handling.

Two structural details:

- The candidate sweep is written **once**, under `:attach`. Each image is a
  `:volN` entry point that sets `target-name`/`target-uri` and jumps there, so
  adding images does not multiply the script.
- `Select iSCSI image` sets those same two variables and returns to the menu,
  so it retargets both the boot attempt and netboot.xyz. It is
  iPXE-session-local and deliberately does **not** change the pinned assignment
  in the manager -- that is `ipxectl assign`.

Verified on hardware: the menu renders with the countdown, `Re-fetch
configuration from the manager` recovers a machine without rebooting, and
netboot.xyz chainloads from the local mirror with the SAN device registered.
**Correction (11.10):** the first implementation chained `netboot.xyz.efi` and
hung on real hardware. It now chains `menu.ipxe`, which runs inside the current
iPXE instance.

What is **not** yet verified end to end is whether an installer booted through
netboot.xyz actually sees the LUN via iBFT -- that needs a real install run.

## Sources

- [Oracle OCI CLI iPXE script example](https://github.com/oracle/oci-cli/blob/master/tests/resources/ipxe_script_example.txt)
- [iPXE `sanboot` / `sanhook` documentation](https://ipxe.org/cmd/sanboot)

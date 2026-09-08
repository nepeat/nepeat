# ipxe_iscsi_mgr

Manages iSCSI boot volumes for netbooted machines. A Cloudflare Worker holds the
data and serves the iPXE config; the machines register themselves the first time
they netboot.

`PLAN.md` is the design doc — read section 11 first, it lists the six things
real hardware corrected about the original plan.

## Live deployment

| | |
| --- | --- |
| Worker | `https://ipxe-iscsi-mgr.butt.workers.dev` |
| D1 | `ipxe-iscsi-mgr` (`e202151b-e096-449b-a3a4-83b8bd2b4f36`) |
| KV | `SECRET_CACHE` (boot-secret hash cache) |
| Netboot server | `sea69-bootbox` = LXC 103 on `sea69-hv-puppy`, `10.36.75.7` |
| iSCSI target host | `sea69-hv-puppy`, `10.36.75.5` |

Credentials are in `.dev.vars` (gitignored). `ADMIN_TOKEN` is also a Worker
secret; boot secrets live hashed in D1 and in `/srv/tftp/mgr.ipxe`.

`.dev.vars` keys: `ADMIN_TOKEN`, `BOOT_SECRET`, `STORAGE_SSH`
(`root@10.36.75.5` — where `ipxectl check` reads ZFS/LIO from). `MGR_URL` falls
back to `vars.MGR_URL` in `wrangler.jsonc`, so it lives in one place.

## How a machine boots

```
DHCP -> ipxe.efi (TFTP)
     -> embedded script: chain http://10.36.75.7/boot.ipxe.cfg
     -> MAC matches the opt-in list? -> mgr.ipxe -> Worker /v1/boot
                                     -> otherwise the original menu, untouched
```

`autoexec.ipxe` on the netboot server is **vestigial** — `ipxe.efi` has an
embedded script that chains straight to `boot.ipxe.cfg`. Editing `autoexec.ipxe`
does nothing. This surprises everyone once.

The Worker replies with a script that `sanhook`s the target once and then walks
the EFI bootloader candidates against the already-hooked drive, so a machine
whose bootloader is not at `\EFI\BOOT\BOOTX64.EFI` still boots.

## No keyboard required

Physical machines reach iPXE with no working keyboard often enough that it is a
design constraint, not an edge case. Every generated script runs to completion
with zero keypresses: dead ends are a bounded wait that ends in `reboot` (30 s
for pending approval, 300 s for hard failures), with ESC as an escape hatch for
anyone who does have a keyboard. Tests enforce this — no bare `prompt`, no
trailing `shell`, no untimed `choose`.

The practical upshot: approving a host is hands-off. Leave the machine powered
on, approve it in the Worker, and it picks the change up on its own next retry.

The legacy menu was patched for the same reason (`choose --default boot_iscsi
--timeout 15000`), so it auto-boots instead of waiting forever. That is a
behaviour change for machines that *do* have keyboards — the menu still appears
and can still be interrupted, it just no longer blocks indefinitely. Revert with
the `.bak-ipxe-mgr` copy.

### Why the USB keyboard is dead on physical machines

`ipxe.efi` binds the USB host controllers natively (`USB_HCD_XHCI`/`EHCI`/`UHCI`
are on by default), which disconnects the firmware's UEFI USB drivers — and
iPXE's `config/usb.h` deliberately does `#undef USB_KEYBOARD` on EFI because it
expects to use "the built-in EFI keyboard driver", which is exactly what it just
disconnected. The deployed binary confirms it: xhci/ehci/uhci present, no
`usbkbd` string at all.

PS/2 keyboards are unaffected, which is why the QEMU `q35` test VM works and
metal does not — **this class of bug cannot be reproduced in that VM.**

**Fixed and deployed 2026-08-26**: rebuilt from upstream master, iPXE
1.21.1+ -> **2.0.0+**, with `config/local/usb.h` = `#define USB_KEYBOARD` (the
local config is included after the EFI `#undef`, so it wins). Verified linked
via the link map, and regression-tested on the real PXE path.

The keyboard fix itself is **unverified** — a QEMU `q35` VM is PS/2 only and
cannot exercise it. If iPXE's own HID driver does not like your keyboard, swap
to the fallback build, which drops iPXE's USB stack so firmware keeps the
controller:

```sh
ssh root@10.36.75.7 'cp /srv/tftp/ipxe-2.0.0-nousb.efi /srv/tftp/ipxe.efi'
# rollback to the original 1.21.1 build:
ssh root@10.36.75.7 'cp /srv/tftp/.bak-ipxe-mgr/ipxe.efi.1.21.1 /srv/tftp/ipxe.efi'
```

Build tree lives at `root@10.36.75.7:/root/ipxe-master`. See `PLAN.md` §11.7.

## Inventory check-in

Every machine chains `/srv/tftp/checkin.ipxe` at the top of `boot.ipxe.cfg`,
before the opt-in branch, so **all** machines report their inventory — including
ones that fall through to the legacy menu and never ask the manager for a boot
script. Unknown machines auto-register as `pending`.

It is a separate endpoint from `/v1/boot` on purpose: `/v1/boot` returns boot
semantics, and a pending host gets a reboot-retry loop, so reusing it for
check-in would loop every unapproved machine. `/v1/checkin` only returns a
non-terminal acknowledgement, and every call site ends in `||` so a manager
outage cannot block booting. The cost is one HTTPS round trip per boot.

Menu item 4 forces a check-in and reloads the config.

```sh
pnpm ipxectl gen-checkin > /tmp/checkin.ipxe   # regenerate (embeds the secret)
scp /tmp/checkin.ipxe root@10.36.75.7:/srv/tftp/checkin.ipxe
```

### The legacy menu no longer auto-boots iSCSI

`boot_iscsi` hardcodes `initiator-iqn ...:debian` and the `iscsi0` target, so
**any** machine choosing it impersonates the debian host and mounts its root
filesystem. It is no longer the unattended default — item 5 (`retry`) is: it
checks in, explains itself, and reboots to retry. `boot_iscsi` is still there as
an explicit choice.

If your physical debian box relied on the menu auto-booting, it now needs to be
registered and pinned in the manager instead (see open question 1 in `PLAN.md`),
or added to the opt-in MAC list.

## Current rollout state

Two MACs are routed to the manager:

| MAC | host | volume |
| --- | --- | --- |
| `bc:24:11:00:09:00` | `ipxe-mgr-test-90000000` (Proxmox VM 900) | `iscsi-test` |
| `1c:83:41:30:e0:b7` | `um350-03000200` (Minisforum UM350) | `minidesktop-nixos` (empty — needs an OS) |

Every other machine gets the original menu byte-for-byte. Originals are backed
up in `/srv/tftp/.bak-ipxe-mgr/`. See the runbook below for adding another.

To revert the netboot server completely:

```sh
ssh root@10.36.75.7 'cp /srv/tftp/.bak-ipxe-mgr/boot.ipxe.cfg /srv/tftp/boot.ipxe.cfg'
```

To add another machine to the opt-in, add an `iseq` line in the block at the top
of `/srv/tftp/boot.ipxe.cfg`. Before making the manager the default for
everything, read open question 1 in `PLAN.md` — the `debian` host is seeded by
name only and would re-register as a new pending host.

## Everyday operations

Everything goes through `ipxectl` (`pnpm ipxectl <cmd>`). `ipxectl help` lists
the full surface, and `--help` on any command gives its own usage:

```sh
pnpm ipxectl host list --help
```

Three output modes on list views:

| flag | for |
| --- | --- |
| *(none)* | the common columns, aligned |
| `--long` | every stored field — wide, pipe to `less -S` |
| `--json` | raw records, nothing else on stdout — for scripts and agents |

`--json` always carries every field, so `--long` is redundant with it. It is
clean under pnpm (pnpm's banner goes to stderr), so this works directly:

```sh
pnpm ipxectl host list --json | jq -r '.[] | select(.state=="pending") | .name'
pnpm ipxectl check --json | jq .ok      # {ok, fails, warns, findings}
```

`--help` is handled before config is loaded, so it works without credentials.

```sh
pnpm ipxectl host pending              # what showed up and is waiting
pnpm ipxectl host approve <name>
pnpm ipxectl volume list
pnpm ipxectl volume add iscsi0 --zvol /dev/zvol/rpool/iscsi0 \
    --target-iqn iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.… \
    --portal 10.36.75.5:3260
pnpm ipxectl assign iscsi0 --host <name>
pnpm ipxectl render <name>             # exactly what that host will receive
pnpm ipxectl events
pnpm ipxectl check                     # <- the important one
```

### `ipxectl check`

The Worker never validates anything — it stores rows and renders templates. This
is the only thing standing between a typo and a machine that will not boot. It
reads configfs and ZFS on the storage host over SSH in a single round trip and
cross-references them against what the manager believes:

- the backing zvol exists, and its size matches what was recorded
- the LIO target exists, is enabled, and has a portal
- the LUN is mapped, and its backstore really points at that zvol
- **for every pinned assignment, the target's ACL contains the host's
  `initiator_iqn` with the LUN mapped into it**
- `shared-ro` volumes actually have `write_protect=1` on every mapped LUN
- no zvol backs two volumes; every IQN is syntactically valid

Failures print the exact `targetcli` command that fixes them, and it exits
non-zero. Mutating commands (`assign`) run the same validation first and refuse
to write if it fails; `--force` overrides and is recorded in the audit log.

That fourth check is the one that earns its keep. LIO runs with
`generate_node_acls=0`, so approving a host in the manager is **not** enough to
make it boot — without an explicit per-initiator ACL the machine logs in and
sees zero LUNs, which looks exactly like a boot bug:

```
FAIL  volume iscsi-test -> ipxe-mgr-test-90000000: no LIO ACL for
      iqn.2026-02.local.client:ipxe-mgr-test-90000000; the host will log in and see zero LUNs
      fix: targetcli /iscsi/<target>/tpg1/acls create <initiator> && targetcli saveconfig
```


## Runbook: adding a host and a volume

Worked example — a 40 G NixOS volume `minidesktop-nixos` bound to the host
`um350-03000200` (a Minisforum UM350 that registered itself by checking in).

The manager owns steps 3–5. Steps 1–2 are yours on the storage host: the Worker
never touches ZFS or LIO and cannot create any of it.

### 0. Decide the name before you create the ACL

The initiator IQN is derived from the host's name at registration
(`iqn.2026-02.local.client:um350-03000200`) and the LIO ACL is keyed to that
exact string. Renaming later means recreating the ACL, so if you want a nicer
name, do it first. Note `host rename` changes only the display name — it does
**not** re-derive the IQN:

```sh
pnpm ipxectl host rename um350-03000200 minidesktop
pnpm ipxectl host set-iqn minidesktop iqn.2026-02.local.client:minidesktop
```

### 1. Create the zvol (storage host)

```sh
zfs create -V 40G rpool/minidesktop-nixos
zfs get -Hp -o value volsize rpool/minidesktop-nixos   # -> 42949672960, for --size
```

Thick, not sparse (`-s`): a sparse zvol can fail writes when the pool fills,
which is not something you want underneath a live root filesystem. Check
`zfs list rpool` for headroom first — a thick zvol reserves its full size
immediately.

### 2. Create the LIO target, LUN and ACL (storage host)

```sh
T=iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.minidesktop
I=iqn.2026-02.local.client:um350-03000200      # must match the manager exactly

targetcli /backstores/block create name=minidesktop_nixos dev=/dev/zvol/rpool/minidesktop-nixos
targetcli /iscsi create $T
targetcli /iscsi/$T/tpg1/luns create /backstores/block/minidesktop_nixos
targetcli /iscsi/$T/tpg1/acls create $I
targetcli /iscsi/$T/tpg1 set attribute authentication=0 generate_node_acls=0
targetcli saveconfig
```

The `acls create` line is the one that bites. With `generate_node_acls=0`,
omitting it means the host logs in successfully and sees **zero LUNs** — which
presents as a boot failure with no obvious cause.

### 3–5. Register, approve, bind (manager)

```sh
pnpm ipxectl volume add minidesktop-nixos \
  --zvol /dev/zvol/rpool/minidesktop-nixos \
  --target-iqn iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.minidesktop \
  --portal 10.36.75.5:3260 --size 42949672960

pnpm ipxectl host approve um350-03000200
pnpm ipxectl assign minidesktop-nixos --host um350-03000200
pnpm ipxectl check
```

**Run `check` again after `assign`, not just before.** `assign` gates on
`check`, but the per-initiator ACL check only fires once an assignment exists —
so the gate structurally cannot pre-validate the ACL. The post-assign run is the
one that actually verifies it.

### 6. Add the MAC to the opt-in list (netboot server)

Easy to miss, and the failure is confusing. Without it the machine checks in
normally, falls through to the legacy menu, hits `:retry` and reboot-loops —
never receiving its boot script. In `/srv/tftp/boot.ipxe.cfg`:

```
iseq ${net0/mac} 1c:83:41:30:e0:b7 && goto mgr ||
iseq ${mac} 1c:83:41:30:e0:b7 && goto mgr ||
```

Get the MAC from `pnpm ipxectl host show <name>`. Both forms are listed because
`${mac}` and `${net0/mac}` are not reliably the same on multi-NIC machines.

Confirm with `pnpm ipxectl render <name>`, which prints the exact script that
host will receive without booting it.

### 7. Getting an OS onto the volume — use the boot-failure menu

None of the above puts anything on the disk. A fresh zvol is empty, so the EFI
candidate sweep fails every path and drops to the **boot-failure menu**, which
is where you install from:

1. Power on the machine. It fails to boot and lands on the menu.
2. Choose **netboot.xyz installer**. The manager's script `sanhook`s the
   selected image *before* chainloading netboot.xyz, so the iSCSI LUN is
   published via iBFT and appears to the installer as an ordinary disk.
3. Pick a distro in netboot.xyz and install onto that disk.
4. Reboot. The candidate sweep now finds a bootloader and the machine boots
   from iSCSI normally.

If several images are allocated, **Select iSCSI image** retargets both the boot
attempt and netboot.xyz first. That selection is iPXE-session-local; it does not
change the pinned assignment in the manager (use `ipxectl assign` for that).

Alternatively, attach the zvol to a Proxmox VM, install there, and hand it over.

Two NixOS-specific requirements:

- The installed system's initiator IQN must match the manager's exactly
  (`services.openiscsi.name`), or LIO's ACL rejects it.
- The initrd must bring up networking and iSCSI. Otherwise iPXE hands off via
  iBFT, GRUB loads, and the kernel then panics with no root device.

## The menu

Reached two ways: press any key during the **3-second** window before the
automatic boot, or automatically when every EFI candidate fails.

```
Booting minidesktop-nixos in 3s -- press any key for the menu.
```

The interrupt stays keyboard-free: the trailing `||` swallows the timeout, so an
unattended machine falls straight through and boots. The primary image is
selected *before* the prompt, so the menu is fully usable when entered this way
— otherwise `${target-uri}` would be unset and every menu action that attaches a
LUN would hook an empty target.

`:failmenu` and `:menu` are separate entry points on purpose. Only a genuine
boot failure goes through `:failmenu`, which says so and files a `/v1/report`.
Reaching the menu deliberately — the interrupt, or backing out of a submenu —
goes straight to `:menu`, because echoing a failure and filing a sanfail report
would both be lies. (Submenu backs used to route through `:failmenu` and were
filing spurious reports; that is fixed.) A test asserts that exactly three sites
route to `:failmenu`, and that all three are `sanhook`/`sanboot` failures.

The menu itself:

```
ipxe_iscsi_mgr -- boot failed on um350-03000200
  Boot image: minidesktop-nixos [pinned]     <- one per allocated image
  Select iSCSI image (for boot and netboot.xyz)   (only if >1 image)
  netboot.xyz installer -- installs onto the selected image
  Re-fetch configuration from the manager
  Force a check-in with the manager
  Memory test
  Drop to an iPXE shell
  Reboot and retry                           (default, 600 s)
```

The failure is still reported to `/v1/report` before the menu appears, so it
shows up in `ipxectl events` either way. With no keyboard the menu times out
after **600 s** and reboots, which re-reads config from the manager — so fixing
an assignment and walking away still works.

`Re-fetch configuration from the manager` recovers without a reboot: it re-runs
`mgr.ipxe`, so a fix made in the manager takes effect immediately.

### netboot.xyz targets are surfaced in our own menu

`netboot.xyz installers` opens a submenu that jumps **straight to a leaf**:

```
netboot.xyz -- will install onto minidesktop-nixos
  Full netboot.xyz menu
  NixOS installer          <- lands directly in NixOS's version menu
  Debian installer
  Ubuntu installer
  All Linux network installs
  Live CDs
  Utilities (UEFI)
  Back
```

netboot.xyz's distro scripts (`nixos.ipxe`, `debian.ipxe`, …) live at the root
of `boot.netboot.xyz` and are self-contained given `${arch}` and `${space}`, so
chaining one directly skips netboot.xyz's top menu *and* its Linux submenu.
That matters here because the machines that need an installer are exactly the
ones whose USB keyboard may not work — every menu level skipped is one fewer
thing to navigate.

Before chaining, `:nbxgo` supplies what `menu.ipxe` would normally have set:
`boot.cfg` for the globals, netboot.xyz's own `cpuid`-based arch detection, and
`space` for item padding. Skipping that prep gives you an unpadded menu and,
worse, an unset `${arch}` so the wrong kernel is fetched.

The list is `DEFAULT_NETBOOTXYZ_ENTRIES` in `src/shared/ipxe.ts` and is
overridable per render via `netbootEntries`. Entries may be arch-scoped
(`linux.ipxe` on x86_64, `linux-arm.ipxe` on arm64).

### Chain a script, never `netboot.xyz.efi`

`netboot.xyz.efi` is for booting netboot.xyz **directly from firmware**.
Chainloading it from an already-running iPXE starts a *second* iPXE instance
that contends with the first for the NIC — on real hardware it hangs at
`Available interfaces...` forever. Observed on the UM350, 2026-08-26.

The fix is to chain netboot.xyz's *scripts* instead. They run inside the
current iPXE, so the network and the SAN hook are left alone. HTTP is the
fallback, since not every iPXE build does HTTPS.

They **cannot be mirrored locally**: these scripts fetch sub-menus and
signatures by *relative* URL, so a local copy resolves them against the netboot
server and 404s. netboot.xyz needs upstream reachability regardless — it streams
kernels from the internet — so a local mirror never removed that dependency
anyway.

One expected side effect: while you browse netboot.xyz's menu, iPXE stops
answering iSCSI NOP-In keepalives and the target drops the session
(`Did not receive response to NOPIN` in the storage host's dmesg). That is
harmless. iBFT is an ACPI *description* of the target, not the live TCP
session — the installer's initramfs reads it and opens its own session.

### keep-san makes `sanunhook` a no-op

Retrying a different image means re-hooking drive 0x80, and that is not as
simple as it looks. `sanunhook` routes through `uriboot()`, whose cleanup
honours `keep-san`: with it set, iPXE prints *"Preserving SAN device 0x80"* and
does **not** unhook. Since the boot script always sets `keep-san` (the OS needs
the session to survive handoff), a naive `sanunhook` does nothing and the next
`sanhook --drive 0x80` fails with `Could not open SAN device (0x03232094)`.

Every unhook is therefore wrapped:

```
set keep-san 0
sanunhook --drive 0x80 ||
set keep-san 1
```

Verified on hardware: without the wrapper the re-hook fails, with it the drive
unregisters and re-registers cleanly. A test asserts the wrapper is present
around every `sanunhook`.

## Firmware updates

EFI firmware updaters, delivered as bootable FAT images from R2.

```
Firmware Updates
  Auto-detected system (BESSTAR TECH LIMITED UM350)
  Other vendors:
  BESSTAR TECH LIMITED  ->  UM350  ->  AF5PN06 BIOS (2022-11-30, AMI Aptio)
```

Auto-detect matches the image's vendor/model against the SMBIOS strings already
collected at check-in (`hosts.manufacturer` / `hosts.product`), case- and
whitespace-insensitively, and jumps straight to that model's image list.

### How an image is delivered

Each image is a FAT volume containing a UEFI Shell as `\EFI\BOOT\BOOTX64.EFI`,
the vendor's EFI flasher, the ROM, and a `startup.nsh` the shell auto-runs.
Selecting it detaches the iSCSI LUN and `sanboot`s the image over HTTP:

```
sanboot --no-describe --drive 0x80 https://<mgr>/fw/<vendor>/<model>/<version>.img
```

- **Detach first.** Nothing good comes of holding a network block device open
  while the BIOS is being rewritten.
- **`--no-describe`.** A firmware image is not an OS root; publishing an iBFT
  for it would be meaningless.
- **`startup.nsh` searches `fs0:`..`fs5:` for the ROM** rather than assuming
  `fs0:`. The shell does not guarantee its working filesystem is the one
  `startup.nsh` came from.

### Why /fw/* has no auth

`sanboot` fetches the image itself, so the alternative is a pre-signed R2 URL
baked into every generated menu — which expires and leaks into logs. These are
vendor BIOS blobs, not secrets; the sensitive act is *flashing* one, which needs
physical access to the machine's boot menu. Range support is required, not
optional: `sanboot` presents the image as a virtual disk and reads it with range
requests, so without 206 responses iPXE would pull the whole image before it
could read the partition table.

### Adding an image

Build the FAT image, upload, then register (the CLI HEADs the blob first and
refuses to register something that 404s):

```sh
wrangler r2 object put ipxe-firmware/<vendor>/<model>/<ver>.img --file img --remote
pnpm ipxectl firmware add "<VENDOR>" "<MODEL>" "<VER>" --key <vendor>/<model>/<ver>.img \
    --label "human readable"
pnpm ipxectl firmware list
```

`_selftest/uefi-shell.img` is a firmware-free image that boots the shell and
prints a marker — use it to validate the delivery path without flashing
anything. It is intentionally not registered, so it never appears in the menu.

## Regenerating the chain script

The secret is embedded in `mgr.ipxe`, so it must be regenerated if the secret
rotates:

```sh
pnpm ipxectl gen-chain --fallback http://10.36.75.7/legacy.ipxe > /tmp/mgr.ipxe
scp /tmp/mgr.ipxe root@10.36.75.7:/srv/tftp/mgr.ipxe
```

`mgr.ipxe` falls back to `legacy.ipxe` (a copy of the original menu) rather than
`boot.ipxe.cfg` — falling back to `boot.ipxe.cfg` creates an infinite loop,
because that file is what routes to the manager in the first place.

## Development

```sh
pnpm install
pnpm test           # 56 tests, no network or Cloudflare needed
pnpm run typecheck
pnpm run deploy
pnpm run db:migrate:remote
```

Layout: `src/shared/` holds the schemas and the iPXE generator, `src/worker/` the
Hono app, `src/cli/` the `ipxectl` tool. The generator is shared on purpose — the Worker renders it for real and the CLI renders it for previews, so
a preview can never disagree with what a machine gets.

## Test VM

Proxmox VM 900 `ipxe-mgr-test` on `sea69-hv-puppy`, UEFI, VLAN 2, with a known
SMBIOS identity. Its volume `rpool/iscsi-test` has an EFI binary deliberately
placed at `\EFI\debian\grubx64.efi` and *not* at the default path, so booting it
proves the candidate sweep works rather than just the happy path.

```sh
ssh root@10.36.75.5 'qm start 900'
# console, since iPXE output scrolls past:
ssh root@10.36.75.5 'echo "screendump /tmp/x.ppm" | qm monitor 900'
```

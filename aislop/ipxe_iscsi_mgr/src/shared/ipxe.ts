import type { Arch, BootProfile, Host, Volume } from './types.js';

/**
 * iPXE has no quoting, so a newline in a value ends the command and anything
 * after it becomes a new one. Every DB-sourced string goes through here before
 * it reaches a script.
 */
export function sanitizeIpxeValue(value: string): string {
  return value.replace(/[\r\n]+/g, ' ').trim();
}

/** iPXE SAN URI: iscsi:<host>:<protocol>:<port>:<lun>:<target-iqn> */
export function targetUri(volume: Pick<Volume, 'portalHost' | 'portalPort' | 'lun' | 'targetIqn'>): string {
  const host = sanitizeIpxeValue(volume.portalHost);
  const iqn = sanitizeIpxeValue(volume.targetIqn);
  return `iscsi:${host}:tcp:${volume.portalPort}:${volume.lun}:${iqn}`;
}

/**
 * Parse an iPXE SAN URI back into parts. Splits from the left for the fixed
 * fields and keeps the remainder as the target IQN, because the IQN itself
 * contains colons.
 */
export function parseTargetUri(uri: string): Pick<Volume, 'portalHost' | 'portalPort' | 'lun' | 'targetIqn'> {
  const m = /^iscsi:([^:]+):([^:]*):(\d+):(\d+):(.+)$/.exec(uri.trim());
  if (!m) throw new Error(`not an iPXE iSCSI URI: ${uri}`);
  return {
    portalHost: m[1]!,
    portalPort: Number(m[3]!),
    lun: Number(m[4]!),
    targetIqn: m[5]!,
  };
}

/** Fallback bootloader sweep when a host has no profile of its own. */
export const DEFAULT_EFI_CANDIDATES: Record<Arch, string[]> = {
  x86_64: [
    '\\EFI\\BOOT\\BOOTX64.EFI',
    '\\EFI\\debian\\shimx64.efi',
    '\\EFI\\debian\\grubx64.efi',
    '\\EFI\\ubuntu\\shimx64.efi',
    '\\EFI\\ubuntu\\grubx64.efi',
    '\\EFI\\redhat\\shimx64.efi',
    '\\EFI\\rocky\\shimx64.efi',
    '\\EFI\\fedora\\shimx64.efi',
    '\\EFI\\Microsoft\\Boot\\bootmgfw.efi',
  ],
  i386: ['\\EFI\\BOOT\\BOOTIA32.EFI', '\\EFI\\debian\\grubia32.efi'],
  arm64: [
    '\\EFI\\BOOT\\BOOTAA64.EFI',
    '\\EFI\\debian\\shimaa64.efi',
    '\\EFI\\debian\\grubaa64.efi',
    '\\EFI\\ubuntu\\shimaa64.efi',
  ],
};

/** Seconds before a machine retries a recoverable state (pending approval). */
export const RETRY_SECONDS = 30;
/** Longer, so a hard failure does not turn into a tight reboot loop. */
export const FAILURE_RETRY_SECONDS = 300;

/**
 * Terminal state for a generated script.
 *
 * Every script must be able to finish WITHOUT a keypress. Physical machines
 * routinely reach iPXE with no working keyboard -- USB HID not brought up, or a
 * BMC/KVM that never presents one -- and a bare `prompt` or a trailing `shell`
 * hangs such a machine forever with no retry and no diagnostics. So every dead
 * end is a bounded wait that ends in `reboot`, with ESC as an escape hatch for
 * whoever does have a keyboard.
 *
 * `prompt --timeout` is safe with no input device: it simply expires. A bare
 * `prompt` is not, and must never be emitted.
 */
function waitThenReboot(seconds: number): string[] {
  return [
    `echo Rebooting in ${seconds}s -- press ESC for an iPXE shell.`,
    `prompt --key 0x1b --timeout ${seconds * 1000} Press ESC for a shell && shell ||`,
    'reboot',
  ];
}

/**
 * Release drive 0x80 so it can be re-hooked with a different target.
 *
 * `sanunhook` routes through uriboot(), whose cleanup honours `keep-san`: with
 * keep-san set it prints "Preserving SAN device" and does NOT unhook. Since we
 * always set keep-san (the OS needs the session to survive handoff), the flag
 * has to be cleared around the unhook or the next `sanhook --drive 0x80` fails
 * with the drive already registered -- which silently breaks every retry path
 * in the failure menu. Verified against usr/autoboot.c in iPXE 2.0.0.
 */
function unhookDrive(): string[] {
  return [
    'set keep-san 0',
    'sanunhook --drive 0x80 ||',
    'set keep-san 1',
  ];
}

/**
 * A netboot.xyz target surfaced directly in our own menu.
 *
 * `file` is a script at the root of boot.netboot.xyz. Its sub-menus and distro
 * menus are self-contained given `${arch}` and `${space}`, so we can chain a
 * leaf directly and skip netboot.xyz's top menu (and, for a distro, its Linux
 * submenu too). Fewer keypresses matters here: the machines that need an
 * installer are exactly the ones whose USB keyboard may not work.
 */
export interface NetbootXyzEntry {
  /** iPXE label; must be unique and label-safe. */
  id: string;
  label: string;
  /** Script name at the netboot.xyz root, e.g. "nixos.ipxe". */
  file: string;
  /** Restrict to one architecture; omit for all. */
  arch?: Arch;
}

/** Sensible default targets. Override per deployment via BootScriptInput. */
export const DEFAULT_NETBOOTXYZ_ENTRIES: NetbootXyzEntry[] = [
  { id: 'nbxfull', label: 'Full netboot.xyz menu', file: 'menu.ipxe' },
  { id: 'nbxnixos', label: 'NixOS installer', file: 'nixos.ipxe' },
  { id: 'nbxdebian', label: 'Debian installer', file: 'debian.ipxe' },
  { id: 'nbxubuntu', label: 'Ubuntu installer', file: 'ubuntu.ipxe' },
  { id: 'nbxlinux', label: 'All Linux network installs', file: 'linux.ipxe', arch: 'x86_64' },
  { id: 'nbxlinuxarm', label: 'All Linux network installs (arm64)', file: 'linux-arm.ipxe', arch: 'arm64' },
  { id: 'nbxlive', label: 'Live CDs', file: 'live.ipxe' },
  { id: 'nbxutils', label: 'Utilities (UEFI)', file: 'utils-efi.ipxe' },
];

/** Base URL for netboot.xyz. Not mirrorable -- see the nbxgo comment. */
const NETBOOTXYZ_BASE = 'boot.netboot.xyz';

/** A bootable firmware-update image registered in the manager. */
export interface FirmwareEntry {
  id: string;
  vendor: string;
  model: string;
  version: string;
  label?: string | null;
  /** Object key inside the firmware bucket, served from <mgrUrl>/fw/<key>. */
  r2Key: string;
}

/** Case/whitespace-insensitive compare for SMBIOS strings, which are messy. */
const smbiosEq = (a?: string | null, b?: string | null): boolean =>
  !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();

/**
 * Firmware update menu: Vendor -> Model -> image, plus a shortcut straight to
 * the model matching this host's SMBIOS.
 *
 * Selecting an image detaches the iSCSI LUN and `sanboot`s a FAT image holding
 * a UEFI Shell, the vendor's EFI flasher and a startup.nsh. Detaching first is
 * deliberate: nothing good comes of holding a network block device open while
 * the BIOS is being rewritten.
 */
export function renderFirmwareMenu(
  entries: FirmwareEntry[],
  opts: { mgrUrl: string; hostVendor?: string | null; hostProduct?: string | null },
): string[] {
  if (!entries.length) return [];
  const base = trimUrl(opts.mgrUrl);
  const L: string[] = [];

  // Group into vendor -> model -> [entries], preserving a stable order.
  const vendors = [...new Set(entries.map((e) => e.vendor))].sort();
  const modelsOf = (v: string) => [...new Set(entries.filter((e) => e.vendor === v).map((e) => e.model))].sort();
  const imagesOf = (v: string, m: string) => entries.filter((e) => e.vendor === v && e.model === m);

  // Flat index per image so every label is unique and iPXE-safe.
  const idxOf = new Map<string, number>();
  entries.forEach((e, i) => idxOf.set(e.id, i));

  const auto = entries.find(
    (e) => smbiosEq(e.vendor, opts.hostVendor) && smbiosEq(e.model, opts.hostProduct),
  );

  L.push(':fwmenu');
  L.push('menu Firmware Updates');
  if (auto) {
    L.push(
      `item fwauto   Auto-detected system (${sanitizeIpxeValue(auto.vendor)} ${sanitizeIpxeValue(auto.model)})`,
    );
    L.push('item --gap Other vendors:');
  }
  vendors.forEach((v, vi) => L.push(`item fwv${vi}   ${sanitizeIpxeValue(v)}`));
  L.push('item fwback   Back');
  L.push(
    `choose --default fwback --timeout ${SUBMENU_TIMEOUT_SECONDS * 1000} selected && goto \${selected} ||`,
  );
  L.push('goto menu');
  L.push('');
  L.push(':fwback');
  L.push('goto menu');
  L.push('');

  if (auto) {
    const vi = vendors.indexOf(auto.vendor);
    const mi = modelsOf(auto.vendor).indexOf(auto.model);
    L.push(':fwauto');
    L.push(`goto fwm${vi}_${mi}`);
    L.push('');
  }

  vendors.forEach((v, vi) => {
    const models = modelsOf(v);
    L.push(`:fwv${vi}`);
    L.push(`menu ${sanitizeIpxeValue(v)}`);
    models.forEach((m, mi) => L.push(`item fwm${vi}_${mi}   ${sanitizeIpxeValue(m)}`));
    L.push(`item fwvb${vi}   Back`);
    L.push(
      `choose --default fwvb${vi} --timeout ${SUBMENU_TIMEOUT_SECONDS * 1000} selected && goto \${selected} ||`,
    );
    L.push('goto fwmenu');
    L.push('');
    L.push(`:fwvb${vi}`);
    L.push('goto fwmenu');
    L.push('');

    models.forEach((m, mi) => {
      L.push(`:fwm${vi}_${mi}`);
      L.push(`menu ${sanitizeIpxeValue(v)} ${sanitizeIpxeValue(m)}`);
      for (const e of imagesOf(v, m)) {
        const text = sanitizeIpxeValue(e.label || e.version);
        L.push(`item fwi${idxOf.get(e.id)}   ${text}`);
      }
      L.push(`item fwmb${vi}_${mi}   Back`);
      L.push(
        `choose --default fwmb${vi}_${mi} --timeout ${SUBMENU_TIMEOUT_SECONDS * 1000} selected && goto \${selected} ||`,
      );
      L.push(`goto fwv${vi}`);
      L.push('');
      L.push(`:fwmb${vi}_${mi}`);
      L.push(`goto fwv${vi}`);
      L.push('');
    });
  });

  entries.forEach((e, i) => {
    L.push(`:fwi${i}`);
    L.push(`echo Firmware update: ${sanitizeIpxeValue(e.vendor)} ${sanitizeIpxeValue(e.model)} ${sanitizeIpxeValue(e.version)}`);
    L.push('echo Detaching iSCSI before flashing...');
    L.push(...unhookDrive());
    L.push('echo Booting the firmware updater. DO NOT power off once it starts.');
    // --no-describe: this is a firmware image, not an OS root. Publishing an
    // iBFT for it would be meaningless and misleading.
    L.push(
      `sanboot --no-describe --drive 0x80 ${base}/fw/${e.r2Key} || goto fwfail`,
    );
    L.push('goto menu');
    L.push('');
  });

  L.push(':fwfail');
  L.push('echo The firmware image did not boot. Nothing was flashed.');
  L.push('goto menu');
  L.push('');
  return L;
}

export interface VolumeChoice {
  name: string;
  portalHost: string;
  portalPort: number;
  lun: number;
  targetIqn: string;
  /** pinned | general-exclusive | shared-ro -- shown in the failure menu. */
  mode?: string | null;
}

export interface BootScriptInput {
  host: Pick<Host, 'name' | 'initiatorIqn' | 'arch'> & {
    /** SMBIOS strings, used to auto-detect a matching firmware image. */
    manufacturer?: string | null;
    product?: string | null;
  };
  /** The image tried first. */
  volume: VolumeChoice;
  /** Every image allocated to this host, listed in the failure menu. */
  volumes?: VolumeChoice[];
  profile?: Pick<BootProfile, 'efiCandidates' | 'extraIpxe'> | null;
  /** Base URL of this manager, used for the failure report. Omit to skip it. */
  mgrUrl?: string | null;
  /** Netboot server, e.g. http://10.36.75.7. Enables reload/check-in/memtest. */
  bootServer?: string | null;
  /** Extra `set` lines carried over from the legacy config, e.g. boot-server. */
  settings?: Record<string, string>;
  /** netboot.xyz targets to surface directly. Defaults to the built-in list. */
  netbootEntries?: NetbootXyzEntry[];
  /** Firmware images to offer. Omitted or empty hides the firmware menu. */
  firmware?: FirmwareEntry[];
}

/** How long the failure menu waits before rebooting to retry. */
export const FAILMENU_TIMEOUT_SECONDS = 600;
/** Submenus back out quickly rather than holding the machine for 10 minutes. */
export const SUBMENU_TIMEOUT_SECONDS = 60;
/** Grace period before the automatic boot, so a human can reach the menu. */
export const INTERRUPT_SECONDS = 3;

/**
 * The real thing: hook the target once, then walk the bootloader candidates
 * against the already-hooked drive.
 *
 * Three things here are load-bearing and easy to "clean up" into a bug:
 *   - no `--no-describe`: iBFT is what lets the OS adopt the session after
 *     iPXE exits. Suppressing it boots the bootloader and then panics on a
 *     missing root device.
 *   - `--keep` on every attempt: a bare failed `sanboot` unhooks the drive,
 *     so candidate 2 onwards would fail for the wrong reason.
 *   - `sanunhook` before `sanhook`: the failure menu can re-enter :attach to
 *     try a different image, and hooking drive 0x80 twice fails.
 */
export function renderBootScript(input: BootScriptInput): string {
  const { host, volume, profile, mgrUrl, settings, bootServer } = input;
  const candidates = (profile?.efiCandidates?.length
    ? profile.efiCandidates
    : DEFAULT_EFI_CANDIDATES[host.arch]
  ).map(sanitizeIpxeValue);

  // The primary always appears in the menu, and never twice.
  const choices: VolumeChoice[] = [volume];
  for (const v of input.volumes ?? []) {
    if (!choices.some((c) => c.targetIqn === v.targetIqn && c.lun === v.lun)) choices.push(v);
  }

  const L: string[] = ['#!ipxe'];
  L.push(`# host=${sanitizeIpxeValue(host.name)} volume=${sanitizeIpxeValue(volume.name)}`);
  L.push('');
  L.push('# keep the iSCSI session alive across the handoff to the OS');
  L.push('set keep-san 1');
  L.push('');
  for (const [k, v] of Object.entries(settings ?? {})) {
    L.push(`set ${sanitizeIpxeValue(k)} ${sanitizeIpxeValue(v)}`);
  }
  L.push(`set initiator-iqn ${sanitizeIpxeValue(host.initiatorIqn)}`);
  L.push('');
  if (profile?.extraIpxe) L.push(profile.extraIpxe.trim(), '');

  // Pre-select the primary image *before* offering the interrupt, so the menu
  // is fully usable when entered this way. Without this ${target-uri} is unset
  // and every menu action that attaches a LUN would hook an empty target.
  L.push(`set target-name ${sanitizeIpxeValue(volume.name)}`);
  L.push(`set target-uri ${targetUri(volume)}`);
  L.push('');
  L.push(`echo Booting \${target-name} in ${INTERRUPT_SECONDS}s -- press any key for the menu.`);
  // Key pressed -> menu. Timeout -> failure -> `||` swallows it and we fall
  // through into :vol0 and boot. Safe on a machine with no keyboard.
  L.push(
    `prompt --timeout ${INTERRUPT_SECONDS * 1000} Press any key for the ipxe_iscsi_mgr menu && goto menu ||`,
  );
  L.push('');

  // Each allocated image gets an entry point that sets the target and jumps to
  // the shared attach/sweep block, so the candidate list is not duplicated.
  choices.forEach((v, i) => {
    L.push(`:vol${i}`);
    L.push(`set target-name ${sanitizeIpxeValue(v.name)}`);
    L.push(`set target-uri ${targetUri(v)}`);
    L.push('goto attach', '');
  });

  L.push(':attach');
  L.push('echo Attaching ${target-name} -- ${target-uri}');
  L.push(...unhookDrive());
  L.push('sanhook --drive 0x80 ${target-uri} || goto failmenu');
  L.push('');

  candidates.forEach((file, i) => {
    L.push(`:try${i}`);
    L.push(`echo Trying ${file}`);
    L.push(`sanboot --keep --drive 0x80 --filename ${file} || goto ${i + 1 < candidates.length ? `try${i + 1}` : 'trydefault'}`);
  });

  L.push(':trydefault');
  L.push('echo Trying UEFI default from the ESP');
  L.push('sanboot --keep --drive 0x80 || goto failmenu');
  L.push('');

  // --- failure menu ------------------------------------------------------
  // Two entry points on purpose. :failmenu is for a genuine boot failure -- it
  // says so and files a report. :menu is the same menu reached deliberately
  // (the interrupt above, or backing out of a submenu), where echoing a failure
  // and filing a sanfail report would both be lies.
  L.push(':failmenu');
  L.push('echo Boot failed for ${target-name}');
  if (mgrUrl) {
    L.push(
      `chain --autofree ${trimUrl(mgrUrl)}/v1/report?host=\${uuid}&mac=\${mac:hexhyp}&status=sanfail ||`,
    );
  }
  L.push('goto menu');
  L.push('');
  L.push(':menu');
  L.push(`menu ipxe_iscsi_mgr -- ${sanitizeIpxeValue(host.name)}`);
  choices.forEach((v, i) => {
    const mode = v.mode ? ` [${sanitizeIpxeValue(v.mode)}]` : '';
    L.push(`item vol${i}   Boot image: ${sanitizeIpxeValue(v.name)}${mode}`);
  });
  const base = bootServer ? trimUrl(bootServer) : null;
  if (choices.length > 1) {
    L.push('item selimg    Select iSCSI image (for boot and netboot.xyz)');
  }
  L.push('item nbxmenu   netboot.xyz installers -- installs onto the selected image');
  if ((input.firmware ?? []).length && mgrUrl) {
    L.push('item fwmenu    Firmware updates');
  }
  if (base) {
    L.push('item reload    Re-fetch configuration from the manager');
    L.push('item forcein   Force a check-in with the manager');
    L.push('item memtest   Memory test');
  }
  L.push('item shellout  Drop to an iPXE shell');
  L.push('item rebootnow Reboot and retry');
  // Default must be safe with no keyboard: rebooting re-runs the whole flow and
  // picks up any change made in the manager meanwhile.
  L.push(`choose --default rebootnow --timeout ${FAILMENU_TIMEOUT_SECONDS * 1000} selected && goto \${selected} ||`);
  L.push('goto rebootnow');
  L.push('');


  // netboot.xyz, with the selected image attached first.
  //
  // The point is iBFT: `sanhook` publishes the iSCSI Boot Firmware Table into
  // ACPI, so an installer booted through netboot.xyz sees the LUN as a normal
  // disk and can install onto it.
  //
  // Chain a *script*, NOT `netboot.xyz.efi`. The .efi is for booting
  // netboot.xyz from firmware; chainloading it from an already-running iPXE
  // starts a SECOND iPXE that contends for the NIC and hangs at "Available
  // interfaces..." on real hardware. Scripts run inside the current instance,
  // so the network and the SAN hook are left alone.
  //
  // Nor can it be mirrored locally: these scripts fetch their sub-menus and
  // signatures by RELATIVE URL, so a local copy resolves them against the
  // netboot server and 404s. netboot.xyz streams kernels from upstream anyway.
  const entries = (input.netbootEntries ?? DEFAULT_NETBOOTXYZ_ENTRIES).filter(
    (e) => !e.arch || e.arch === host.arch,
  );

  L.push(':nbxmenu');
  L.push(`menu netboot.xyz -- will install onto \${target-name}`);
  for (const e of entries) {
    L.push(`item ${sanitizeIpxeValue(e.id)}  ${sanitizeIpxeValue(e.label)}`);
  }
  L.push('item nbxback  Back');
  L.push(
    `choose --default nbxback --timeout ${SUBMENU_TIMEOUT_SECONDS * 1000} selected && goto \${selected} ||`,
  );
  L.push('goto menu');
  L.push('');
  for (const e of entries) {
    L.push(`:${sanitizeIpxeValue(e.id)}`);
    L.push(`set nbxfile ${sanitizeIpxeValue(e.file)}`);
    L.push('goto nbxgo');
    L.push('');
  }
  L.push(':nbxback');
  L.push('goto menu');
  L.push('');

  L.push(':nbxgo');
  L.push('echo Attaching ${target-name} so the installer can see it via iBFT...');
  L.push(...unhookDrive());
  L.push('sanhook --drive 0x80 ${target-uri} || goto failmenu');
  // boot.cfg supplies the globals netboot.xyz scripts expect; arch and space
  // are normally set by menu.ipxe, which we are skipping. Arch detection is
  // copied from netboot.xyz's own menu.ipxe so it behaves identically.
  L.push('echo Preparing netboot.xyz environment...');
  L.push(`chain --autofree https://${NETBOOTXYZ_BASE}/boot.cfg ||`);
  L.push('cpuid --ext 29 && set arch x86_64 || set arch i386');
  L.push('iseq ${buildarch} arm64 && set arch arm64 ||');
  L.push('set space:hex 20:20');
  L.push('set space ${space:string}');
  L.push('echo Loading netboot.xyz ${nbxfile} -- install onto ${target-name}');
  L.push(`chain --autofree https://${NETBOOTXYZ_BASE}/\${nbxfile} || goto nbxhttp`);
  L.push('goto menu');
  L.push('');
  L.push(':nbxhttp');
  L.push('echo HTTPS failed, retrying netboot.xyz over HTTP...');
  L.push(`chain --autofree http://${NETBOOTXYZ_BASE}/\${nbxfile} || goto menu`);
  L.push('goto menu');
  L.push('');

  // Image selection. Sets the variables the boot attempt and netboot.xyz both
  // read, so a rescue image can be installed to, or a different image booted,
  // without touching the manager.
  if (choices.length > 1) {
    L.push(':selimg');
    L.push('menu Select iSCSI image');
    choices.forEach((v, i) => {
      const mode = v.mode ? ` [${sanitizeIpxeValue(v.mode)}]` : '';
      L.push(`item sel${i}   ${sanitizeIpxeValue(v.name)}${mode}`);
    });
    L.push('item selback  Back');
    L.push(
      `choose --default selback --timeout ${SUBMENU_TIMEOUT_SECONDS * 1000} selected && goto \${selected} ||`,
    );
    L.push('goto menu');
    L.push('');
    choices.forEach((v, i) => {
      L.push(`:sel${i}`);
      L.push(`set target-name ${sanitizeIpxeValue(v.name)}`);
      L.push(`set target-uri ${targetUri(v)}`);
      L.push('echo Selected ${target-name}');
      L.push('goto menu');
      L.push('');
    });
    L.push(':selback');
    L.push('goto menu');
    L.push('');
  }

  if (base) {
    L.push(':reload');
    L.push(`chain --autofree ${base}/mgr.ipxe || goto menu`);
    L.push('goto rebootnow');
    L.push('');
    L.push(':forcein');
    L.push('set checkin-force 1');
    L.push(`chain --autofree ${base}/checkin.ipxe ||`);
    L.push('set checkin-force 0');
    L.push('goto menu');
    L.push('');
    L.push(':memtest');
    L.push(`chain ${base}/memtest/BOOTX64.efi || goto menu`);
    L.push('goto menu');
    L.push('');
  }
  L.push(':shellout');
  L.push('shell');
  L.push('goto menu');
  L.push('');
  if ((input.firmware ?? []).length && mgrUrl) {
    L.push(...renderFirmwareMenu(input.firmware!, {
      mgrUrl,
      hostVendor: host.manufacturer,
      hostProduct: host.product,
    }));
  }

  L.push(':rebootnow');
  L.push('reboot');
  L.push('');
  return L.join('\n');
}

/** Script for a host we have never seen. It has been recorded; it just cannot boot yet. */
export function renderPendingScript(hostRef: string, retrySeconds = RETRY_SECONDS): string {
  return [
    '#!ipxe',
    `# host recorded, awaiting approval: ${sanitizeIpxeValue(hostRef)}`,
    '',
    'echo',
    'echo This machine has been registered and is awaiting approval.',
    `echo Reference: ${sanitizeIpxeValue(hostRef)}`,
    'echo',
    ...waitThenReboot(retrySeconds),
    '',
  ].join('\n');
}

export function renderNoVolumeScript(hostName: string): string {
  return [
    '#!ipxe',
    '',
    'echo',
    `echo No boot volume is assigned to ${sanitizeIpxeValue(hostName)}.`,
    'echo Assign one with: ipxectl assign <volume> --host ' + sanitizeIpxeValue(hostName),
    'echo',
    ...waitThenReboot(FAILURE_RETRY_SECONDS),
    '',
  ].join('\n');
}

export function renderRefusalScript(reason: string): string {
  return [
    '#!ipxe',
    '',
    'echo',
    `echo Boot refused: ${sanitizeIpxeValue(reason)}`,
    'echo',
    ...waitThenReboot(FAILURE_RETRY_SECONDS),
    '',
  ].join('\n');
}

/**
 * The inventory a booting machine reports about itself. Shared verbatim by the
 * boot chain and the check-in script -- if these two ever drift, a host's
 * identity changes depending on which path it took, and it re-registers as a
 * duplicate.
 */
export const BOOT_FACTS_QUERY = [
  'mac=${mac:hexhyp}',
  // Bare, NOT :uristring. `uuid` is already a formatted UUID; applying
  // :uristring re-reads the underlying 16 raw bytes and returns mojibake.
  'uuid=${uuid}',
  'serial=${serial:uristring}',
  'asset=${asset:uristring}',
  'manufacturer=${manufacturer:uristring}',
  'product=${product:uristring}',
  // memsize is a BIOS-era setting and comes back empty under UEFI, so the
  // SMBIOS probes below are the fallback. See decodeMem() in the worker.
  'mem=${memsize}',
  'mem16=${smbios/16.0x7.4}',
  'mem17=${smbios/17.0xc.2}',
  'arch=${buildarch}',
  'platform=${platform}',
  // SMBIOS type 4: 0x10 = Processor Version, 0x07 = Processor Manufacturer.
  // QEMU fills Version with its machine type; real hardware puts the CPU
  // model there.
  'cpu=${smbios/4.0x10.0:uristring}',
  'cpuvendor=${smbios/4.0x07.0:uristring}',
].join('&');

/**
 * The bootstrap script that lives on the netboot server. Collects what SMBIOS
 * will give us and chains into the manager.
 *
 * `${mac:hexhyp}` renders aa-bb-cc-dd-ee-ff rather than colon-separated, and
 * `:uristring` percent-encodes free-text SMBIOS fields that routinely contain
 * spaces.
 */
export function renderChainScript(
  mgrUrl: string,
  opts: { fallbackUrl?: string | null; secret?: string | null } = {},
): string {
  // iPXE supports credentials in the URI userinfo, which it sends as HTTP
  // Basic. That keeps the secret out of the request path, but it does mean
  // this file (and any iPXE binary embedding it) is itself a credential.
  const base = opts.secret ? withUserinfo(trimUrl(mgrUrl), 'boot', opts.secret) : trimUrl(mgrUrl);
  const query = BOOT_FACTS_QUERY;

  // iPXE has no multi-line `||`; each fallback is its own labelled step.
  const L = [
    '#!ipxe',
    '# generated by ipxe_iscsi_mgr -- edit the manager, not this file',
    '',
    'dhcp || echo DHCP failed, continuing anyway',
    '',
    ':mgr',
    `chain --autofree ${base}/v1/boot?${query} || goto fallback`,
    'goto done',
    '',
    ':fallback',
  ];
  if (opts.fallbackUrl) {
    L.push('echo Manager unreachable, falling back to the local config');
    L.push(`chain --autofree ${trimUrl(opts.fallbackUrl)} || goto shellout`);
    L.push('goto done');
  } else {
    L.push('echo Manager unreachable');
    L.push('goto shellout');
  }
  L.push('', ':shellout', ...waitThenReboot(FAILURE_RETRY_SECONDS), '', ':done', '');
  return L.join('\n');
}


/**
 * Inventory check-in, served by the netboot server and run by EVERY machine at
 * startup -- including ones that go on to boot from the legacy menu and never
 * touch the manager otherwise. That is the point: reporting yourself is
 * decoupled from getting a boot script.
 *
 * Deliberately non-terminal. It contains no `boot`, `reboot` or `shell`, so
 * when it finishes iPXE returns control to whatever chained it and the normal
 * boot path continues. The caller must invoke it with a trailing `||` so a
 * manager outage can never block booting.
 *
 * `${checkin-force}` lets the caller request a forced check-in (the menu item)
 * without needing a second copy of this file. Unset expands to empty, which
 * the worker reads as false.
 */
export function renderCheckinScript(mgrUrl: string, opts: { secret?: string | null } = {}): string {
  const base = opts.secret ? withUserinfo(trimUrl(mgrUrl), 'boot', opts.secret) : trimUrl(mgrUrl);
  return [
    '#!ipxe',
    '# generated by ipxe_iscsi_mgr -- edit the manager, not this file',
    '# Non-terminal on purpose: returns to the caller so booting continues.',
    '',
    'echo Checking in with ipxe_iscsi_mgr...',
    `chain --autofree ${base}/v1/checkin?force=\${checkin-force}&${BOOT_FACTS_QUERY} ||`,
    '',
  ].join('\n');
}

/**
 * The worker's reply to a check-in. Must not boot, reboot or prompt -- it runs
 * inside someone else's boot flow and has to hand control straight back.
 */
export function renderCheckinAck(hostName: string, state: string, isNew: boolean): string {
  return [
    '#!ipxe',
    `# checkin ok: ${sanitizeIpxeValue(hostName)} (${sanitizeIpxeValue(state)})`,
    `echo Checked in as ${sanitizeIpxeValue(hostName)} [${sanitizeIpxeValue(state)}]${isNew ? ' -- newly registered' : ''}`,
    '',
  ].join('\n');
}

function trimUrl(u: string): string {
  return sanitizeIpxeValue(u).replace(/\/+$/, '');
}

/** https://host/path -> https://user:secret@host/path */
function withUserinfo(url: string, user: string, secret: string): string {
  const m = /^(https?:\/\/)(.*)$/.exec(url);
  if (!m) throw new Error(`cannot embed credentials in ${url}`);
  return `${m[1]}${encodeURIComponent(user)}:${encodeURIComponent(sanitizeIpxeValue(secret))}@${m[2]}`;
}

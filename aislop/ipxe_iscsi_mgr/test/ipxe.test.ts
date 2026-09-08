import { describe, expect, it } from 'vitest';
import {
  BOOT_FACTS_QUERY,
  DEFAULT_EFI_CANDIDATES,
  DEFAULT_NETBOOTXYZ_ENTRIES,
  FAILMENU_TIMEOUT_SECONDS,
  INTERRUPT_SECONDS,
  SUBMENU_TIMEOUT_SECONDS,
  parseTargetUri,
  renderBootScript,
  renderChainScript,
  renderCheckinAck,
  renderCheckinScript,
  renderNoVolumeScript,
  renderPendingScript,
  renderRefusalScript,
  sanitizeIpxeValue,
  targetUri,
} from '../src/shared/ipxe.js';
import { iqnSchema, volumeSchema } from '../src/shared/types.js';

/**
 * Live values, read off sea69-hv-puppy and sea69-bootbox on 2026-08-25.
 * These are the import baseline -- if the generator stops reproducing them the
 * one machine that currently boots stops booting.
 */
const LIVE_TARGET_IQN = 'iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.80c84b6cc10c';
const LIVE_INITIATOR_IQN = 'iqn.2026-02.local.client:debian';
const LIVE_TARGET_URI = `iscsi:10.36.75.5:tcp:3260:0:${LIVE_TARGET_IQN}`;

const debianVolume = {
  name: 'iscsi0',
  zvolPath: '/dev/zvol/rpool/iscsi0',
  portalHost: '10.36.75.5',
  portalPort: 3260,
  lun: 0,
  targetIqn: LIVE_TARGET_IQN,
};

const debianHost = {
  name: 'debian',
  initiatorIqn: LIVE_INITIATOR_IQN,
  arch: 'x86_64' as const,
};


/**
 * A bare `shell` strands a keyboard-less machine -- unless it sits behind a
 * label that is only reachable by choosing a menu item, which already required
 * a keyboard. This returns the labels where a reachable-without-input `shell`
 * lives; it must always be empty.
 *
 * Doubles as the injection check: a smuggled `shell` lands at a label that is
 * not a menu target, so it shows up here.
 */
function unreachableShellLabels(script: string): string[] {
  const items = new Set([...script.matchAll(/^item\s+(\S+)/gm)].map((m) => m[1]!));
  const bad: string[] = [];
  let label = '';
  for (const raw of script.split('\n')) {
    const l = raw.trim();
    const m = /^:(\w+)$/.exec(l);
    if (m) { label = m[1]!; continue; }
    if (l === 'shell' && !items.has(label)) bad.push(label || '(top level)');
  }
  return bad;
}

describe('import parity with the hand-rolled config', () => {
  it('reproduces the live target-uri byte-for-byte', () => {
    expect(targetUri(debianVolume)).toBe(LIVE_TARGET_URI);
  });

  it('emits the three set lines the legacy config had', () => {
    const script = renderBootScript({
      host: debianHost,
      volume: debianVolume,
      settings: { 'boot-server': 'http://10.36.75.7' },
    });
    expect(script).toContain('set boot-server http://10.36.75.7');
    expect(script).toContain(`set initiator-iqn ${LIVE_INITIATOR_IQN}`);
    expect(script).toContain(`set target-uri ${LIVE_TARGET_URI}`);
  });

  it('round-trips the target URI despite colons in the IQN', () => {
    const parsed = parseTargetUri(LIVE_TARGET_URI);
    expect(parsed.targetIqn).toBe(LIVE_TARGET_IQN);
    expect(parsed.portalHost).toBe('10.36.75.5');
    expect(parsed.portalPort).toBe(3260);
    expect(parsed.lun).toBe(0);
    expect(targetUri(parsed)).toBe(LIVE_TARGET_URI);
  });

  it('still tries the debian bootloaders the legacy config tried', () => {
    const script = renderBootScript({ host: debianHost, volume: debianVolume });
    for (const f of ['\\EFI\\debian\\shimx64.efi', '\\EFI\\debian\\grubx64.efi', '\\EFI\\BOOT\\BOOTX64.EFI']) {
      expect(script).toContain(`--filename ${f}`);
    }
  });
});

describe('boot script correctness', () => {
  const script = renderBootScript({ host: debianHost, volume: debianVolume, mgrUrl: 'https://mgr.example/' });

  it('never suppresses iBFT -- the OS needs it to adopt the session', () => {
    expect(script).not.toContain('--no-describe');
  });

  it('keeps the drive hooked so later candidates are tried against a live drive', () => {
    const sanboots = script.split('\n').filter((l) => l.startsWith('sanboot '));
    expect(sanboots.length).toBeGreaterThan(1);
    for (const line of sanboots) expect(line).toContain('--keep');
  });

  it('sets keep-san so the session survives the handoff', () => {
    expect(script).toContain('set keep-san 1');
  });

  it('always unhooks before hooking, so every attach site is re-entrant', () => {
    // There are two legitimate hook sites: :attach and :nbxyz. Both can be
    // re-entered from the failure menu, so both must unhook first or the
    // second hook of drive 0x80 fails.
    const lines = script.split('\n').map((l) => l.trim());
    const hooks = lines.map((l, i) => [l, i] as const).filter(([l]) => l.startsWith('sanhook '));
    expect(hooks.length).toBeGreaterThan(0);
    for (const [, i] of hooks) {
      expect(lines.slice(0, i).reverse().find((l) => l.startsWith('sanunhook') || l.startsWith('sanhook')))
        .toMatch(/^sanunhook/);
    }
  });

  it('gives every candidate a reachable label and a terminal fallback', () => {
    const labels = new Set([...script.matchAll(/^:(\w+)$/gm)].map((m) => m[1]!));
    const gotos = [...script.matchAll(/goto (\w+)/g)].map((m) => m[1]!);
    for (const g of gotos) expect(labels).toContain(g);
    expect(script).toContain(':trydefault');
    expect(script).toContain(':failmenu');
  });

  it('ends the candidate chain at trydefault, not off the end of the script', () => {
    const cands = DEFAULT_EFI_CANDIDATES.x86_64;
    expect(script).toContain(`--filename ${cands[cands.length - 1]} || goto trydefault`);
  });
});

describe('injection safety', () => {
  it('collapses newlines that would otherwise start a new iPXE command', () => {
    expect(sanitizeIpxeValue('a\nshell\nb')).toBe('a shell b');
  });

  it('cannot be used to smuggle a command through a volume name', () => {
    const script = renderBootScript({
      host: debianHost,
      volume: { ...debianVolume, name: 'evil\nshell' },
      mgrUrl: 'https://mgr.example',
    });
    // The only legitimate `shell` is behind the menu's shellout item.
    expect(unreachableShellLabels(script)).toEqual([]);
    expect(script).toContain('volume=evil shell');
  });

  it('cannot smuggle a command through an initiator IQN', () => {
    const script = renderBootScript({
      host: { ...debianHost, initiatorIqn: 'iqn.2026-02.local.client:x\nsanboot http://evil/' },
      volume: debianVolume,
      mgrUrl: 'https://mgr.example',
    });
    expect(script).not.toMatch(/^sanboot http/m);
  });
});

describe('chain script', () => {
  const script = renderChainScript('https://mgr.example', { fallbackUrl: 'http://10.36.75.7/boot.ipxe.cfg' });

  it('uses hyphenated MAC and encodes free-text SMBIOS fields', () => {
    expect(script).toContain('mac=${mac:hexhyp}');
    expect(script).toContain('product=${product:uristring}');
    expect(script).toContain('cpu=${smbios/4.0x10.0:uristring}');
  });

  it('has no bare multi-line || continuation, which iPXE cannot parse', () => {
    for (const line of script.split('\n')) expect(line.trimStart()).not.toMatch(/^\|\|/);
  });

  it('falls back to the local config when the manager is unreachable', () => {
    expect(script).toContain('chain --autofree http://10.36.75.7/boot.ipxe.cfg || goto shellout');
  });
});

describe('schema validation', () => {
  it('accepts the live IQNs', () => {
    expect(iqnSchema.safeParse(LIVE_TARGET_IQN).success).toBe(true);
    expect(iqnSchema.safeParse(LIVE_INITIATOR_IQN).success).toBe(true);
  });

  it('rejects malformed IQNs and non-zvol paths', () => {
    expect(iqnSchema.safeParse('iqn.bogus').success).toBe(false);
    expect(iqnSchema.safeParse('not-an-iqn').success).toBe(false);
    expect(volumeSchema.safeParse({ ...debianVolume, id: 'v1', zvolPath: '/dev/sda' }).success).toBe(false);
  });
});

/**
 * A physical machine can reach iPXE with no working keyboard (USB HID not
 * brought up, or a BMC/KVM that never presents one). Any script that dead-ends
 * in a bare `prompt` or a trailing `shell` strands such a machine forever, with
 * no retry and no telemetry. Observed on real hardware 2026-08-26.
 */
describe('every generated script survives a machine with no keyboard', () => {
  const scripts: Record<string, string> = {
    boot: renderBootScript({ host: debianHost, volume: debianVolume, mgrUrl: 'https://mgr.example' }),
    bootNoMgr: renderBootScript({ host: debianHost, volume: debianVolume }),
    pending: renderPendingScript('some-host'),
    noVolume: renderNoVolumeScript('some-host'),
    refusal: renderRefusalScript('nope'),
    chain: renderChainScript('https://mgr.example', { fallbackUrl: 'http://10.0.0.1/legacy.ipxe' }),
  };

  for (const [name, script] of Object.entries(scripts)) {
    describe(name, () => {
      const s = script;
      const lines = script.split('\n').map((l) => l.trim()).filter(Boolean);

      it('never blocks on an untimed prompt', () => {
        for (const line of lines) {
          if (line.startsWith('prompt')) expect(line).toContain('--timeout');
        }
      });

      it('never dead-ends in a shell reachable without a keypress', () => {
        expect(unreachableShellLabels(s)).toEqual([]);
      });

      it('never blocks on a choose without BOTH a timeout and a safe default', () => {
        for (const line of lines) {
          if (!line.startsWith('choose')) continue;
          expect(line).toContain('--timeout');
          expect(line).toContain('--default');
          // The unattended default must not be an interactive dead end.
          const def = /--default\s+(\S+)/.exec(line)?.[1];
          expect(def).not.toBe('shellout');
          expect(def).not.toBe('shell');
        }
      });

      it('always reaches a reboot so the machine retries on its own', () => {
        expect(lines).toContain('reboot');
      });
    });
  }

  it('backs off harder on a hard failure than on pending approval', () => {
    expect(scripts.pending).toContain(`--timeout ${30 * 1000} `);
    expect(scripts.refusal).toContain(`--timeout ${300 * 1000} `);
  });

  it('still offers a shell to anyone who does have a keyboard', () => {
    expect(scripts.refusal).toContain('Press ESC for a shell && shell ||');
  });
});

describe('inventory check-in', () => {
  const script = renderCheckinScript('https://mgr.example', { secret: 's3cr3t' });
  const ack = renderCheckinAck('some-host', 'pending', true);

  it('reports the exact same facts as the boot chain, so identity cannot drift', () => {
    const chain = renderChainScript('https://mgr.example', { secret: 's3cr3t' });
    for (const field of BOOT_FACTS_QUERY.split('&')) {
      expect(script).toContain(field);
      expect(chain).toContain(field);
    }
  });

  it('hits /v1/checkin, never /v1/boot -- boot semantics would reboot-loop a pending host', () => {
    expect(script).toContain('/v1/checkin?');
    expect(script).not.toContain('/v1/boot');
  });

  it('passes the force flag through as a variable so one file serves both callers', () => {
    expect(script).toContain('force=${checkin-force}');
  });

  it('carries the secret as userinfo', () => {
    expect(script).toContain('https://boot:s3cr3t@mgr.example/v1/checkin');
  });

  /**
   * The inverse of the keyboard-free rule elsewhere: this script runs *inside*
   * another script's boot flow, so it must hand control back rather than take
   * a terminal action. A stray `reboot` here would loop every machine on the
   * network at startup.
   */
  for (const [name, s] of Object.entries({ script, ack })) {
    it(`${name} is non-terminal -- no boot, reboot, shell or prompt`, () => {
      const lines = s.split('\n').map((l) => l.trim()).filter(Boolean);
      for (const verb of ['reboot', 'shell', 'sanboot', 'boot', 'prompt']) {
        expect(lines.some((l) => l === verb || l.startsWith(`${verb} `))).toBe(false);
      }
    });
  }

  it('ends every failable command with || so a manager outage cannot block booting', () => {
    const chains = script.split('\n').filter((l) => l.startsWith('chain '));
    expect(chains.length).toBeGreaterThan(0);
    for (const line of chains) expect(line.trimEnd().endsWith('||')).toBe(true);
  });

  it('acknowledges a newly registered host distinctly from a known one', () => {
    expect(renderCheckinAck('h', 'pending', true)).toContain('newly registered');
    expect(renderCheckinAck('h', 'approved', false)).not.toContain('newly registered');
  });
});

describe('boot failure menu', () => {
  const vol = (name: string, sn: string, mode: string) => ({
    name, portalHost: '10.36.75.5', portalPort: 3260, lun: 0, mode,
    targetIqn: `iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.${sn}`,
  });
  const primary = vol('minidesktop-nixos', 'minidesktop', 'pinned');
  const rescue = vol('rescue', 'rescue', 'shared-ro');
  const script = renderBootScript({
    host: { name: 'um350', initiatorIqn: 'iqn.2026-02.local.client:um350', arch: 'x86_64' },
    volume: primary,
    volumes: [primary, rescue],
    mgrUrl: 'https://mgr.example',
    bootServer: 'http://10.36.75.7',
  });
  const items = [...script.matchAll(/^item\s+(\S+)\s+(.*)$/gm)].map((m) => ({ label: m[1]!, text: m[2]! }));
  const labels = new Set([...script.matchAll(/^:(\w+)$/gm)].map((m) => m[1]!));

  it('waits 600s before retrying, per the configured failure timeout', () => {
    expect(script).toContain(`--timeout ${FAILMENU_TIMEOUT_SECONDS * 1000}`);
    expect(FAILMENU_TIMEOUT_SECONDS).toBe(600);
  });

  it('defaults to rebooting, which re-reads config from the manager', () => {
    expect(script).toMatch(/choose --default rebootnow --timeout 600000/);
    expect(labels).toContain('rebootnow');
  });

  it('lists every image allocated to the host', () => {
    const text = items.filter((i) => i.label.startsWith('vol')).map((i) => i.text).join('\n');
    expect(text).toContain('minidesktop-nixos');
    expect(text).toContain('rescue');
    expect(text).toContain('[pinned]');
    expect(text).toContain('[shared-ro]');
  });

  it('does not list the primary image twice when it is also in the allocation list', () => {
    const bootItems = items.filter((i) => /^vol\d+$/.test(i.label));
    expect(bootItems.filter((i) => i.text.includes('minidesktop-nixos'))).toHaveLength(1);
    expect(bootItems).toHaveLength(2);
  });

  it('gives every menu item a label that actually exists', () => {
    for (const i of items) expect(labels).toContain(i.label);
  });

  it('lets each image be retried through the shared attach block', () => {
    expect(script).toContain(':vol0');
    expect(script).toContain(':vol1');
    expect(script.match(/goto attach/g)?.length).toBe(2);
    // The candidate sweep is written once, not duplicated per image.
    expect(script.match(/:try0/g)).toHaveLength(1);
  });

  it('unhooks before hooking, so re-entering :attach for a second image works', () => {
    const lines = script.split('\n').map((l) => l.trim());
    const unhook = lines.indexOf('sanunhook --drive 0x80 ||');
    const hook = lines.findIndex((l) => l.startsWith('sanhook '));
    expect(unhook).toBeGreaterThan(-1);
    expect(unhook).toBeLessThan(hook);
  });

  it('clears keep-san around every unhook -- otherwise the unhook is a no-op', () => {
    // sanunhook honours keep-san and refuses to unhook while it is set, so
    // without this every retry hits "drive 0x80 already registered".
    const lines = script.split('\n').map((l) => l.trim());
    const unhooks = lines.map((l, i) => [l, i] as const).filter(([l]) => l.startsWith('sanunhook'));
    expect(unhooks.length).toBeGreaterThan(0);
    for (const [, i] of unhooks) {
      expect(lines[i - 1]).toBe('set keep-san 0');
      expect(lines[i + 1]).toBe('set keep-san 1');
    }
  });

  it('still reports the failure to the manager before showing the menu', () => {
    const report = script.indexOf('/v1/report?');
    const menu = script.indexOf('menu ipxe_iscsi_mgr');
    expect(report).toBeGreaterThan(-1);
    expect(report).toBeLessThan(menu);
  });

  it('offers reload / check-in / memtest only when a boot server is configured', () => {
    const withoutServer = renderBootScript({
      host: { name: 'h', initiatorIqn: 'iqn.2026-02.local.client:h', arch: 'x86_64' },
      volume: primary,
    });
    for (const label of ['reload', 'forcein', 'memtest']) {
      expect(script).toContain(`item ${label}`);
      expect(withoutServer).not.toContain(`item ${label}`);
    }
    // ...and the menu is still usable without one.
    expect(withoutServer).toContain('item rebootnow');
    expect(unreachableShellLabels(withoutServer)).toEqual([]);
  });
});

describe('netboot.xyz and image selection', () => {
  const vol = (name: string, sn: string, mode: string) => ({
    name, portalHost: '10.36.75.5', portalPort: 3260, lun: 0, mode,
    targetIqn: `iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.${sn}`,
  });
  const primary = vol('minidesktop-nixos', 'minidesktop', 'pinned');
  const rescue = vol('rescue', 'rescue', 'shared-ro');
  const mk = (arch: 'x86_64' | 'arm64' | 'i386', vols: any[], bootServer: string | null = 'http://10.36.75.7') =>
    renderBootScript({
      host: { name: 'h', initiatorIqn: 'iqn.2026-02.local.client:h', arch },
      volume: vols[0], volumes: vols, mgrUrl: 'https://mgr.example', bootServer,
    });
  const script = mk('x86_64', [primary, rescue]);

  it('attaches the image BEFORE chainloading, so iBFT is published', () => {
    const block = script.slice(script.indexOf(':nbxgo'));
    const hook = block.indexOf('sanhook --drive 0x80');
    const chain = block.indexOf('chain --autofree');
    expect(hook).toBeGreaterThan(-1);
    expect(hook).toBeLessThan(chain);
  });

  it('never uses --no-describe on that hook -- that would suppress the iBFT', () => {
    expect(script.slice(script.indexOf(':nbxgo'))).not.toContain('--no-describe');
  });

  it('chains scripts, never the .efi -- the .efi starts a second iPXE that hangs', () => {
    expect(script).not.toContain('netboot.xyz.efi');
    expect(script).not.toContain('/ipxe/netboot.xyz');
  });

  it('falls back to HTTP, since some iPXE builds cannot do HTTPS', () => {
    expect(script).toContain('chain --autofree https://boot.netboot.xyz/${nbxfile} || goto nbxhttp');
    expect(script).toContain('chain --autofree http://boot.netboot.xyz/${nbxfile}');
  });

  it('does not try to serve netboot.xyz from the local mirror', () => {
    expect(script).not.toContain('10.36.75.7/netboot.xyz');
  });

  it('surfaces netboot.xyz leaves directly, so no netboot.xyz menu walking is needed', () => {
    // A distro script is self-contained given arch and space, so jumping to it
    // skips both the netboot.xyz top menu and its Linux submenu.
    for (const e of DEFAULT_NETBOOTXYZ_ENTRIES.filter((x) => !x.arch || x.arch === 'x86_64')) {
      expect(script).toContain(`item ${e.id}`);
      expect(script).toContain(`:${e.id}`);
      expect(script).toContain(`set nbxfile ${e.file}`);
    }
    expect(script).toContain('set nbxfile nixos.ipxe');
  });

  it('supplies the variables menu.ipxe would have set, since we skip it', () => {
    const go = script.slice(script.indexOf(':nbxgo'));
    expect(go).toContain('https://boot.netboot.xyz/boot.cfg');   // globals
    expect(go).toContain('cpuid --ext 29 && set arch x86_64');    // arch
    expect(go).toContain('set space:hex 20:20');                  // item padding
  });

  it('filters entries by architecture', () => {
    expect(script).toContain('set nbxfile linux.ipxe');
    expect(script).not.toContain('set nbxfile linux-arm.ipxe');
    const arm = mk('arm64', [primary]);
    expect(arm).toContain('set nbxfile linux-arm.ipxe');
    expect(arm).not.toContain('set nbxfile linux.ipxe');
  });

  it('accepts a custom entry list', () => {
    const s2 = renderBootScript({
      host: { name: 'h', initiatorIqn: 'iqn.2026-02.local.client:h', arch: 'x86_64' },
      volume: primary,
      netbootEntries: [{ id: 'nbxtalos', label: 'Talos', file: 'talos.ipxe' }],
    });
    expect(s2).toContain('item nbxtalos');
    expect(s2).toContain('set nbxfile talos.ipxe');
    expect(s2).not.toContain('nixos.ipxe');
  });

  it('offers netboot.xyz even with no boot server configured', () => {
    const s2 = mk('x86_64', [primary], null);
    expect(s2).toContain('item nbxmenu');
    expect(s2).toContain('boot.netboot.xyz');
  });

  it('offers image selection only when there is more than one image', () => {
    expect(script).toContain('item selimg');
    expect(mk('x86_64', [primary])).not.toContain('item selimg');
  });

  it('selection sets both variables that boot and netboot.xyz read', () => {
    const sel = script.slice(script.indexOf(':sel1'), script.indexOf(':selback'));
    expect(sel).toContain('set target-name rescue');
    expect(sel).toContain('set target-uri iscsi:10.36.75.5:tcp:3260:0:');
    // Selecting must not boot -- it returns to the menu, and must NOT route
    // through :failmenu, which would file a bogus sanfail report.
    expect(sel).toContain('goto menu');
    expect(sel).not.toContain('goto failmenu');
    expect(sel).not.toContain('sanboot');
  });

  it('backs out of the submenu quickly instead of holding the machine', () => {
    expect(script).toContain(`--default selback --timeout ${SUBMENU_TIMEOUT_SECONDS * 1000}`);
    expect(SUBMENU_TIMEOUT_SECONDS).toBeLessThan(FAILMENU_TIMEOUT_SECONDS);
  });

  it('gives every menu item across both menus a real label', () => {
    const labels = new Set([...script.matchAll(/^:(\w+)$/gm)].map((m) => m[1]!));
    for (const m of script.matchAll(/^item\s+(\S+)/gm)) expect(labels).toContain(m[1]!);
  });
});

describe('interrupt into the menu', () => {
  const vol = {
    name: 'minidesktop-nixos', portalHost: '10.36.75.5', portalPort: 3260, lun: 0, mode: 'pinned',
    targetIqn: 'iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.minidesktop',
  };
  const script = renderBootScript({
    host: { name: 'h', initiatorIqn: 'iqn.2026-02.local.client:h', arch: 'x86_64' },
    volume: vol, volumes: [vol], mgrUrl: 'https://mgr.example', bootServer: 'http://10.36.75.7',
  });

  it('offers a 3s window before booting', () => {
    expect(INTERRUPT_SECONDS).toBe(3);
    expect(script).toContain(`prompt --timeout ${INTERRUPT_SECONDS * 1000} `);
    expect(script).toContain('&& goto menu ||');
  });

  it('falls through and boots when nobody presses a key', () => {
    // Trailing `||` swallows the timeout failure, so the script continues into
    // :vol0. A machine with no keyboard must still boot unattended.
    const line = script.split('\n').find((l) => l.startsWith('prompt --timeout 3000'))!;
    expect(line.trimEnd().endsWith('||')).toBe(true);
    const promptAt = script.indexOf('prompt --timeout 3000');
    expect(script.indexOf(':vol0')).toBeGreaterThan(promptAt);
  });

  it('pre-selects the image BEFORE the interrupt, so the menu is usable', () => {
    // Entering the menu this way must not leave ${target-uri} unset, or every
    // menu action that attaches a LUN would hook an empty target.
    const promptAt = script.indexOf('prompt --timeout 3000');
    expect(script.lastIndexOf('set target-uri iscsi:', promptAt)).toBeGreaterThan(-1);
    expect(script.lastIndexOf('set target-name ', promptAt)).toBeGreaterThan(-1);
  });

  it('separates the failure entry from the menu itself', () => {
    expect(script).toContain(':failmenu');
    expect(script).toContain(':menu');
    // The failure entry reports, then hands over to the shared menu.
    const fail = script.slice(script.indexOf(':failmenu'), script.indexOf(':menu'));
    expect(fail).toContain('Boot failed for');
    expect(fail).toContain('/v1/report?');
    expect(fail.trimEnd().endsWith('goto menu')).toBe(true);
  });

  it('does not file a failure report when the menu is entered deliberately', () => {
    // Only the three genuine attach/boot failures may route via :failmenu.
    const failGotos = [...script.matchAll(/goto failmenu/g)];
    expect(failGotos).toHaveLength(3);
    for (const line of script.split('\n').filter((l) => l.includes('goto failmenu'))) {
      expect(line).toMatch(/^sanhook |^sanboot /);
    }
  });

  it('titles the menu neutrally, since it is not only a failure menu', () => {
    expect(script).toContain('menu ipxe_iscsi_mgr -- h');
    expect(script).not.toContain('boot failed on');
  });
});

describe('firmware update menu', () => {
  const vol = {
    name: 'minidesktop-nixos', portalHost: '10.36.75.5', portalPort: 3260, lun: 0, mode: 'pinned',
    targetIqn: 'iqn.2003-01.org.linux-iscsi.sea69-hv-puppy.x8664:sn.minidesktop',
  };
  const fw = [
    { id: 'a', vendor: 'BESSTAR TECH LIMITED', model: 'UM350', version: 'AF5PN06', label: 'AF5PN06 BIOS', r2Key: 'besstar/um350/af5pn06.img' },
    { id: 'b', vendor: 'BESSTAR TECH LIMITED', model: 'UM790', version: 'V1.2', r2Key: 'besstar/um790/v12.img' },
    { id: 'c', vendor: 'Dell Inc.', model: 'OptiPlex', version: '1.9.0', r2Key: 'dell/optiplex/190.img' },
  ];
  const mk = (manufacturer?: string, product?: string) => renderBootScript({
    host: { name: 'h', initiatorIqn: 'iqn.2026-02.local.client:h', arch: 'x86_64', manufacturer, product },
    volume: vol, volumes: [vol], mgrUrl: 'https://mgr.example', bootServer: 'http://10.36.75.7', firmware: fw,
  });
  const script = mk('BESSTAR TECH LIMITED', 'UM350');

  it('builds the vendor -> model -> image tree', () => {
    expect(script).toContain('menu Firmware Updates');
    expect(script).toContain('item fwv0   BESSTAR TECH LIMITED');
    expect(script).toContain('item fwv1   Dell Inc.');
    expect(script).toContain('item fwm0_0   UM350');
    expect(script).toContain('item fwm0_1   UM790');
    expect(script).toContain('item fwi0   AF5PN06 BIOS');
  });

  it('offers the auto-detected system and jumps straight to its model', () => {
    expect(script).toContain('item fwauto   Auto-detected system (BESSTAR TECH LIMITED UM350)');
    const auto = script.slice(script.indexOf(':fwauto'));
    expect(auto.split('\n')[1]).toBe('goto fwm0_0');
  });

  it('matches SMBIOS case- and whitespace-insensitively', () => {
    expect(mk('  besstar tech limited ', 'um350')).toContain('item fwauto');
  });

  it('omits auto-detect when nothing matches this host', () => {
    expect(mk('Acme', 'Nothing')).not.toContain('item fwauto');
    expect(mk(undefined, undefined)).not.toContain('item fwauto');
    // ...but the vendor tree is still browsable.
    expect(mk('Acme', 'Nothing')).toContain('menu Firmware Updates');
  });

  it('hides the whole menu when no firmware is registered', () => {
    const bare = renderBootScript({
      host: { name: 'h', initiatorIqn: 'iqn.2026-02.local.client:h', arch: 'x86_64' },
      volume: vol, mgrUrl: 'https://mgr.example',
    });
    expect(bare).not.toContain('item fwmenu');
    expect(bare).not.toContain(':fwmenu');
  });

  it('detaches the iSCSI LUN before flashing', () => {
    // Holding a network block device open while the BIOS is rewritten is a bad
    // idea; the unhook must come before the sanboot.
    const item = script.slice(script.indexOf(':fwi0'), script.indexOf(':fwi1'));
    expect(item.indexOf('sanunhook')).toBeLessThan(item.indexOf('sanboot'));
    expect(item).toContain('set keep-san 0');
  });

  it('boots the image with --no-describe -- it is not an OS root', () => {
    expect(script).toContain('sanboot --no-describe --drive 0x80 https://mgr.example/fw/besstar/um350/af5pn06.img');
  });

  it('serves images from the unauthenticated /fw path, not a signed URL', () => {
    expect(script).toContain('https://mgr.example/fw/');
    expect(script).not.toMatch(/X-Amz-|Signature=|\?token=/);
  });

  it('gives every firmware menu item a real label', () => {
    const labels = new Set([...script.matchAll(/^:(\w+)$/gm)].map((m) => m[1]!));
    for (const m of script.matchAll(/^item (fw\w+)/gm)) expect(labels).toContain(m[1]!);
  });

  it('keeps every submenu escapable and timed', () => {
    for (const line of script.split('\n').filter((l) => l.startsWith('choose'))) {
      expect(line).toContain('--timeout');
      expect(line).toContain('--default');
    }
  });
});

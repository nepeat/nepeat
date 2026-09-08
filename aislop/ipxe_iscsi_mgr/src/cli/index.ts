#!/usr/bin/env tsx
import { api, ApiError } from './api.js';
import { loadConfig, type Config } from './config.js';
import { runCheck, type Finding } from './check.js';
import { renderChainScript, renderCheckinScript } from '../shared/ipxe.js';
import { listCols, renderTable } from './format.js';

interface Args {
  positionals: string[];
  flags: Record<string, string | boolean>;
}

function parseArgs(argv: string[]): Args {
  const positionals: string[] = [];
  const flags: Record<string, string | boolean> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]!;
    if (!a.startsWith('--')) { positionals.push(a); continue; }
    const eq = a.indexOf('=');
    if (eq > -1) { flags[a.slice(2, eq)] = a.slice(eq + 1); continue; }
    const next = argv[i + 1];
    if (next !== undefined && !next.startsWith('--')) { flags[a.slice(2)] = next; i++; }
    else flags[a.slice(2)] = true;
  }
  return { positionals, flags };
}

const str = (v: string | boolean | undefined): string | undefined =>
  typeof v === 'string' ? v : undefined;

const USAGE = `ipxectl -- manage iSCSI boot volumes

  host    list [--state S] [--long] | pending [--long] | show <h>
          approve <h> | disable <h>
          rename <h> <new> | set-iqn <h> <iqn> | set-profile <h> <profile>
  volume  list [--long] | add <name> --zvol P --target-iqn I --portal H[:PORT] [--lun N] [--size BYTES]
          rm <v>
  assign  <v> --host H | --general | --shared-ro   [--priority N] [--force]
  release <v>
  profile list | add <name> --arch A --candidates 'a,b,c'
  render  <h>                      what that host would receive, without booting it
  events  [--limit N]
  secret  add <secret> [--label L]
  firmware list | add <vendor> <model> <version> --key R2KEY [--label L] [--notes N]
           rm <id|r2key>
  check   [--ssh user@host]        validate ZFS/LIO against what the manager believes
  gen-chain [--fallback URL]       emit the netboot server's chain script
  gen-checkin                      emit the netboot server's check-in script

Global flags:
  --json    machine-readable JSON on stdout (for scripts and agents)
  --long    show every stored field in list views
  --help    per-command usage, e.g. ipxectl host list --help

Config comes from .dev.vars or the environment: MGR_URL, ADMIN_TOKEN,
optionally STORAGE_SSH (e.g. root@10.36.75.5) and BOOT_SECRET.`;

/**
 * Render a result set. --json wins over --long, because JSON always carries
 * every field anyway. JSON goes to stdout alone so it can be piped straight
 * into a parser.
 */
function output(data: unknown, rows: Record<string, unknown>[], cols: string[], args: Args): void {
  if (args.flags.json) console.log(JSON.stringify(data, null, 2));
  else console.log(renderTable(rows, cols));
}

interface CommandHelp {
  usage: string;
  summary: string;
  flags?: [string, string][];
  examples?: string[];
  notes?: string;
}

/**
 * Per-subcommand help, keyed by the command path. `--help` is intercepted
 * centrally before dispatch, so adding an entry here is all a new command
 * needs.
 */
const COMMAND_HELP: Record<string, CommandHelp> = {
  'host list': {
    usage: 'ipxectl host list [--state STATE] [--long]',
    summary: 'List known hosts.',
    flags: [
      ['--state STATE', 'only hosts in this state: pending | approved | disabled'],
      ['--long', 'show every stored field, not just the common ones'],
      ['--json', 'machine-readable JSON on stdout (implies every field)'],
      ['--help', 'show this help'],
    ],
    examples: [
      'ipxectl host list',
      'ipxectl host list --state pending',
      'ipxectl host list --long',
      'ipxectl host list --long | less -S      # --long is wide; -S stops wrapping',
      'ipxectl host list --json | jq -r .[].name',
    ],
    notes:
      'Default columns are name, state, arch, product, cpu_model, mem_mb,\n' +
      'mem_source and boot_count. --long adds ids, IQNs, SMBIOS UUID, serial,\n' +
      'asset tag, timestamps and notes. mem_source says where mem_mb came from:\n' +
      "'memsize' is measured, anything smbios* is an approximation.\n" +
      '--json emits the raw records (JSON always carries every field, so --long\n' +
      'is redundant with it) and nothing else on stdout.',
  },
  'host pending': {
    usage: 'ipxectl host pending [--long]',
    summary: 'List hosts that have checked in but are not approved yet.',
    flags: [['--long', 'show every stored field'], ['--json', 'machine-readable JSON'], ['--help', 'show this help']],
    examples: ['ipxectl host pending', 'ipxectl host pending --long'],
  },
  'volume list': {
    usage: 'ipxectl volume list [--long]',
    summary: 'List volumes and how they are assigned.',
    flags: [['--long', 'show every stored field'], ['--json', 'machine-readable JSON'], ['--help', 'show this help']],
    examples: ['ipxectl volume list', 'ipxectl volume list --long'],
  },
  check: {
    usage: 'ipxectl check [--ssh user@host]',
    summary: 'Validate ZFS/LIO on the storage host against what the manager believes.',
    flags: [
      ['--ssh user@host', 'storage host to read from (default: STORAGE_SSH in .dev.vars)'],
      ['--json', 'emit {ok, fails, warns, findings} as JSON (for CI and agents)'],
      ['--help', 'show this help'],
    ],
    notes: 'Exits non-zero if anything fails. Failures print the targetcli command that fixes them.',
  },
};

function printCommandHelp(key: string): void {
  const h = COMMAND_HELP[key];
  if (!h) { console.log(USAGE); return; }
  console.log(`${h.summary}\n`);
  console.log(`usage: ${h.usage}`);
  if (h.flags?.length) {
    console.log('\nflags:');
    const w = Math.max(...h.flags.map(([f]) => f.length));
    for (const [f, d] of h.flags) console.log(`  ${f.padEnd(w)}  ${d}`);
  }
  if (h.examples?.length) {
    console.log('\nexamples:');
    for (const e of h.examples) console.log(`  ${e}`);
  }
  if (h.notes) console.log(`\n${h.notes}`);
}

async function cmdCheck(cfg: Config, args: Args): Promise<number> {
  const findings = await runCheck(cfg, { ssh: str(args.flags.ssh) });
  const fails = findings.filter((f) => f.level === 'fail');
  const warns = findings.filter((f) => f.level === 'warn');

  if (args.flags.json) {
    console.log(JSON.stringify({ ok: fails.length === 0, fails: fails.length, warns: warns.length, findings }, null, 2));
    return fails.length ? 1 : 0;
  }

  for (const f of findings) {
    const tag = f.level === 'fail' ? 'FAIL' : 'WARN';
    console.log(`${tag}  ${f.what}: ${f.detail}`);
    if (f.fix) console.log(`      fix: ${f.fix}`);
  }
  if (!findings.length) console.log('OK    everything the manager believes matches the storage host');
  else console.log(`\n${fails.length} failure(s), ${warns.length} warning(s)`);
  return fails.length ? 1 : 0;
}

/** Mutating commands run check first: the CLI is the only validation gate. */
async function gate(cfg: Config, args: Args, what: string): Promise<boolean> {
  if (args.flags.force) {
    console.log(`(--force: skipping validation for ${what})`);
    return true;
  }
  const findings = (await runCheck(cfg, { ssh: str(args.flags.ssh) })).filter((f) => f.level === 'fail');
  if (!findings.length) return true;
  console.error(`refusing to ${what}: storage state has ${findings.length} failure(s)`);
  for (const f of findings) console.error(`  FAIL  ${f.what}: ${f.detail}`);
  console.error('run `ipxectl check` for detail, or pass --force to proceed anyway');
  return false;
}

async function main(): Promise<number> {
  const argv = process.argv.slice(2);
  if (!argv.length || argv[0] === 'help' || argv[0] === '--help') { console.log(USAGE); return 0; }

  const args = parseArgs(argv);
  const [group, sub, ...rest] = args.positionals;

  // Intercept --help before loadConfig(), so `--help` works without credentials.
  if (args.flags.help) {
    const key = [group, sub].filter(Boolean).join(' ');
    printCommandHelp(COMMAND_HELP[key] ? key : (group && COMMAND_HELP[group] ? group : ''));
    return 0;
  }

  const cfg = loadConfig();

  switch (group) {
    case 'host': {
      switch (sub) {
        case 'list':
        case undefined: {
          const q = str(args.flags.state) ? `?state=${args.flags.state}` : '';
          const { hosts } = await api(cfg, 'GET', `/v1/hosts${q}`);
          output(hosts, hosts, listCols(hosts,
            ['name', 'state', 'arch', 'product', 'cpu_model', 'mem_mb', 'mem_source', 'boot_count'],
            args.flags.long === true), args);
          return 0;
        }
        case 'pending': {
          const { hosts } = await api(cfg, 'GET', '/v1/hosts?state=pending');
          output(hosts, hosts, listCols(hosts,
            ['name', 'serial', 'product', 'manufacturer', 'mem_mb', 'initiator_iqn'],
            args.flags.long === true), args);
          return 0;
        }
        case 'show': {
          // Always JSON: a host record is nested (macs, assignment) and does
          // not fit a table. --json is therefore a no-op here.
          console.log(JSON.stringify(await api(cfg, 'GET', `/v1/hosts/${rest[0]}`), null, 2));
          return 0;
        }
        case 'approve':
        case 'disable': {
          const state = sub === 'approve' ? 'approved' : 'disabled';
          await api(cfg, 'PATCH', `/v1/hosts/${rest[0]}`, { state });
          console.log(`${rest[0]} -> ${state}`);
          if (state === 'approved') {
            console.log('note: the LIO ACL is separate -- run `ipxectl check` to confirm it can actually boot');
          }
          return 0;
        }
        case 'rename':
          await api(cfg, 'PATCH', `/v1/hosts/${rest[0]}`, { name: rest[1] });
          console.log(`${rest[0]} -> ${rest[1]}`); return 0;
        case 'set-iqn':
          await api(cfg, 'PATCH', `/v1/hosts/${rest[0]}`, { initiator_iqn: rest[1] });
          console.log('note: the LIO ACL must be recreated for the new IQN'); return 0;
        case 'set-profile':
          await api(cfg, 'PATCH', `/v1/hosts/${rest[0]}`, { boot_profile_id: rest[1] });
          return 0;
        default: console.error(`unknown: host ${sub}`); return 2;
      }
    }

    case 'volume': {
      switch (sub) {
        case 'list':
        case undefined: {
          const { volumes } = await api(cfg, 'GET', '/v1/volumes');
          output(volumes, volumes, listCols(volumes,
            ['name', 'zvol_path', 'portal_host', 'lun', 'mode', 'state', 'target_iqn'],
            args.flags.long === true), args);
          return 0;
        }
        case 'add': {
          const [host, port] = (str(args.flags.portal) ?? '').split(':');
          const body = {
            name: rest[0],
            zvolPath: str(args.flags.zvol),
            targetIqn: str(args.flags['target-iqn']),
            portalHost: host,
            portalPort: port ? Number(port) : 3260,
            lun: Number(str(args.flags.lun) ?? 0),
            sizeBytes: str(args.flags.size) ? Number(args.flags.size) : null,
          };
          if (!body.name || !body.zvolPath || !body.targetIqn || !body.portalHost) {
            console.error('need: <name> --zvol <path> --target-iqn <iqn> --portal <host[:port]>'); return 2;
          }
          const r = await api(cfg, 'POST', '/v1/volumes', body);
          console.log(`created ${body.name} (${r.id})`);
          console.log('run `ipxectl check` to confirm it matches the storage host');
          return 0;
        }
        case 'rm':
          await api(cfg, 'DELETE', `/v1/volumes/${rest[0]}`);
          console.log(`removed ${rest[0]}`); return 0;
        default: console.error(`unknown: volume ${sub}`); return 2;
      }
    }

    case 'assign': {
      const volume = sub;
      const host = str(args.flags.host);
      const mode = args.flags['shared-ro'] ? 'shared-ro' : args.flags.general ? 'general-exclusive' : 'pinned';
      if (!volume) { console.error('usage: ipxectl assign <volume> --host H | --general | --shared-ro'); return 2; }
      if (mode === 'pinned' && !host) { console.error('a pinned assignment needs --host'); return 2; }
      if (!(await gate(cfg, args, `assign ${volume}`))) return 1;
      await api(cfg, 'POST', '/v1/assignments', {
        volume, mode, host,
        priority: str(args.flags.priority) ? Number(args.flags.priority) : undefined,
        forced: args.flags.force === true,
      });
      console.log(`${volume} -> ${mode}${host ? ` (${host})` : ''}`);
      return 0;
    }

    case 'release':
      await api(cfg, 'POST', `/v1/assignments/${sub}/release`, {});
      console.log(`released lease on ${sub}`); return 0;

    case 'profile': {
      if (sub === 'add') {
        const candidates = (str(args.flags.candidates) ?? '').split(',').map((s) => s.trim()).filter(Boolean);
        if (!rest[0] && !sub) { console.error('usage: ipxectl profile add <name> --arch A --candidates a,b'); return 2; }
        await api(cfg, 'POST', '/v1/profiles', {
          name: rest[0], arch: str(args.flags.arch) ?? 'x86_64', efiCandidates: candidates,
        });
        console.log(`profile ${rest[0]} saved`); return 0;
      }
      const { profiles } = await api(cfg, 'GET', '/v1/profiles');
      output(profiles, profiles, ['name', 'arch', 'efi_candidates'], args);
      return 0;
    }

    case 'render':
      process.stdout.write(await api(cfg, 'GET', `/v1/render/${sub}`));
      return 0;

    case 'events': {
      const { events } = await api(cfg, 'GET', `/v1/events?limit=${str(args.flags.limit) ?? 20}`);
      // `ts` is unix seconds; renderTable humanises it, so just pass it through.
      output(events, events, listCols(events,
        ['ts', 'outcome', 'host_name', 'volume_name'], args.flags.long === true), args);
      return 0;
    }

    case 'firmware': {
      switch (sub) {
        case 'list':
        case undefined: {
          const { firmware } = await api(cfg, 'GET', '/v1/firmware');
          output(firmware, firmware, listCols(firmware,
            ['vendor', 'model', 'version', 'label', 'r2_key', 'size_bytes'],
            args.flags.long === true), args);
          return 0;
        }
        case 'add': {
          const [vendor, model, version] = rest;
          const key = str(args.flags.key);
          if (!vendor || !model || !version || !key) {
            console.error('usage: ipxectl firmware add <vendor> <model> <version> --key <r2key>');
            return 2;
          }
          // The Worker stores rows without checking the blob exists. Verify it
          // here, or the menu offers an image that 404s at sanboot time.
          const url = `${cfg.mgrUrl}/fw/${key}`;
          const head = await fetch(url, { method: 'HEAD' });
          if (!head.ok) {
            console.error(`refusing to register: ${url} returned HTTP ${head.status}`);
            console.error('upload it first:  wrangler r2 object put ipxe-firmware/<key> --file <img> --remote');
            return 1;
          }
          const size = Number(head.headers.get('content-length') ?? 0);
          if (head.headers.get('accept-ranges') !== 'bytes') {
            console.error('warning: the blob is served without Range support; sanboot will be slow');
          }
          const r = await api(cfg, 'POST', '/v1/firmware', {
            vendor, model, version, r2Key: key,
            label: str(args.flags.label), notes: str(args.flags.notes), sizeBytes: size,
          });
          console.log(`registered ${vendor} ${model} ${version} (${r.id}, ${size} bytes)`);
          return 0;
        }
        case 'rm':
          await api(cfg, 'DELETE', `/v1/firmware/${rest[0]}`);
          console.log(`removed ${rest[0]}`); return 0;
        default: console.error(`unknown: firmware ${sub}`); return 2;
      }
    }

    case 'secret': {
      if (sub !== 'add' || !rest[0]) { console.error('usage: ipxectl secret add <secret> [--label L]'); return 2; }
      await api(cfg, 'POST', '/v1/secrets', { secret: rest[0], label: str(args.flags.label) });
      console.log('secret registered; regenerate mgr.ipxe with `ipxectl gen-chain`');
      return 0;
    }

    case 'check':
      return cmdCheck(cfg, args);

    case 'gen-checkin': {
      if (!cfg.bootSecret) { console.error('BOOT_SECRET not set in .dev.vars'); return 2; }
      process.stdout.write(renderCheckinScript(cfg.mgrUrl, { secret: cfg.bootSecret }));
      return 0;
    }

    case 'gen-chain': {
      if (!cfg.bootSecret) { console.error('BOOT_SECRET not set in .dev.vars'); return 2; }
      process.stdout.write(renderChainScript(cfg.mgrUrl, {
        secret: cfg.bootSecret,
        fallbackUrl: str(args.flags.fallback) ?? null,
      }));
      return 0;
    }

    default:
      console.error(`unknown command: ${group}\n`);
      console.log(USAGE);
      return 2;
  }
}

main()
  .then((code) => process.exit(code))
  .catch((err) => {
    if (err instanceof ApiError) console.error(`api error: ${err.message}`);
    else console.error(String(err?.message ?? err));
    process.exit(1);
  });

import { execFileSync } from 'node:child_process';
import { connect } from 'node:net';
import { api } from './api.js';
import type { Config } from './config.js';
import { iqnSchema } from '../shared/types.js';

/**
 * One shell script, one round trip. Reads configfs directly rather than parsing
 * `targetcli` output, which is a human-facing tree that changes between
 * versions. Emits pipe-delimited records for the TypeScript side to correlate.
 */
const PROBE = `
set -u
B=/sys/kernel/config/target
for d in "$B"/iscsi/iqn.*; do
  [ -d "$d" ] || continue
  iqn=$(basename "$d")
  for tpg in "$d"/tpgt_*; do
    [ -d "$tpg" ] || continue
    echo "TARGET|$iqn|$(cat "$tpg/enable" 2>/dev/null || echo 0)"
    for np in "$tpg"/np/*; do [ -d "$np" ] && echo "PORTAL|$iqn|$(basename "$np")"; done
    for l in "$tpg"/lun/lun_*; do
      [ -d "$l" ] || continue
      n=\${l##*lun_}; bs=""
      for s in "$l"/*; do [ -L "$s" ] && bs=$(readlink -f "$s") && break; done
      up=""
      [ -n "$bs" ] && up=$(cat "$bs/udev_path" 2>/dev/null || echo "")
      echo "LUN|$iqn|$n|$up"
    done
    for a in "$tpg"/acls/*; do
      [ -d "$a" ] || continue
      init=$(basename "$a")
      echo "ACLNODE|$iqn|$init"
      for ml in "$a"/lun_*; do
        [ -d "$ml" ] || continue
        echo "ACL|$iqn|$init|\${ml##*lun_}|$(cat "$ml/write_protect" 2>/dev/null || echo '?')"
      done
    done
  done
done
zfs list -Hp -o name,volsize -t volume 2>/dev/null | while read -r n v; do echo "ZVOL|$n|$v"; done
`;

export interface Finding {
  level: 'fail' | 'warn';
  what: string;
  detail: string;
  fix?: string;
}

interface State {
  targets: Map<string, string>;            // iqn -> enable
  portals: Map<string, string[]>;          // iqn -> ["0.0.0.0:3260"]
  luns: Map<string, string>;               // "iqn|n" -> udev_path
  aclNodes: Set<string>;                   // "iqn|initiator"
  acls: Map<string, string>;               // "iqn|initiator|n" -> write_protect
  zvols: Map<string, number>;              // dataset -> volsize bytes
}

function probe(ssh?: string): State {
  const out = ssh
    ? execFileSync('ssh', [ssh, PROBE], { encoding: 'utf8', maxBuffer: 16 << 20 })
    : execFileSync('sh', ['-c', PROBE], { encoding: 'utf8', maxBuffer: 16 << 20 });

  const s: State = {
    targets: new Map(), portals: new Map(), luns: new Map(),
    aclNodes: new Set(), acls: new Map(), zvols: new Map(),
  };
  for (const line of out.split('\n')) {
    const p = line.trim().split('|');
    if (p[0] === 'TARGET') s.targets.set(p[1]!, p[2] ?? '0');
    else if (p[0] === 'PORTAL') s.portals.set(p[1]!, [...(s.portals.get(p[1]!) ?? []), p[2]!]);
    else if (p[0] === 'LUN') s.luns.set(`${p[1]}|${p[2]}`, p[3] ?? '');
    else if (p[0] === 'ACLNODE') s.aclNodes.add(`${p[1]}|${p[2]}`);
    else if (p[0] === 'ACL') s.acls.set(`${p[1]}|${p[2]}|${p[3]}`, p[4] ?? '?');
    else if (p[0] === 'ZVOL') s.zvols.set(p[1]!, Number(p[2] ?? 0));
  }
  return s;
}

function tcpReachable(host: string, port: number, timeoutMs = 3000): Promise<boolean> {
  return new Promise((res) => {
    const sock = connect({ host, port });
    const done = (ok: boolean) => { sock.destroy(); res(ok); };
    sock.setTimeout(timeoutMs);
    sock.once('connect', () => done(true));
    sock.once('timeout', () => done(false));
    sock.once('error', () => done(false));
  });
}

/**
 * Validate that what the Worker believes is actually true on the storage host.
 * The Worker never checks any of this -- it stores rows -- so this is the only
 * thing standing between a typo and a machine that will not boot.
 */
export async function runCheck(cfg: Config, opts: { ssh?: string } = {}): Promise<Finding[]> {
  const findings: Finding[] = [];
  const ssh = opts.ssh ?? cfg.storageSsh;

  const { volumes } = await api<{ volumes: any[] }>(cfg, 'GET', '/v1/volumes');
  const { hosts } = await api<{ hosts: any[] }>(cfg, 'GET', '/v1/hosts');
  const hostById = new Map(hosts.map((h) => [h.id, h]));

  let state: State;
  try {
    state = probe(ssh);
  } catch (err) {
    return [{
      level: 'fail',
      what: 'storage probe',
      detail: `could not read ZFS/LIO state${ssh ? ` on ${ssh}` : ' locally'}: ${String(err).split('\n')[0]}`,
      fix: 'set STORAGE_SSH=root@<zfs-host> in .dev.vars, or run ipxectl check on the storage host',
    }];
  }

  // --- syntax and uniqueness, checkable without the storage host -----------
  const seenZvol = new Map<string, string>();
  for (const v of volumes) {
    if (!iqnSchema.safeParse(v.target_iqn).success) {
      findings.push({ level: 'fail', what: `volume ${v.name}`, detail: `malformed target IQN: ${v.target_iqn}` });
    }
    const prev = seenZvol.get(v.zvol_path);
    if (prev) {
      findings.push({
        level: 'fail', what: `volume ${v.name}`,
        detail: `zvol ${v.zvol_path} is already backing volume ${prev} -- two targets on one zvol will corrupt it`,
      });
    }
    seenZvol.set(v.zvol_path, v.name);
  }
  for (const h of hosts) {
    if (!iqnSchema.safeParse(h.initiator_iqn).success) {
      findings.push({ level: 'fail', what: `host ${h.name}`, detail: `malformed initiator IQN: ${h.initiator_iqn}` });
    }
  }

  // --- per volume ----------------------------------------------------------
  for (const v of volumes) {
    const tag = `volume ${v.name}`;
    const dataset = v.zvol_path.replace(/^\/dev\/zvol\//, '');
    const volsize = state.zvols.get(dataset);

    if (volsize === undefined) {
      findings.push({ level: 'fail', what: tag, detail: `zvol ${v.zvol_path} does not exist on the storage host` });
    } else if (v.size_bytes && Number(v.size_bytes) !== volsize) {
      findings.push({
        level: 'warn', what: tag,
        detail: `recorded size ${v.size_bytes} != actual volsize ${volsize}`,
      });
    }

    if (!state.targets.has(v.target_iqn)) {
      findings.push({ level: 'fail', what: tag, detail: `no LIO target ${v.target_iqn}` });
      continue;
    }
    if (state.targets.get(v.target_iqn) !== '1') {
      findings.push({
        level: 'fail', what: tag, detail: `LIO target ${v.target_iqn} is disabled`,
        fix: `targetcli /iscsi/${v.target_iqn}/tpg1 set attribute enable=1`,
      });
    }
    if (!(state.portals.get(v.target_iqn) ?? []).length) {
      findings.push({ level: 'fail', what: tag, detail: `target ${v.target_iqn} has no portal` });
    }

    const lunKey = `${v.target_iqn}|${v.lun}`;
    if (!state.luns.has(lunKey)) {
      findings.push({
        level: 'fail', what: tag, detail: `LUN ${v.lun} is not mapped in ${v.target_iqn}`,
        fix: `targetcli /iscsi/${v.target_iqn}/tpg1/luns create /backstores/block/<backstore>`,
      });
    } else {
      const udev = state.luns.get(lunKey)!;
      if (udev !== v.zvol_path) {
        findings.push({
          level: 'fail', what: tag,
          detail: `LUN ${v.lun} is backed by ${udev || '(unknown)'}, but the manager says ${v.zvol_path}`,
        });
      }
    }

    if (!(await tcpReachable(v.portal_host, Number(v.portal_port)))) {
      findings.push({
        level: 'warn', what: tag,
        detail: `portal ${v.portal_host}:${v.portal_port} not reachable from here (may still be reachable from the boot VLAN)`,
      });
    }

    // --- assignment-dependent checks --------------------------------------
    if (v.mode === 'pinned' && v.host_id) {
      const h = hostById.get(v.host_id);
      if (h) {
        const nodeKey = `${v.target_iqn}|${h.initiator_iqn}`;
        const aclKey = `${nodeKey}|${v.lun}`;
        if (!state.aclNodes.has(nodeKey)) {
          findings.push({
            level: 'fail', what: `${tag} -> ${h.name}`,
            detail: `no LIO ACL for ${h.initiator_iqn}; the host will log in and see zero LUNs`,
            fix: `targetcli /iscsi/${v.target_iqn}/tpg1/acls create ${h.initiator_iqn} && targetcli saveconfig`,
          });
        } else if (!state.acls.has(aclKey)) {
          findings.push({
            level: 'fail', what: `${tag} -> ${h.name}`,
            detail: `ACL for ${h.initiator_iqn} exists but LUN ${v.lun} is not mapped into it`,
            fix: `targetcli /iscsi/${v.target_iqn}/tpg1/acls/${h.initiator_iqn} create ${v.lun} ${v.lun}`,
          });
        }
      }
    }

    if (v.mode === 'shared-ro') {
      const wp = [...state.acls.entries()].filter(([k]) => k.startsWith(`${v.target_iqn}|`) && k.endsWith(`|${v.lun}`));
      const writable = wp.filter(([, val]) => val !== '1');
      if (!wp.length) {
        findings.push({
          level: 'fail', what: tag,
          detail: 'declared shared-ro but has no mapped ACLs to verify read-only against',
        });
      }
      for (const [k] of writable) {
        findings.push({
          level: 'fail', what: tag,
          detail: `declared shared-ro but ${k.split('|')[1]} has write access -- concurrent writers will corrupt it`,
          fix: `targetcli /iscsi/${v.target_iqn}/tpg1/acls/${k.split('|')[1]} set attribute write_protect=1`,
        });
      }
    }
  }

  return findings;
}

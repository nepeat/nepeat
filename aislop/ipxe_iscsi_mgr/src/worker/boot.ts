import type { BootFacts } from '../shared/types.js';
import type { Env } from './env.js';
import { cleanSmbios, decodeMem, id, normalizeMac, normalizeUuid, now } from './env.js';

export interface HostRow {
  id: string;
  name: string;
  state: string;
  arch: string;
  initiator_iqn: string;
  boot_profile_id: string | null;
  smbios_uuid: string | null;
  // Carried so the boot script can auto-detect a matching firmware image.
  manufacturer: string | null;
  product: string | null;
}

export interface VolumeRow {
  id: string;
  name: string;
  portal_host: string;
  portal_port: number;
  lun: number;
  target_iqn: string;
}

export interface ProfileRow {
  id: string;
  efi_candidates: string;
  extra_ipxe: string | null;
}

/**
 * Identity matching, strongest key first. SMBIOS UUID is stable across NIC
 * changes; MAC is the weakest because NICs move between chassis and a
 * multi-port machine presents whichever port happened to boot.
 */
export async function findHost(env: Env, facts: BootFacts): Promise<HostRow | null> {
  const uuid = normalizeUuid(facts.uuid);
  const serial = cleanSmbios(facts.serial);
  const mac = normalizeMac(facts.mac);
  const cols = `id, name, state, arch, initiator_iqn, boot_profile_id, smbios_uuid, manufacturer, product`;

  if (uuid) {
    const row = await env.DB.prepare(`SELECT ${cols} FROM hosts WHERE smbios_uuid = ?1`)
      .bind(uuid)
      .first<HostRow>();
    if (row) return row;
  }
  if (serial) {
    const row = await env.DB.prepare(`SELECT ${cols} FROM hosts WHERE serial = ?1`)
      .bind(serial)
      .first<HostRow>();
    if (row) return row;
  }
  if (mac) {
    const row = await env.DB.prepare(
      `SELECT ${cols.split(', ').map((c) => 'h.' + c).join(', ')}
         FROM hosts h JOIN host_macs m ON m.host_id = h.id
        WHERE m.mac = ?1`,
    )
      .bind(mac)
      .first<HostRow>();
    if (row) return row;
  }
  return null;
}

/** A stable, human-usable name for a machine we have never seen. */
function deriveName(facts: BootFacts, uuid: string | null, mac: string | null): string {
  const product = cleanSmbios(facts.product);
  const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 24);
  const suffix = (uuid?.slice(0, 8) ?? mac?.replace(/:/g, '').slice(-6) ?? crypto.randomUUID().slice(0, 8));
  return product ? `${slug(product)}-${suffix}` : `host-${suffix}`;
}

/**
 * Record an unknown machine as `pending`. Registration is a side effect of
 * booting, so an unrecognised box shows up in `ipxectl host pending` with its
 * inventory already filled in.
 */
export async function registerHost(env: Env, facts: BootFacts): Promise<HostRow> {
  const uuid = normalizeUuid(facts.uuid);
  const mac = normalizeMac(facts.mac);
  const ts = now();
  const hostId = id('h');
  let name = deriveName(facts, uuid, mac);
  const arch = facts.arch ?? 'x86_64';
  const platform = facts.platform ?? 'efi';
  const mem = decodeMem(facts);

  for (let attempt = 0; attempt < 5; attempt++) {
    const candidate = attempt === 0 ? name : `${name}-${attempt}`;
    const iqn = `${env.INITIATOR_IQN_PREFIX}:${candidate}`;
    try {
      await env.DB.prepare(
        `INSERT INTO hosts (id, name, state, smbios_uuid, serial, asset_tag, manufacturer, product,
                            cpu_model, mem_mb, arch, platform, initiator_iqn, first_seen, last_seen, boot_count,
                            mem_source, cpu_vendor)
         VALUES (?1,?2,'pending',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13,0,?14,?15)`,
      )
        .bind(
          hostId,
          candidate,
          uuid,
          cleanSmbios(facts.serial),
          cleanSmbios(facts.asset),
          cleanSmbios(facts.manufacturer),
          cleanSmbios(facts.product),
          cleanSmbios(facts.cpu),
          mem.mb,
          arch,
          platform,
          iqn,
          ts,
          mem.source,
          cleanSmbios(facts.cpuvendor),
        )
        .run();
      name = candidate;
      if (mac) {
        await env.DB.prepare(
          `INSERT INTO host_macs (mac, host_id, first_seen) VALUES (?1,?2,?3)
           ON CONFLICT(mac) DO UPDATE SET host_id = excluded.host_id`,
        )
          .bind(mac, hostId, ts)
          .run();
      }
      return {
        id: hostId,
        name: candidate,
        state: 'pending',
        arch,
        initiator_iqn: iqn,
        boot_profile_id: null,
        smbios_uuid: uuid,
        manufacturer: cleanSmbios(facts.manufacturer),
        product: cleanSmbios(facts.product),
      };
    } catch (err) {
      // Name/IQN collision -- retry with a suffix. Anything else is real.
      if (!String(err).includes('UNIQUE')) throw err;
    }
  }
  throw new Error('could not allocate a unique host name');
}


/**
 * Refresh a known host's inventory from a fresh set of facts.
 *
 * COALESCE on every field so a fact that came back empty this boot (SMBIOS is
 * inconsistent about which fields it populates) never wipes a good stored
 * value. `countBoot` distinguishes an actual boot from an inventory check-in --
 * a check-in must not inflate boot_count.
 */
export async function refreshHost(
  env: Env,
  host: HostRow,
  facts: BootFacts,
  opts: { countBoot?: boolean } = {},
): Promise<void> {
  const ts = now();
  const mem = decodeMem(facts);
  const mac = normalizeMac(facts.mac);

  const stmts = [
    env.DB.prepare(
      `UPDATE hosts SET
         serial       = COALESCE(?1, serial),
         asset_tag    = COALESCE(?2, asset_tag),
         manufacturer = COALESCE(?3, manufacturer),
         product      = COALESCE(?4, product),
         cpu_model    = COALESCE(?5, cpu_model),
         cpu_vendor   = COALESCE(?6, cpu_vendor),
         mem_mb       = COALESCE(?7, mem_mb),
         mem_source   = COALESCE(?8, mem_source),
         arch         = COALESCE(?9, arch),
         platform     = COALESCE(?10, platform),
         last_seen    = ?11,
         boot_count   = boot_count + ?12
       WHERE id = ?13`,
    ).bind(
      cleanSmbios(facts.serial),
      cleanSmbios(facts.asset),
      cleanSmbios(facts.manufacturer),
      cleanSmbios(facts.product),
      cleanSmbios(facts.cpu),
      cleanSmbios(facts.cpuvendor),
      mem.mb,
      mem.source,
      facts.arch,
      facts.platform,
      ts,
      opts.countBoot ? 1 : 0,
      host.id,
    ),
  ];
  if (mac) {
    stmts.push(
      env.DB.prepare(
        `INSERT INTO host_macs (mac, host_id, first_seen) VALUES (?1,?2,?3)
         ON CONFLICT(mac) DO UPDATE SET host_id = excluded.host_id`,
      ).bind(mac, host.id, ts),
    );
  }
  await env.DB.batch(stmts);

  // Learn the SMBIOS UUID if we matched on a weaker key and did not have one.
  // Separate statement because smbios_uuid is uniquely indexed: a collision
  // here must not roll back the inventory refresh above.
  const uuid = normalizeUuid(facts.uuid);
  if (uuid && !host.smbios_uuid) {
    try {
      await env.DB.prepare(`UPDATE hosts SET smbios_uuid = ?1 WHERE id = ?2 AND smbios_uuid IS NULL`)
        .bind(uuid, host.id)
        .run();
    } catch {
      // Another host already claims this UUID -- leave it alone for a human.
    }
  }
}

/** Boot-path update: same refresh as a check-in, but this one counts as a boot. */
export async function touchHost(env: Env, host: HostRow, facts: BootFacts): Promise<void> {
  await refreshHost(env, host, facts, { countBoot: true });
}

export type Resolution =
  | { kind: 'volume'; volume: VolumeRow; mode: string }
  | { kind: 'none' };

/**
 * Pinned first, then a lease this host already holds, then a claimable pooled
 * volume.
 *
 * The claim is a conditional UPDATE rather than a SELECT-then-UPDATE: two
 * machines powering on together must not both be told to mount the same block
 * device read-write. Whoever's UPDATE reports changes = 1 wins; the loser falls
 * through to the next candidate.
 */
export async function resolveVolume(env: Env, host: HostRow): Promise<Resolution> {
  const cols = `v.id, v.name, v.portal_host, v.portal_port, v.lun, v.target_iqn`;
  const ts = now();
  const ttl = Number(env.LEASE_TTL_SECONDS || '3600');

  const pinned = await env.DB.prepare(
    `SELECT ${cols}, a.mode AS mode FROM assignments a JOIN volumes v ON v.id = a.volume_id
      WHERE a.mode = 'pinned' AND a.host_id = ?1 AND v.state = 'active' LIMIT 1`,
  )
    .bind(host.id)
    .first<VolumeRow & { mode: string }>();
  if (pinned) return { kind: 'volume', volume: pinned, mode: pinned.mode };

  // shared-ro is safe to hand to everyone; it is verified read-only at the LUN.
  const shared = await env.DB.prepare(
    `SELECT ${cols}, a.mode AS mode FROM assignments a JOIN volumes v ON v.id = a.volume_id
      WHERE a.mode = 'shared-ro' AND v.state = 'active' ORDER BY a.priority LIMIT 1`,
  ).first<VolumeRow & { mode: string }>();

  // Renew a lease we already hold before trying to take a new one.
  const renewed = await env.DB.prepare(
    `UPDATE assignments SET lease_expires_at = ?1
      WHERE mode = 'general-exclusive' AND lease_host_id = ?2
      RETURNING volume_id`,
  )
    .bind(ts + ttl, host.id)
    .first<{ volume_id: string }>();
  if (renewed) {
    const v = await env.DB.prepare(
      `SELECT id, name, portal_host, portal_port, lun, target_iqn FROM volumes WHERE id = ?1 AND state = 'active'`,
    )
      .bind(renewed.volume_id)
      .first<VolumeRow>();
    if (v) return { kind: 'volume', volume: v, mode: 'general-exclusive' };
  }

  const claimable = await env.DB.prepare(
    `SELECT a.id FROM assignments a JOIN volumes v ON v.id = a.volume_id
      WHERE a.mode = 'general-exclusive' AND v.state = 'active'
        AND (a.lease_host_id IS NULL OR a.lease_expires_at IS NULL OR a.lease_expires_at < ?1)
      ORDER BY a.priority`,
  )
    .bind(ts)
    .all<{ id: string }>();

  for (const row of claimable.results ?? []) {
    const won = await env.DB.prepare(
      `UPDATE assignments SET lease_host_id = ?1, lease_expires_at = ?2
        WHERE id = ?3 AND mode = 'general-exclusive'
          AND (lease_host_id IS NULL OR lease_expires_at IS NULL OR lease_expires_at < ?4)
        RETURNING volume_id`,
    )
      .bind(host.id, ts + ttl, row.id, ts)
      .first<{ volume_id: string }>();
    if (!won) continue; // lost the race, try the next volume
    const v = await env.DB.prepare(
      `SELECT id, name, portal_host, portal_port, lun, target_iqn FROM volumes WHERE id = ?1`,
    )
      .bind(won.volume_id)
      .first<VolumeRow>();
    if (v) return { kind: 'volume', volume: v, mode: 'general-exclusive' };
  }

  if (shared) return { kind: 'volume', volume: shared, mode: shared.mode };
  return { kind: 'none' };
}

/**
 * Every image this host may boot: its pinned volume, any pooled volume it
 * currently holds a lease on, and any read-only shared image. Used to populate
 * the boot-failure menu, so an operator standing at a machine that failed can
 * pick a different image (e.g. a rescue one) instead of just watching it
 * reboot.
 */
export async function listHostVolumes(env: Env, host: HostRow): Promise<(VolumeRow & { mode: string })[]> {
  const { results } = await env.DB.prepare(
    `SELECT v.id, v.name, v.portal_host, v.portal_port, v.lun, v.target_iqn, a.mode AS mode
       FROM assignments a JOIN volumes v ON v.id = a.volume_id
      WHERE v.state = 'active'
        AND ( (a.mode = 'pinned'            AND a.host_id = ?1)
           OR (a.mode = 'general-exclusive' AND a.lease_host_id = ?1)
           OR  a.mode = 'shared-ro' )
      ORDER BY CASE a.mode WHEN 'pinned' THEN 0 WHEN 'general-exclusive' THEN 1 ELSE 2 END,
               a.priority, v.name`,
  )
    .bind(host.id)
    .all<VolumeRow & { mode: string }>();
  return results ?? [];
}

/** Firmware images available to offer in the boot menu. */
export async function listFirmware(env: Env): Promise<any[]> {
  const { results } = await env.DB.prepare(
    `SELECT id, vendor, model, version, label, r2_key FROM firmware ORDER BY vendor, model, version`,
  ).all();
  return results ?? [];
}

export async function recordBootEvent(
  env: Env,
  outcome: string,
  hostId: string | null,
  volumeId: string | null,
  detail: unknown,
): Promise<void> {
  await env.DB.prepare(
    `INSERT INTO boot_events (id, ts, host_id, volume_id, outcome, detail) VALUES (?1,?2,?3,?4,?5,?6)`,
  )
    .bind(id('be'), now(), hostId, volumeId, outcome, detail ? JSON.stringify(detail) : null)
    .run();
}

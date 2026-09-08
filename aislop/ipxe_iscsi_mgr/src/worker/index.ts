import { Hono } from 'hono';
import {
  renderBootScript,
  renderChainScript,
  renderCheckinAck,
  renderCheckinScript,
  renderNoVolumeScript,
  renderPendingScript,
  renderRefusalScript,
} from '../shared/ipxe.js';
import { assignModeSchema, bootFactsSchema, iqnSchema, volumeSchema } from '../shared/types.js';
import { extractBootSecret, verifyAdmin, verifyBootSecret } from './auth.js';
import {
  findHost,
  listFirmware,
  listHostVolumes,
  recordBootEvent,
  refreshHost,
  registerHost,
  resolveVolume,
  touchHost,
  type HostRow,
  type ProfileRow,
  type VolumeRow,
} from './boot.js';
import { cleanSmbios, id, normalizeMac, normalizeUuid, now, sha256Hex, type Env } from './env.js';

const app = new Hono<{ Bindings: Env }>();

const IPXE_HEADERS = { 'content-type': 'text/plain; charset=utf-8', 'cache-control': 'no-store' };
const ipxe = (body: string, status = 200) => new Response(body, { status, headers: IPXE_HEADERS });

/**
 * iPXE does not send URI credentials preemptively -- it waits for a challenge
 * and only then retries with Basic auth. A bare 401 makes it give up with
 * "Permission denied", so the WWW-Authenticate header is what makes the
 * userinfo in the chain URL work at all.
 */
const ipxeUnauthorized = (body: string) =>
  new Response(body, {
    status: 401,
    headers: { ...IPXE_HEADERS, 'www-authenticate': 'Basic realm="ipxe-iscsi-mgr"' },
  });

app.get('/v1/health', (c) => c.json({ ok: true, service: 'ipxe-iscsi-mgr' }));

/** The script the netboot server chains into. Handy for regenerating it. */
app.get('/v1/chain.ipxe', async (c) => {
  const fallback = c.req.query('fallback') ?? null;
  return ipxe(renderChainScript(c.env.MGR_URL, { fallbackUrl: fallback }));
});

/** The check-in script the netboot server serves. Secret is added by the CLI. */
app.get('/v1/checkin.ipxe', async (c) => ipxe(renderCheckinScript(c.env.MGR_URL)));


// ------------------------------------------------------ firmware (public)

/**
 * Firmware images, served with NO auth on purpose.
 *
 * iPXE has to fetch these itself, and the alternative -- a pre-signed R2 URL --
 * would have to be baked into every generated menu and would expire. These are
 * vendor BIOS blobs, not secrets; the sensitive operation is flashing one, and
 * that requires physical access to the machine's boot menu.
 *
 * Range support is not optional: `sanboot <http url>` presents the image as a
 * virtual disk and reads it with range requests. Without 206 responses iPXE
 * would pull all 32MB before it could read the partition table.
 */
app.on(['GET', 'HEAD'], '/fw/*', async (c) => {
  const key = decodeURIComponent(new URL(c.req.url).pathname.replace(/^\/fw\//, ''));
  // No traversal, no absolute keys: the bucket is a flat namespace but the
  // paths look hierarchical, so normalise defensively.
  if (!key || key.includes('..') || key.startsWith('/')) {
    return c.text('bad firmware key\n', 400);
  }

  const rangeHeader = c.req.header('range');
  let range: R2Range | undefined;
  let wantPartial = false;
  if (rangeHeader) {
    const m = /^bytes=(\d*)-(\d*)$/.exec(rangeHeader.trim());
    if (m) {
      const start = m[1] ? Number(m[1]) : undefined;
      const end = m[2] ? Number(m[2]) : undefined;
      if (start !== undefined && end !== undefined) range = { offset: start, length: end - start + 1 };
      else if (start !== undefined) range = { offset: start };
      else if (end !== undefined) range = { suffix: end };
      wantPartial = range !== undefined;
    }
  }

  const obj = await c.env.FIRMWARE.get(key, {
    range,
    onlyIf: c.req.header('if-none-match') ? { etagDoesNotMatch: c.req.header('if-none-match') } : undefined,
  });
  if (!obj) return c.text('no such firmware image\n', 404);

  const headers = new Headers();
  headers.set('content-type', obj.httpMetadata?.contentType ?? 'application/octet-stream');
  headers.set('accept-ranges', 'bytes');
  headers.set('etag', obj.httpEtag);
  // Immutable: an image is identified by vendor/model/version and never
  // rewritten in place -- a new BIOS gets a new key.
  headers.set('cache-control', 'public, max-age=31536000, immutable');

  if (wantPartial && obj.range && 'offset' in obj.range) {
    const offset = obj.range.offset ?? 0;
    const length = obj.range.length ?? obj.size - offset;
    headers.set('content-range', `bytes ${offset}-${offset + length - 1}/${obj.size}`);
    headers.set('content-length', String(length));
    return new Response(c.req.method === 'HEAD' ? null : obj.body, { status: 206, headers });
  }

  headers.set('content-length', String(obj.size));
  return new Response(c.req.method === 'HEAD' ? null : obj.body, { status: 200, headers });
});

// ---------------------------------------------------------------- boot path

app.get('/v1/boot', async (c) => {
  if (!(await verifyBootSecret(c.env, extractBootSecret(c.req.raw)))) {
    return ipxeUnauthorized(renderRefusalScript('bad or missing boot secret'));
  }

  const parsed = bootFactsSchema.safeParse(Object.fromEntries(new URL(c.req.url).searchParams));
  if (!parsed.success) return ipxe(renderRefusalScript('malformed boot facts'), 400);
  const facts = parsed.data;

  let host = await findHost(c.env, facts);
  if (!host) {
    // Without at least one identity key we would mint a fresh row on every
    // boot, so refuse rather than fill the table with untraceable duplicates.
    if (!normalizeMac(facts.mac) && !normalizeUuid(facts.uuid) && !cleanSmbios(facts.serial)) {
      await recordBootEvent(c.env, 'unidentifiable', null, null, facts);
      return ipxe(renderRefusalScript('no usable identity (MAC, SMBIOS UUID or serial)'));
    }
    host = await registerHost(c.env, facts);
    await recordBootEvent(c.env, 'registered', host.id, null, facts);
    return ipxe(renderPendingScript(host.name));
  }

  await touchHost(c.env, host, facts);

  if (host.state === 'disabled') {
    await recordBootEvent(c.env, 'refused', host.id, null, { reason: 'disabled' });
    return ipxe(renderRefusalScript(`host ${host.name} is disabled`));
  }
  if (host.state === 'pending') {
    await recordBootEvent(c.env, 'pending', host.id, null, null);
    return ipxe(renderPendingScript(host.name));
  }

  const res = await resolveVolume(c.env, host);
  if (res.kind === 'none') {
    await recordBootEvent(c.env, 'no-volume', host.id, null, null);
    return ipxe(renderNoVolumeScript(host.name));
  }

  const profile = await loadProfile(c.env, host);
  const all = await listHostVolumes(c.env, host);
  const firmware = await listFirmware(c.env);
  await recordBootEvent(c.env, 'booted', host.id, res.volume.id, { mode: res.mode, images: all.length });
  return ipxe(buildScript(c.env, host, res.volume, profile, all, firmware));
});


/**
 * Inventory check-in. Every machine calls this at startup, including ones that
 * then boot from the legacy menu and never ask the manager for a boot script.
 *
 * Strictly separate from /v1/boot, and it must stay that way: /v1/boot returns
 * boot semantics (a pending host gets a reboot-retry loop), which would turn an
 * unattended check-in into a reboot loop on any machine not yet approved. This
 * endpoint only ever returns a non-terminal acknowledgement.
 */
app.get('/v1/checkin', async (c) => {
  if (!(await verifyBootSecret(c.env, extractBootSecret(c.req.raw)))) {
    return ipxeUnauthorized(renderRefusalScript('bad or missing boot secret'));
  }
  const params = Object.fromEntries(new URL(c.req.url).searchParams);
  const parsed = bootFactsSchema.safeParse(params);
  if (!parsed.success) return ipxe(renderRefusalScript('malformed check-in facts'), 400);
  const facts = parsed.data;
  const forced = params.force === '1' || params.force === 'true';

  if (!normalizeMac(facts.mac) && !normalizeUuid(facts.uuid) && !cleanSmbios(facts.serial)) {
    await recordBootEvent(c.env, 'checkin-unidentifiable', null, null, facts);
    return ipxe(renderRefusalScript('no usable identity (MAC, SMBIOS UUID or serial)'));
  }

  let host = await findHost(c.env, facts);
  const isNew = !host;
  if (!host) {
    host = await registerHost(c.env, facts);
  } else {
    // A check-in is not a boot, so this must not increment boot_count.
    await refreshHost(c.env, host, facts, { countBoot: false });
  }

  await recordBootEvent(c.env, forced ? 'checkin-forced' : 'checkin', host.id, null, {
    ...facts,
    registered: isNew,
  });
  return ipxe(renderCheckinAck(host.name, host.state, isNew));
});

/** Called by the generated script when every bootloader candidate failed. */
app.get('/v1/report', async (c) => {
  if (!(await verifyBootSecret(c.env, extractBootSecret(c.req.raw)))) {
    return ipxeUnauthorized(renderRefusalScript('bad or missing boot secret'));
  }
  const facts = bootFactsSchema.safeParse(Object.fromEntries(new URL(c.req.url).searchParams));
  const host = facts.success ? await findHost(c.env, facts.data) : null;
  await recordBootEvent(c.env, c.req.query('status') ?? 'unknown', host?.id ?? null, null, {
    query: Object.fromEntries(new URL(c.req.url).searchParams),
  });
  return ipxe(renderRefusalScript('failure recorded'));
});

// --------------------------------------------------------------- admin API

app.use('/v1/hosts/*', adminOnly);
app.use('/v1/hosts', adminOnly);
app.use('/v1/volumes/*', adminOnly);
app.use('/v1/volumes', adminOnly);
app.use('/v1/assignments/*', adminOnly);
app.use('/v1/assignments', adminOnly);
app.use('/v1/profiles/*', adminOnly);
app.use('/v1/profiles', adminOnly);
app.use('/v1/secrets/*', adminOnly);
app.use('/v1/secrets', adminOnly);
app.use('/v1/firmware/*', adminOnly);
app.use('/v1/firmware', adminOnly);
app.use('/v1/events', adminOnly);
app.use('/v1/render/*', adminOnly);

async function adminOnly(c: any, next: any) {
  if (!(await verifyAdmin(c.req.raw, c.env))) return c.json({ error: 'unauthorized' }, 401);
  await next();
}

app.get('/v1/hosts', async (c) => {
  const state = c.req.query('state');
  const q = state
    ? c.env.DB.prepare(`SELECT * FROM hosts WHERE state = ?1 ORDER BY name`).bind(state)
    : c.env.DB.prepare(`SELECT * FROM hosts ORDER BY name`);
  const { results } = await q.all();
  return c.json({ hosts: results });
});

/** Seed a host we already know about, ahead of it ever netbooting. */
app.post('/v1/hosts', async (c) => {
  const body = await c.req.json<{
    name: string;
    initiatorIqn?: string;
    state?: string;
    arch?: string;
    smbiosUuid?: string;
    serial?: string;
    macs?: string[];
    bootProfileId?: string;
  }>();
  if (!body.name) return c.json({ error: 'name is required' }, 400);
  const iqn = iqnSchema.parse(body.initiatorIqn ?? `${c.env.INITIATOR_IQN_PREFIX}:${body.name}`);
  const hid = id('h');
  const ts = now();
  await c.env.DB.prepare(
    `INSERT INTO hosts (id,name,state,smbios_uuid,serial,arch,platform,initiator_iqn,boot_profile_id,first_seen,last_seen,boot_count)
     VALUES (?1,?2,?3,?4,?5,?6,'efi',?7,?8,?9,?9,0)`,
  )
    .bind(
      hid,
      body.name,
      body.state ?? 'approved',
      normalizeUuid(body.smbiosUuid),
      cleanSmbios(body.serial),
      body.arch ?? 'x86_64',
      iqn,
      body.bootProfileId ?? null,
      ts,
    )
    .run();
  for (const mac of body.macs ?? []) {
    const n = normalizeMac(mac);
    if (n) {
      await c.env.DB.prepare(
        `INSERT INTO host_macs (mac, host_id, first_seen) VALUES (?1,?2,?3)
         ON CONFLICT(mac) DO UPDATE SET host_id = excluded.host_id`,
      )
        .bind(n, hid, ts)
        .run();
    }
  }
  await audit(c.env, 'host.create', hid, body);
  return c.json({ id: hid, initiatorIqn: iqn }, 201);
});

app.get('/v1/hosts/:id', async (c) => {
  const host = await lookupHost(c.env, c.req.param('id'));
  if (!host) return c.json({ error: 'no such host' }, 404);
  const macs = await c.env.DB.prepare(`SELECT mac FROM host_macs WHERE host_id = ?1`).bind(host.id).all();
  const assignment = await c.env.DB.prepare(
    `SELECT a.*, v.name AS volume_name FROM assignments a JOIN volumes v ON v.id = a.volume_id
      WHERE a.host_id = ?1 OR a.lease_host_id = ?1`,
  )
    .bind(host.id)
    .first();
  return c.json({ host, macs: (macs.results ?? []).map((r: any) => r.mac), assignment });
});

app.patch('/v1/hosts/:id', async (c) => {
  const host = await lookupHost(c.env, c.req.param('id'));
  if (!host) return c.json({ error: 'no such host' }, 404);
  const body = await c.req.json<Record<string, unknown>>();

  const sets: string[] = [];
  const binds: unknown[] = [];
  const allow: Record<string, (v: unknown) => unknown> = {
    state: (v) => String(v),
    name: (v) => String(v),
    initiator_iqn: (v) => iqnSchema.parse(v),
    boot_profile_id: (v) => (v === null ? null : String(v)),
    notes: (v) => (v === null ? null : String(v)),
    arch: (v) => String(v),
  };
  for (const [k, coerce] of Object.entries(allow)) {
    if (k in body) {
      sets.push(`${k} = ?${sets.length + 1}`);
      binds.push(coerce(body[k]));
    }
  }
  if (!sets.length) return c.json({ error: 'nothing to update' }, 400);
  binds.push(host.id);
  await c.env.DB.prepare(`UPDATE hosts SET ${sets.join(', ')} WHERE id = ?${binds.length}`)
    .bind(...binds)
    .run();
  await audit(c.env, 'host.update', host.id, body);
  return c.json({ host: await lookupHost(c.env, host.id) });
});

app.get('/v1/volumes', async (c) => {
  const { results } = await c.env.DB.prepare(
    `SELECT v.*, a.mode, a.host_id, a.lease_host_id, a.lease_expires_at
       FROM volumes v LEFT JOIN assignments a ON a.volume_id = v.id ORDER BY v.name`,
  ).all();
  return c.json({ volumes: results });
});

app.post('/v1/volumes', async (c) => {
  const body = await c.req.json();
  // The worker does not validate reachability -- ipxectl does. This only
  // enforces shape, so a typo cannot become an unparseable boot script.
  const v = volumeSchema.omit({ id: true }).parse(body);
  const vid = id('v');
  const ts = now();
  await c.env.DB.prepare(
    `INSERT INTO volumes (id,name,provider,zvol_path,portal_host,portal_port,lun,target_iqn,size_bytes,state,created_at,updated_at)
     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)`,
  )
    .bind(vid, v.name, v.provider, v.zvolPath, v.portalHost, v.portalPort, v.lun, v.targetIqn, v.sizeBytes, v.state, ts)
    .run();
  await audit(c.env, 'volume.create', vid, v);
  return c.json({ id: vid }, 201);
});

app.delete('/v1/volumes/:id', async (c) => {
  const row = await c.env.DB.prepare(`SELECT id FROM volumes WHERE id = ?1 OR name = ?1`)
    .bind(c.req.param('id'))
    .first<{ id: string }>();
  if (!row) return c.json({ error: 'no such volume' }, 404);
  await c.env.DB.prepare(`DELETE FROM volumes WHERE id = ?1`).bind(row.id).run();
  await audit(c.env, 'volume.delete', row.id, null);
  return c.json({ ok: true });
});

app.post('/v1/assignments', async (c) => {
  const body = await c.req.json<{ volume: string; mode: string; host?: string; priority?: number; forced?: boolean }>();
  const mode = assignModeSchema.parse(body.mode);
  const vol = await c.env.DB.prepare(`SELECT id FROM volumes WHERE id = ?1 OR name = ?1`)
    .bind(body.volume)
    .first<{ id: string }>();
  if (!vol) return c.json({ error: 'no such volume' }, 404);

  let hostId: string | null = null;
  if (mode === 'pinned') {
    if (!body.host) return c.json({ error: 'pinned assignment needs a host' }, 400);
    const h = await lookupHost(c.env, body.host);
    if (!h) return c.json({ error: 'no such host' }, 404);
    hostId = h.id;
  }

  await c.env.DB.prepare(
    `INSERT INTO assignments (id, volume_id, mode, host_id, priority) VALUES (?1,?2,?3,?4,?5)
     ON CONFLICT(volume_id) DO UPDATE SET
       mode = excluded.mode, host_id = excluded.host_id, priority = excluded.priority,
       lease_host_id = NULL, lease_expires_at = NULL`,
  )
    .bind(id('a'), vol.id, mode, hostId, body.priority ?? 100)
    .run();
  await audit(c.env, 'assignment.set', vol.id, body, body.forced === true);
  return c.json({ ok: true });
});

app.delete('/v1/assignments/:volume', async (c) => {
  const vol = await c.env.DB.prepare(`SELECT id FROM volumes WHERE id = ?1 OR name = ?1`)
    .bind(c.req.param('volume'))
    .first<{ id: string }>();
  if (!vol) return c.json({ error: 'no such volume' }, 404);
  await c.env.DB.prepare(`DELETE FROM assignments WHERE volume_id = ?1`).bind(vol.id).run();
  await audit(c.env, 'assignment.clear', vol.id, null);
  return c.json({ ok: true });
});

/** Drop a general-pool lease without disturbing the assignment itself. */
app.post('/v1/assignments/:volume/release', async (c) => {
  const vol = await c.env.DB.prepare(`SELECT id FROM volumes WHERE id = ?1 OR name = ?1`)
    .bind(c.req.param('volume'))
    .first<{ id: string }>();
  if (!vol) return c.json({ error: 'no such volume' }, 404);
  await c.env.DB.prepare(
    `UPDATE assignments SET lease_host_id = NULL, lease_expires_at = NULL WHERE volume_id = ?1`,
  )
    .bind(vol.id)
    .run();
  await audit(c.env, 'assignment.release', vol.id, null);
  return c.json({ ok: true });
});

app.get('/v1/profiles', async (c) => {
  const { results } = await c.env.DB.prepare(`SELECT * FROM boot_profiles ORDER BY name`).all();
  return c.json({ profiles: results });
});

app.post('/v1/profiles', async (c) => {
  const body = await c.req.json<{ name: string; arch: string; efiCandidates: string[]; extraIpxe?: string }>();
  const pid = id('p');
  await c.env.DB.prepare(
    `INSERT INTO boot_profiles (id,name,arch,efi_candidates,extra_ipxe) VALUES (?1,?2,?3,?4,?5)
     ON CONFLICT(name) DO UPDATE SET arch=excluded.arch, efi_candidates=excluded.efi_candidates, extra_ipxe=excluded.extra_ipxe`,
  )
    .bind(pid, body.name, body.arch, JSON.stringify(body.efiCandidates), body.extraIpxe ?? null)
    .run();
  await audit(c.env, 'profile.set', body.name, body);
  return c.json({ id: pid }, 201);
});

app.post('/v1/secrets', async (c) => {
  const body = await c.req.json<{ secret: string; label?: string; kind?: string }>();
  if (!body.secret || body.secret.length < 16) return c.json({ error: 'secret must be >= 16 chars' }, 400);
  const sid = id('s');
  await c.env.DB.prepare(`INSERT INTO secrets (id,kind,hash,label,created_at) VALUES (?1,?2,?3,?4,?5)`)
    .bind(sid, body.kind ?? 'boot', await sha256Hex(body.secret), body.label ?? null, now())
    .run();
  await audit(c.env, 'secret.create', sid, { label: body.label });
  return c.json({ id: sid }, 201);
});


app.get('/v1/firmware', async (c) => {
  const { results } = await c.env.DB.prepare(
    `SELECT * FROM firmware ORDER BY vendor, model, version`,
  ).all();
  return c.json({ firmware: results });
});

app.post('/v1/firmware', async (c) => {
  const b = await c.req.json<{
    vendor: string; model: string; version: string;
    r2Key: string; label?: string; sizeBytes?: number; sha256?: string; notes?: string;
  }>();
  for (const k of ['vendor', 'model', 'version', 'r2Key'] as const) {
    if (!b[k]) return c.json({ error: `${k} is required` }, 400);
  }
  // The Worker does not verify the blob exists -- same trust split as volumes.
  // `ipxectl firmware add` checks it before registering.
  const fid = id('fw');
  await c.env.DB.prepare(
    `INSERT INTO firmware (id,vendor,model,version,label,r2_key,size_bytes,sha256,notes,created_at)
     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)`,
  )
    .bind(fid, b.vendor, b.model, b.version, b.label ?? null, b.r2Key,
          b.sizeBytes ?? null, b.sha256 ?? null, b.notes ?? null, now())
    .run();
  await audit(c.env, 'firmware.create', fid, b);
  return c.json({ id: fid }, 201);
});

app.delete('/v1/firmware/:id', async (c) => {
  const row = await c.env.DB.prepare(`SELECT id FROM firmware WHERE id = ?1 OR r2_key = ?1`)
    .bind(c.req.param('id'))
    .first<{ id: string }>();
  if (!row) return c.json({ error: 'no such firmware' }, 404);
  await c.env.DB.prepare(`DELETE FROM firmware WHERE id = ?1`).bind(row.id).run();
  await audit(c.env, 'firmware.delete', row.id, null);
  return c.json({ ok: true });
});

app.get('/v1/events', async (c) => {
  const { results } = await c.env.DB.prepare(
    `SELECT e.*, h.name AS host_name, v.name AS volume_name FROM boot_events e
       LEFT JOIN hosts h ON h.id = e.host_id
       LEFT JOIN volumes v ON v.id = e.volume_id
      ORDER BY e.ts DESC LIMIT ?1`,
  )
    .bind(Number(c.req.query('limit') ?? 50))
    .all();
  return c.json({ events: results });
});

/** Exactly what this host would receive, without making it boot. */
app.get('/v1/render/:host', async (c) => {
  const host = await lookupHost(c.env, c.req.param('host'));
  if (!host) return c.json({ error: 'no such host' }, 404);
  if (host.state !== 'approved') return ipxe(renderPendingScript(host.name));
  const res = await resolveVolume(c.env, host);
  if (res.kind === 'none') return ipxe(renderNoVolumeScript(host.name));
  return ipxe(
    buildScript(
      c.env, host, res.volume,
      await loadProfile(c.env, host),
      await listHostVolumes(c.env, host),
      await listFirmware(c.env),
    ),
  );
});

// ----------------------------------------------------------------- helpers

function buildScript(
  env: Env,
  host: HostRow,
  volume: VolumeRow,
  profile: ProfileRow | null,
  all: (VolumeRow & { mode: string })[] = [],
  firmware: any[] = [],
): string {
  const asChoice = (v: VolumeRow & { mode?: string }) => ({
    name: v.name,
    portalHost: v.portal_host,
    portalPort: v.portal_port,
    lun: v.lun,
    targetIqn: v.target_iqn,
    mode: v.mode ?? null,
  });
  return renderBootScript({
    volumes: all.map(asChoice),
    bootServer: env.BOOT_SERVER ?? null,
    firmware: firmware.map((f) => ({
      id: f.id, vendor: f.vendor, model: f.model, version: f.version,
      label: f.label, r2Key: f.r2_key,
    })),
    host: {
      name: host.name,
      initiatorIqn: host.initiator_iqn,
      arch: (host.arch as 'x86_64' | 'i386' | 'arm64') ?? 'x86_64',
      manufacturer: host.manufacturer,
      product: host.product,
    },
    volume: asChoice(volume),
    profile: profile
      ? { efiCandidates: safeJsonArray(profile.efi_candidates), extraIpxe: profile.extra_ipxe }
      : null,
    mgrUrl: env.MGR_URL,
  });
}

function safeJsonArray(s: string): string[] {
  try {
    const v = JSON.parse(s);
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

async function loadProfile(env: Env, host: HostRow): Promise<ProfileRow | null> {
  if (!host.boot_profile_id) return null;
  return env.DB.prepare(`SELECT id, efi_candidates, extra_ipxe FROM boot_profiles WHERE id = ?1 OR name = ?1`)
    .bind(host.boot_profile_id)
    .first<ProfileRow>();
}

async function lookupHost(env: Env, ref: string): Promise<HostRow | null> {
  return env.DB.prepare(
    `SELECT id, name, state, arch, initiator_iqn, boot_profile_id, smbios_uuid, manufacturer, product
       FROM hosts WHERE id = ?1 OR name = ?1`,
  )
    .bind(ref)
    .first<HostRow>();
}

async function audit(env: Env, action: string, subject: string, detail: unknown, forced = false): Promise<void> {
  await env.DB.prepare(
    `INSERT INTO audit_log (id, ts, actor, action, subject, detail, forced) VALUES (?1,?2,?3,?4,?5,?6,?7)`,
  )
    .bind(id('au'), now(), 'admin', action, subject, detail ? JSON.stringify(detail) : null, forced ? 1 : 0)
    .run();
}

app.onError((err, c) => {
  console.error('unhandled', err);
  const p = new URL(c.req.url).pathname;
  if (p.startsWith('/v1/boot') || p.startsWith('/v1/report')) {
    return ipxe(renderRefusalScript('manager error'), 500);
  }
  return c.json({ error: String(err) }, 500);
});

export default app;

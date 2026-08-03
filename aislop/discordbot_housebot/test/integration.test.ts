import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { beforeEach, describe, expect, it } from 'vitest';
import { Repo } from '../src/db/repo';
import { handleInteraction } from '../src/discord/interactions';
import type { Interaction } from '../src/discord/types';
import { runScheduledRefresh } from '../src/scheduled/refresh';
import { HouseService } from '../src/service/house';
import { asD1, FakeD1 } from './helpers/d1';
import { fakeRest, htmlResponse, type RecordedCall } from './helpers/discord';

const HOUSE_CHANNEL = '1533771541106655423';
const ZILLOW_LINK =
  'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49059541_zpid/';

function fixture(name: string): string {
  return readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)), 'utf8');
}

interface Harness {
  repo: Repo;
  service: HouseService;
  calls: RecordedCall[];
  run: (i: Interaction) => Promise<Response>;
  setPage: (html: string | number) => void;
  now: () => number;
  setNow: (t: number) => void;
  routesCalls: () => number;
  setRoutesUp: (up: boolean) => void;
}

function harness(): Harness {
  const d1 = new FakeD1();
  const repo = new Repo(asD1(d1));
  const { rest, calls } = fakeRest();

  let page: string | number = fixture('zillow-active.html');
  let clock = 1_700_000_000;
  let routesCalls = 0;
  let routesUp = true;

  const listingFetch: typeof fetch = async () =>
    typeof page === 'number' ? new Response('', { status: page }) : htmlResponse(page);

  const service = new HouseService({
    repo,
    rest,
    config: {
      houseChannelId: HOUSE_CHANNEL,
      userAgent: 'housebot-test/0.1',
      refreshIntervalSeconds: 3600,
      fetchTimeoutMs: 500,
    },
    // Airtable intentionally unconfigured: the Discord path must not care.
    airtable: {},
    commute: {
      apiKey: 'test-key',
      departureIso: '2026-08-11T08:00:00-07:00',
      apiUrl: 'https://routes.test/compute',
      fetchImpl: (async () => {
        routesCalls += 1;
        return routesUp
          ? new Response(
              JSON.stringify({
                routes: [{ duration: '1115s', staticDuration: '1189s', distanceMeters: 18083 }],
              }),
              { status: 200 },
            )
          : new Response('boom', { status: 500 });
      }) as unknown as typeof fetch,
    },
    fetchImpl: listingFetch,
    now: () => clock,
  });

  const pending: Array<Promise<unknown>> = [];

  return {
    repo,
    service,
    calls,
    now: () => clock,
    setNow: (t) => {
      clock = t;
    },
    setPage: (html) => {
      page = html;
    },
    routesCalls: () => routesCalls,
    setRoutesUp: (up) => {
      routesUp = up;
    },
    run: async (interaction) => {
      const res = await handleInteraction(interaction, {
        repo,
        service,
        houseChannelId: HOUSE_CHANNEL,
        now: () => clock,
        waitUntil: (p) => pending.push(p),
        editOriginal: (token, content) => rest.editOriginalResponse(token, content),
      });
      await Promise.all(pending.splice(0));
      return res;
    },
  };
}

let seq = 0;
function interaction(
  sub: string,
  opts: {
    channelId?: string;
    parentId?: string | null;
    link?: string;
    id?: string;
    reenrich?: boolean;
  } = {},
): Interaction {
  seq += 1;
  const subOptions: Array<{ type: number; name: string; value: string | boolean }> = [];
  if (opts.link) subOptions.push({ type: 3, name: 'link', value: opts.link });
  if (opts.reenrich !== undefined) {
    subOptions.push({ type: 5, name: 'reenrich', value: opts.reenrich });
  }
  const options = subOptions.length
    ? [{ type: 1, name: sub, options: subOptions }]
    : [{ type: 1, name: sub }];
  return {
    id: opts.id ?? `interaction-${seq}`,
    type: 2,
    token: `token-${seq}`,
    application_id: '1234',
    guild_id: '555',
    channel_id: opts.channelId ?? HOUSE_CHANNEL,
    channel: {
      id: opts.channelId ?? HOUSE_CHANNEL,
      type: opts.parentId ? 11 : 0,
      parent_id: opts.parentId ?? null,
    },
    member: { user: { id: 'u1', username: 'nepeat' } },
    data: { name: 'house', options },
  };
}

function followUps(calls: RecordedCall[]): string[] {
  return calls
    .filter((c) => c.method === 'PATCH' && c.path.includes('/messages/@original'))
    .map((c) => String((c.body as { content?: string }).content));
}

function threadMessages(calls: RecordedCall[]): string[] {
  return calls
    .filter((c) => c.method === 'POST' && /\/channels\/\d+\/messages$/.test(c.path))
    .map((c) => String((c.body as { content?: string }).content));
}

describe('interaction plumbing', () => {
  let h: Harness;
  beforeEach(() => {
    h = harness();
  });

  it('answers PING with PONG', async () => {
    const res = await h.run({
      id: 'p1',
      type: 1,
      token: 't',
      application_id: '1234',
    } as Interaction);
    expect(await res.json()).toEqual({ type: 1 });
  });

  it('refuses /house outside the house channel and its threads', async () => {
    const res = await h.run(
      interaction('add', { channelId: '999', parentId: '888', link: ZILLOW_LINK }),
    );
    const body = (await res.json()) as { data: { content: string } };
    expect(body.data.content).toContain('only works in');
    expect(h.calls).toHaveLength(0);
  });

  it('is idempotent on a retried interaction id', async () => {
    await h.run(interaction('add', { link: ZILLOW_LINK, id: 'dup-1' }));
    const before = h.calls.length;
    const res = await h.run(interaction('add', { link: ZILLOW_LINK, id: 'dup-1' }));
    const body = (await res.json()) as { data: { content: string } };
    expect(body.data.content).toContain('already handled');
    expect(h.calls.length).toBe(before);
  });

  it('defers long-running subcommands and follows up', async () => {
    const res = await h.run(interaction('add', { link: ZILLOW_LINK }));
    expect((await res.json()) as unknown).toMatchObject({ type: 5, data: { flags: 64 } });
    expect(followUps(h.calls)).toHaveLength(1);
  });

  it('rejects a non-listing link without creating anything', async () => {
    await h.run(interaction('add', { link: 'https://www.realtor.com/x' }));
    expect(followUps(h.calls)[0]).toContain("doesn't look like a Zillow or Redfin listing URL");
    expect(h.calls.filter((c) => c.path.endsWith('/threads'))).toHaveLength(0);
  });
});

describe('/house add', () => {
  it('creates a thread with the canonical title and posts a snapshot', async () => {
    const h = harness();
    await h.run(interaction('add', { link: `${ZILLOW_LINK}?utm_source=x` }));

    const create = h.calls.find((c) => c.path === `/channels/${HOUSE_CHANNEL}/threads`);
    expect(create?.body).toMatchObject({
      name: '$725,000 - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
      type: 11,
    });

    const posted = threadMessages(h.calls);
    expect(posted[0]).toContain('**Price:** $725,000');
    expect(posted[0]).toContain(ZILLOW_LINK);

    const row = await h.repo.getByListingKey('zillow:49059541');
    expect(row).toMatchObject({
      provider: 'zillow',
      listing_id: '49059541',
      source_url: ZILLOW_LINK,
      status: 'active',
      force_closed: 0,
      parent_channel_id: HOUSE_CHANNEL,
    });
    expect(row?.next_check_at).toBe(h.now() + 3600);
  });

  it('deduplicates by canonical listing key across url variants', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const threadsBefore = h.calls.filter((c) => c.path.endsWith('/threads')).length;

    await h.run(interaction('add', { link: 'https://m.zillow.com/homedetails/49059541_zpid/?x=1' }));
    expect(h.calls.filter((c) => c.path.endsWith('/threads')).length).toBe(threadsBefore);
    expect(followUps(h.calls).at(-1)).toContain('already tracking');
  });

  it('reports a fetch failure cleanly and stores nothing', async () => {
    const h = harness();
    h.setPage(403);
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    expect(followUps(h.calls)[0]).toContain('blocked');
    expect(await h.repo.getByListingKey('zillow:49059541')).toBeNull();
  });
});

describe('/house bind', () => {
  const THREAD = '7000000000000001';
  const inThread = (link?: string, name?: string) => {
    const i = interaction('bind', { channelId: THREAD, parentId: HOUSE_CHANNEL, link });
    if (name !== undefined) i.channel!.name = name;
    return i;
  };

  it('adopts an existing thread, renames it, and posts the first snapshot', async () => {
    const h = harness();
    await h.run(inThread(ZILLOW_LINK, 'old thread name'));

    const row = await h.repo.getByThreadId(THREAD);
    expect(row).toMatchObject({
      listing_key: 'zillow:49059541',
      thread_id: THREAD,
      parent_channel_id: HOUSE_CHANNEL,
      force_closed: 0,
    });
    // No thread was created -- it bound the one it was run in.
    expect(h.calls.filter((c) => c.path.endsWith('/threads'))).toHaveLength(0);

    const rename = h.calls.find((c) => c.method === 'PATCH' && c.path === `/channels/${THREAD}`);
    expect(rename?.body).toMatchObject({
      name: '$725,000 - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
    });
    expect(threadMessages(h.calls)[0]).toContain('**Price:** $725,000');
    expect(followUps(h.calls).at(-1)).toContain('bound this thread to `zillow:49059541`');
  });

  it('skips the rename when the thread name already matches', async () => {
    const h = harness();
    await h.run(
      inThread(ZILLOW_LINK, '$725,000 - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057'),
    );
    expect(h.calls.filter((c) => c.method === 'PATCH' && c.path === `/channels/${THREAD}`)).toHaveLength(0);
  });

  it('refuses when run in the channel instead of a thread', async () => {
    const h = harness();
    const res = await h.run(interaction('bind', { channelId: HOUSE_CHANNEL, link: ZILLOW_LINK }));
    const body = (await res.json()) as { data: { content: string } };
    expect(body.data.content).toContain('inside the thread you want to bind');
    expect(h.calls).toHaveLength(0);
  });

  it('refuses to rebind an already-bound thread', async () => {
    const h = harness();
    await h.run(inThread(ZILLOW_LINK));
    await h.run(inThread('https://www.redfin.com/WA/Bellevue/x/home/173510'));
    expect(followUps(h.calls).at(-1)).toContain('already bound to `zillow:49059541`');
    expect(await h.repo.getByListingKey('redfin:173510')).toBeNull();
  });

  it('refuses when the listing is already tracked in another thread', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const existing = await h.repo.getByListingKey('zillow:49059541');
    await h.run(inThread(ZILLOW_LINK));
    expect(followUps(h.calls).at(-1)).toContain(`already tracked in <#${existing!.thread_id}>`);
    expect(await h.repo.getByThreadId(THREAD)).toBeNull();
  });

  it('binds nothing when the listing fetch fails', async () => {
    const h = harness();
    h.setPage(403);
    await h.run(inThread(ZILLOW_LINK));
    expect(followUps(h.calls).at(-1)).toContain('blocked');
    expect(await h.repo.getByThreadId(THREAD)).toBeNull();
    expect(threadMessages(h.calls)).toHaveLength(0);
  });

  it('rejects a non-listing link', async () => {
    const h = harness();
    await h.run(inThread('https://www.realtor.com/x'));
    expect(followUps(h.calls).at(-1)).toContain("doesn't look like a Zillow or Redfin listing URL");
  });

  it('leaves the bound thread refreshable by the cron', async () => {
    const h = harness();
    await h.run(inThread(ZILLOW_LINK));
    h.setNow(h.now() + 7200);
    h.setPage(fixture('zillow-sold.html'));
    const report = await runScheduledRefresh({
      repo: h.repo,
      service: h.service,
      batchSize: 10,
      now: () => h.now(),
    });
    expect(report).toMatchObject({ considered: 1, changed: 1 });
    expect(threadMessages(h.calls).at(-1)).toContain('**Status:** Active → Closed / Sold');
  });
});

describe('enrichment', () => {
  const LIVE_LINK =
    'https://www.zillow.com/homedetails/400-Cedar-Ave-S-Renton-WA-98057/49024254_zpid/';

  function liveHarness() {
    const h = harness();
    h.setPage(fixture('zillow-live-2026.html'));
    return h;
  }

  it('stores coordinates from the listing page', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const row = await h.repo.getByListingKey('zillow:49024254');
    expect(row?.lat).toBeCloseTo(47.4779);
    expect(row?.lon).toBeCloseTo(-122.20163);
  });

  it('posts commute + heating into the thread on add', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const posted = threadMessages(h.calls);
    expect(posted).toHaveLength(2); // snapshot, then enrichment
    expect(posted[1]).toContain('Bellevue office (nep): **19 min**');
    expect(posted[1]).toContain('heat pump');
    expect(posted[1]).toContain('unverified');
    expect(h.routesCalls()).toBe(2);
  });

  it('persists enrichment rows with provenance', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const row = await h.repo.getByListingKey('zillow:49024254');
    const rows = await h.repo.listEnrichment(row!.id);
    const byKind = Object.fromEntries(rows.map((r) => [r.kind, r]));
    expect(byKind['commute']?.status).toBe('ok');
    expect(byKind['commute']?.provenance).toContain('TRAFFIC_AWARE_OPTIMAL');
    expect(byKind['hvac']?.status).toBe('unverified');
    expect(JSON.parse(byKind['hvac']!.value_json!).kinds).toContain('heat-pump');
  });

  it('does NOT re-run enrichment on refresh when it already succeeded', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const before = h.routesCalls();
    const row = await h.repo.getByListingKey('zillow:49024254');
    await h.run(interaction('update', { channelId: row!.thread_id, parentId: HOUSE_CHANNEL }));
    expect(h.routesCalls()).toBe(before);
    expect(followUps(h.calls).at(-1)).not.toContain('filled in');
  });

  it('update self-heals a commute that was previously unavailable', async () => {
    const h = liveHarness();
    h.setRoutesUp(false);
    await h.run(interaction('add', { link: LIVE_LINK }));

    const row = await h.repo.getByListingKey('zillow:49024254');
    const before = await h.repo.listEnrichment(row!.id);
    expect(before.find((e) => e.kind === 'commute')?.status).toBe('unavailable');

    // Routing recovers; a routine update should backfill it with no /house enrich.
    h.setRoutesUp(true);
    await h.run(interaction('update', { channelId: row!.thread_id, parentId: HOUSE_CHANNEL }));

    const after = await h.repo.listEnrichment(row!.id);
    const commute = after.find((e) => e.kind === 'commute');
    expect(commute?.status).toBe('ok');
    expect(JSON.parse(commute!.value_json!)).toHaveLength(2);
    expect(threadMessages(h.calls).at(-1)).toContain('Bellevue office (nep)');
    expect(followUps(h.calls).at(-1)).toContain('filled in');
  });

  it('update backfills commute once coordinates appear on the page', async () => {
    const h = harness(); // old fixture: no geo, so no origin to route from
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const row = await h.repo.getByListingKey('zillow:49059541');
    expect(h.routesCalls()).toBe(0);
    expect(row?.lat).toBeNull();

    // Same listing key, but the page now publishes geo.
    h.setPage(
      fixture('zillow-live-2026.html').replace(/49024254/g, '49059541'),
    );
    await h.run(interaction('update', { channelId: row!.thread_id, parentId: HOUSE_CHANNEL }));

    const updated = await h.repo.getByThreadId(row!.thread_id);
    expect(updated?.lat).toBeCloseTo(47.4779);
    expect(h.routesCalls()).toBe(2);
    const commute = (await h.repo.listEnrichment(row!.id)).find((e) => e.kind === 'commute');
    expect(commute?.status).toBe('ok');
  });

  it('re-classifies heating when the listing text changes, without routing again', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const row = await h.repo.getByListingKey('zillow:49024254');
    const routesBefore = h.routesCalls();

    h.setPage(
      fixture('zillow-live-2026.html').replace('Heating</h6>', 'Heating</h6>').replace(
        'Fireplace',
        'Oil furnace, Fireplace',
      ),
    );
    await h.run(interaction('update', { channelId: row!.thread_id, parentId: HOUSE_CHANNEL }));

    const hvac = (await h.repo.listEnrichment(row!.id)).find((e) => e.kind === 'hvac');
    expect(JSON.parse(hvac!.value_json!).disliked).toEqual(['oil']);
    expect(threadMessages(h.calls).at(-1)).toContain('⚠️');
    // Reclassifying is free; it must not trigger another pair of Google calls.
    expect(h.routesCalls()).toBe(routesBefore);
  });

  it('recomputes on /house update reenrich:true', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const before = h.routesCalls();
    const row = await h.repo.getByListingKey('zillow:49024254');
    await h.run(
      interaction('update', {
        channelId: row!.thread_id,
        parentId: HOUSE_CHANNEL,
        reenrich: true,
      }),
    );
    expect(h.routesCalls()).toBe(before + 2);
    expect(followUps(h.calls).at(-1)).toContain('filled in');
  });

  it('reenrich:false behaves like a normal update', async () => {
    const h = liveHarness();
    await h.run(interaction('add', { link: LIVE_LINK }));
    const before = h.routesCalls();
    const row = await h.repo.getByListingKey('zillow:49024254');
    await h.run(
      interaction('update', {
        channelId: row!.thread_id,
        parentId: HOUSE_CHANNEL,
        reenrich: false,
      }),
    );
    expect(h.routesCalls()).toBe(before);
  });

  it('still adds the house when routing is down', async () => {
    const h = liveHarness();
    h.setRoutesUp(false);
    await h.run(interaction('add', { link: LIVE_LINK }));

    const row = await h.repo.getByListingKey('zillow:49024254');
    expect(row).not.toBeNull();
    expect(followUps(h.calls).at(-1)).toContain('tracking');

    const rows = await h.repo.listEnrichment(row!.id);
    const commute = rows.find((r) => r.kind === 'commute');
    expect(commute?.status).toBe('unavailable');
    expect(commute?.value_json).toBeNull();
    // Heating still posted; only the commute line is missing.
    expect(threadMessages(h.calls).at(-1)).toContain('heat pump');
  });

  it('records commute as unavailable when the page has no coordinates', async () => {
    const h = harness(); // the old fixture has no geo block
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const row = await h.repo.getByListingKey('zillow:49059541');
    const rows = await h.repo.listEnrichment(row!.id);
    const commute = rows.find((r) => r.kind === 'commute');
    expect(commute?.status).toBe('unavailable');
    expect(commute?.provenance).toContain('coordinates');
    expect(h.routesCalls()).toBe(0);
  });
});

describe('/house update', () => {
  async function seeded() {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const row = await h.repo.getByListingKey('zillow:49059541');
    return { h, threadId: row!.thread_id };
  }

  it('posts no notice when nothing material changed', async () => {
    const { h, threadId } = await seeded();
    const before = threadMessages(h.calls).length;
    await h.run(interaction('update', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    expect(threadMessages(h.calls).length).toBe(before);
    expect(followUps(h.calls).at(-1)).toContain('nothing material changed');
  });

  it('posts a change notice and renames the thread when the listing sells', async () => {
    const { h, threadId } = await seeded();
    h.setPage(fixture('zillow-sold.html'));
    await h.run(interaction('update', { channelId: threadId, parentId: HOUSE_CHANNEL }));

    const rename = h.calls.filter((c) => c.method === 'PATCH' && c.path === `/channels/${threadId}`);
    expect(rename.at(-1)?.body).toMatchObject({
      name: '❌ $699,000 - 4,670ft - 4b2b - 400 Cedar Avenue S, Renton, WA 98057',
    });

    const notice = threadMessages(h.calls).at(-1)!;
    expect(notice.startsWith('❌ ')).toBe(true);
    expect(notice).toContain('**Price:** $725,000 → $699,000');
    expect(notice).toContain('**Status:** Active → Closed / Sold');

    const row = await h.repo.getByThreadId(threadId);
    expect(row?.status).toBe('closed');
    expect(row?.last_changed_at).toBe(h.now());
  });

  it('records a snapshot history row per change set', async () => {
    const { h, threadId } = await seeded();
    h.setPage(fixture('zillow-sold.html'));
    await h.run(interaction('update', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    const row = await h.repo.getByThreadId(threadId);
    const history = await h.repo.listSnapshots(row!.id, 10);
    // one row for the initial add, one for the observed change set
    expect(history).toHaveLength(2);
    expect(history[0]?.source).toBe('manual');
    expect(JSON.parse(history[0]!.changes_json)).toHaveLength(2);
    expect(history[1]?.source).toBe('add');
    expect(JSON.parse(history[1]!.changes_json)).toEqual([]);
  });

  it('backs off after a failed refresh without losing the property', async () => {
    const { h, threadId } = await seeded();
    h.setPage(500);
    await h.run(interaction('update', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    const row = await h.repo.getByThreadId(threadId);
    expect(row?.fail_count).toBe(1);
    expect(row?.last_error).toContain('http-status');
    expect(row!.next_check_at).toBeGreaterThan(h.now() + 3600);
    expect(followUps(h.calls).at(-1)).toContain('refresh failed');
  });

  it('refuses to run outside a tracked thread', async () => {
    const h = harness();
    await h.run(interaction('update', { channelId: HOUSE_CHANNEL }));
    expect(followUps(h.calls).at(-1)).toContain('run this inside a house thread');
  });

  it('accepts an explicit link for an already-tracked house', async () => {
    const { h } = await seeded();
    await h.run(interaction('update', { channelId: HOUSE_CHANNEL, link: ZILLOW_LINK }));
    expect(followUps(h.calls).at(-1)).toContain('nothing material changed');
  });
});

describe('/house close and /house open', () => {
  async function seeded() {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const row = await h.repo.getByListingKey('zillow:49059541');
    return { h, threadId: row!.thread_id };
  }

  it('force-closes with the ❌ prefix and no fetch', async () => {
    const { h, threadId } = await seeded();
    await h.run(interaction('close', { channelId: threadId, parentId: HOUSE_CHANNEL }));

    const rename = h.calls.filter((c) => c.method === 'PATCH' && c.path === `/channels/${threadId}`);
    expect(String((rename.at(-1)?.body as { name: string }).name).startsWith('❌ ')).toBe(true);

    const row = await h.repo.getByThreadId(threadId);
    expect(row?.force_closed).toBe(1);
  });

  it('re-opens when the listing is still active', async () => {
    const { h, threadId } = await seeded();
    await h.run(interaction('close', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    await h.run(interaction('open', { channelId: threadId, parentId: HOUSE_CHANNEL }));

    const row = await h.repo.getByThreadId(threadId);
    expect(row?.force_closed).toBe(0);
    expect(row?.title?.startsWith('❌')).toBe(false);
    expect(followUps(h.calls).at(-1)).toContain('re-opened');
  });

  it('refuses to re-open a sold listing and explains why', async () => {
    const { h, threadId } = await seeded();
    await h.run(interaction('close', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    h.setPage(fixture('zillow-sold.html'));
    await h.run(interaction('open', { channelId: threadId, parentId: HOUSE_CHANNEL }));

    expect(followUps(h.calls).at(-1)).toContain('still reports sold/closed');
    expect((await h.repo.getByThreadId(threadId))?.force_closed).toBe(1);
  });

  it('leaves it closed when the source cannot be reached', async () => {
    const { h, threadId } = await seeded();
    await h.run(interaction('close', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    h.setPage(500);
    await h.run(interaction('open', { channelId: threadId, parentId: HOUSE_CHANNEL }));
    expect(followUps(h.calls).at(-1)).toContain('left it closed');
    expect((await h.repo.getByThreadId(threadId))?.force_closed).toBe(1);
  });
});

describe('/house status', () => {
  it('answers inline from D1 with no outgoing fetch', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const row = await h.repo.getByListingKey('zillow:49059541');
    const callsBefore = h.calls.length;

    const res = await h.run(
      interaction('status', { channelId: row!.thread_id, parentId: HOUSE_CHANNEL }),
    );
    const body = (await res.json()) as {
      type: number;
      data: { content: string; flags?: number };
    };
    expect(body.type).toBe(4);
    // Public on purpose: everyone in the thread should see the summary.
    expect(body.data.flags).toBeUndefined();
    expect(body.data.content).toContain('**Listing key:** `zillow:49059541`');
    expect(body.data.content).toContain('Last checked');
    expect(h.calls.length).toBe(callsBefore); // no REST, no listing fetch
  });
});

describe('scheduled refresh', () => {
  it('only touches due properties and respects the batch bound', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));

    const notDue = await runScheduledRefresh({
      repo: h.repo,
      service: h.service,
      batchSize: 10,
      now: () => h.now(),
    });
    expect(notDue.considered).toBe(0);

    h.setNow(h.now() + 7200);
    h.setPage(fixture('zillow-sold.html'));
    const report = await runScheduledRefresh({
      repo: h.repo,
      service: h.service,
      batchSize: 10,
      now: () => h.now(),
    });
    expect(report).toMatchObject({ considered: 1, changed: 1, failed: 0 });
    expect(threadMessages(h.calls).at(-1)).toContain('**Status:** Active → Closed / Sold');
  });

  it('skips force-closed properties entirely', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    const row = await h.repo.getByListingKey('zillow:49059541');
    await h.run(interaction('close', { channelId: row!.thread_id, parentId: HOUSE_CHANNEL }));

    h.setNow(h.now() + 7200);
    const report = await runScheduledRefresh({
      repo: h.repo,
      service: h.service,
      batchSize: 10,
      now: () => h.now(),
    });
    expect(report.considered).toBe(0);
  });

  it('reports a per-property failure without failing the batch', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK }));
    h.setNow(h.now() + 7200);
    h.setPage(500);

    const report = await runScheduledRefresh({
      repo: h.repo,
      service: h.service,
      batchSize: 10,
      now: () => h.now(),
    });
    expect(report).toMatchObject({ considered: 1, failed: 1, changed: 0 });
    expect(report.errors[0]?.listingKey).toBe('zillow:49059541');
  });

  it('prunes the interaction log', async () => {
    const h = harness();
    await h.run(interaction('add', { link: ZILLOW_LINK, id: 'old-1' }));
    h.setNow(h.now() + 30 * 86400);
    await runScheduledRefresh({
      repo: h.repo,
      service: h.service,
      batchSize: 10,
      now: () => h.now(),
    });
    // The id is now free to be reused, proving the row was pruned.
    const res = await h.run(interaction('status', { id: 'old-1' }));
    const body = (await res.json()) as { data: { content: string } };
    expect(body.data.content).not.toContain('already handled');
  });
});

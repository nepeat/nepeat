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
}

function harness(): Harness {
  const d1 = new FakeD1();
  const repo = new Repo(asD1(d1));
  const { rest, calls } = fakeRest();

  let page: string | number = fixture('zillow-active.html');
  let clock = 1_700_000_000;

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
  opts: { channelId?: string; parentId?: string | null; link?: string; id?: string } = {},
): Interaction {
  seq += 1;
  const options = opts.link
    ? [{ type: 1, name: sub, options: [{ type: 3, name: 'link', value: opts.link }] }]
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
    const body = (await res.json()) as { type: number; data: { content: string; flags: number } };
    expect(body.type).toBe(4);
    expect(body.data.flags).toBe(64);
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

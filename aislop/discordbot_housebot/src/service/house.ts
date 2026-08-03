import { syncListing, type AirtableConfig, type AirtableSyncResult } from '../airtable/client';
import { estimateCommutes, formatCommute, type CommuteConfig } from '../enrichment/commute';
import { classifyHvac, type HvacClassification } from '../enrichment/hvac';
import { buildEnrichmentEmbed, buildStatusEmbed, type Embed } from '../discord/embeds';
import {
  isCurrentCommuteShape,
  normalizeCommuteValue,
  type CommuteValue,
} from '../enrichment/commute';
import {
  backoffSeconds,
  parseSnapshot,
  Repo,
  type EnrichmentRow,
  type PropertyRow,
} from '../db/repo';
import type { DiscordRest } from '../discord/rest';
import { computeChanges } from '../listing/diff';
import { explainFailure, fetchListing } from '../listing/fetcher';
import {
  buildChangeMessage,
  buildSnapshotMessage,
  buildThreadTitle,
  statusLabel,
} from '../listing/format';
import { evaluateOpen, isClosed, OPEN_FAILURE_TEXT } from '../listing/status';
import type { FieldChange, ListingStatus, Snapshot } from '../listing/types';
import { identifyUrl } from '../listing/url';

export interface HouseServiceConfig {
  houseChannelId: string;
  userAgent: string;
  refreshIntervalSeconds: number;
  fetchTimeoutMs: number;
}

export interface HouseServiceDeps {
  repo: Repo;
  rest: DiscordRest;
  config: HouseServiceConfig;
  airtable: AirtableConfig;
  commute?: CommuteConfig;
  fetchImpl?: typeof fetch;
  now?: () => number;
}

export interface RefreshOutcome {
  kind: 'changed' | 'unchanged' | 'not-modified' | 'error';
  changes: FieldChange[];
  snapshot?: Snapshot;
  detail?: string;
  /** How many enrichment lines this refresh newly filled in. */
  enriched?: number;
}

export class HouseService {
  private readonly repo: Repo;
  private readonly rest: DiscordRest;
  private readonly config: HouseServiceConfig;
  private readonly airtable: AirtableConfig;
  private readonly commute: CommuteConfig;
  private readonly fetchImpl?: typeof fetch;
  private readonly nowFn: () => number;

  constructor(deps: HouseServiceDeps) {
    this.repo = deps.repo;
    this.rest = deps.rest;
    this.config = deps.config;
    this.airtable = deps.airtable;
    this.commute = deps.commute ?? {};
    this.fetchImpl = deps.fetchImpl;
    this.nowFn = deps.now ?? (() => Math.floor(Date.now() / 1000));
  }

  private now(): number {
    return this.nowFn();
  }

  private fetchOpts(row?: PropertyRow | null) {
    return {
      userAgent: this.config.userAgent,
      timeoutMs: this.config.fetchTimeoutMs,
      etag: row?.etag ?? null,
      lastModified: row?.last_modified ?? null,
      ...(this.fetchImpl ? { fetchImpl: this.fetchImpl } : {}),
    };
  }

  /** `/house add` — dedupe, fetch, create thread, post the first snapshot. */
  async add(input: {
    link: string;
    guildId: string | null;
  }): Promise<{ message: string }> {
    const ident = identifyUrl(input.link);
    if (!ident) {
      return {
        message:
          "that doesn't look like a Zillow or Redfin listing URL. I need a `/homedetails/..._zpid/` or `/home/<id>` link.",
      };
    }

    const existing = await this.repo.getByListingKey(ident.listingKey);
    if (existing) {
      return {
        message: `already tracking that one → <#${existing.thread_id}>`,
      };
    }

    const now = this.now();
    const result = await fetchListing(ident.canonicalUrl, this.fetchOpts(), now);
    if (!result.ok) {
      return {
        message: `couldn't add that house: ${explainFailure(result.reason, result.detail)}`,
      };
    }
    if (result.kind === 'not-modified') {
      // Only possible with validators we never send on a first fetch.
      return { message: 'unexpected 304 from the listing page; try again.' };
    }

    const snapshot = result.snapshot;
    const closed = isClosed({ forceClosed: false, listingStatus: snapshot.status });
    const title = buildThreadTitle(snapshot, { closed, fallback: ident.listingKey });

    const thread = await this.rest.createThread(this.config.houseChannelId, title);

    // Re-check after the (slow) network work in case a concurrent /house add won.
    const raced = await this.repo.getByListingKey(ident.listingKey);
    if (raced) {
      return { message: `already tracking that one → <#${raced.thread_id}>` };
    }

    const row = await this.repo.insertProperty({
      snapshot,
      guildId: input.guildId,
      parentChannelId: this.config.houseChannelId,
      threadId: thread.id,
      title,
      etag: result.etag ?? null,
      lastModified: result.lastModified ?? null,
      now,
      nextCheckAt: now + this.config.refreshIntervalSeconds,
    });

    await this.rest.postMessage(thread.id, buildSnapshotMessage(snapshot, closed));
    await this.enrich(row, snapshot);
    await this.syncAirtable(row, snapshot);

    return { message: `tracking → <#${thread.id}>` };
  }

  /**
   * `/house bind` — adopt an existing thread instead of creating one.
   * Same dedupe rules as `add`; the thread is renamed to the canonical title so
   * D1 and Discord never disagree about what this thread is.
   */
  async bind(input: {
    link: string;
    guildId: string | null;
    threadId: string;
    parentChannelId: string;
    currentName?: string | null;
  }): Promise<{ message: string }> {
    const ident = identifyUrl(input.link);
    if (!ident) {
      return {
        message:
          "that doesn't look like a Zillow or Redfin listing URL. I need a `/homedetails/..._zpid/` or `/home/<id>` link.",
      };
    }

    const bound = await this.repo.getByThreadId(input.threadId);
    if (bound) {
      return {
        message: `this thread is already bound to \`${bound.listing_key}\`. Use \`/house update\` to refresh it.`,
      };
    }
    const elsewhere = await this.repo.getByListingKey(ident.listingKey);
    if (elsewhere) {
      return { message: `that listing is already tracked in <#${elsewhere.thread_id}>` };
    }

    const now = this.now();
    const result = await fetchListing(ident.canonicalUrl, this.fetchOpts(), now);
    if (!result.ok) {
      return {
        message: `couldn't bind that house: ${explainFailure(result.reason, result.detail)}`,
      };
    }
    if (result.kind === 'not-modified') {
      return { message: 'unexpected 304 from the listing page; try again.' };
    }

    // Re-check after the slow network work in case a concurrent command won.
    if (await this.repo.getByThreadId(input.threadId)) {
      return { message: 'this thread just got bound by another command.' };
    }
    if (await this.repo.getByListingKey(ident.listingKey)) {
      return { message: 'that listing just got tracked by another command.' };
    }

    const snapshot = result.snapshot;
    const closed = isClosed({ forceClosed: false, listingStatus: snapshot.status });
    const title = buildThreadTitle(snapshot, { closed, fallback: ident.listingKey });

    const row = await this.repo.insertProperty({
      snapshot,
      guildId: input.guildId,
      parentChannelId: input.parentChannelId,
      threadId: input.threadId,
      title,
      etag: result.etag ?? null,
      lastModified: result.lastModified ?? null,
      now,
      nextCheckAt: now + this.config.refreshIntervalSeconds,
    });

    if (input.currentName !== title) {
      await this.rest.renameThread(input.threadId, title);
    }
    await this.rest.postMessage(input.threadId, buildSnapshotMessage(snapshot, closed));
    await this.enrich(row, snapshot);
    await this.syncAirtable(row, snapshot);

    return { message: `bound this thread to \`${snapshot.listingKey}\`.` };
  }

  /**
   * Shared refresh path for `/house update` and the cron.
   * Posts a change notice only when a material field moved.
   */
  async refresh(
    row: PropertyRow,
    source: 'manual' | 'scheduled',
    overrideUrl?: string,
    opts: { enrichMode?: 'missing' | 'force' } = {},
  ): Promise<RefreshOutcome> {
    const now = this.now();
    const url = overrideUrl ?? row.source_url;
    // An explicit override URL means "ignore my cached validators".
    const fetchOptions = overrideUrl ? this.fetchOpts() : this.fetchOpts(row);
    const result = await fetchListing(url, fetchOptions, now);

    if (!result.ok) {
      const nextAt = now + backoffSeconds(row.fail_count + 1, this.config.refreshIntervalSeconds);
      await this.repo.applyFetchFailure(
        row.id,
        `${result.reason}: ${result.detail}`,
        now,
        nextAt,
      );
      return {
        kind: 'error',
        changes: [],
        detail: explainFailure(result.reason, result.detail),
      };
    }

    if (result.kind === 'not-modified') {
      await this.repo.touchChecked(row.id, now, now + this.config.refreshIntervalSeconds);
      // The listing did not change, but enrichment may still be blank or stale.
      // Skipping it here would strand a house whose page always answers 304.
      const stored = parseSnapshot(row);
      const backfilled = stored
        ? await this.enrich(row, stored, { mode: opts.enrichMode ?? 'missing' })
        : { lines: [] };
      return { kind: 'not-modified', changes: [], enriched: backfilled.lines.length };
    }

    const prev = parseSnapshot(row);
    const next = result.snapshot;
    const changes = computeChanges(prev, next);
    const closed = isClosed({
      forceClosed: row.force_closed === 1,
      listingStatus: next.status,
    });
    const title = buildThreadTitle(next, { closed, fallback: next.listingKey });

    await this.repo.applyFetchSuccess({
      propertyId: row.id,
      snapshot: next,
      title,
      changes,
      etag: result.etag ?? null,
      lastModified: result.lastModified ?? null,
      now,
      nextCheckAt: now + this.config.refreshIntervalSeconds,
      source,
    });

    if (title !== row.title) {
      await this.rest.renameThread(row.thread_id, title);
    }
    if (changes.length > 0) {
      await this.rest.postMessage(row.thread_id, buildChangeMessage(changes, next, closed));
    }

    // Fill whatever enrichment is still blank. Free when everything already
    // succeeded, and it self-heals rows added before a parser improvement.
    const filled = await this.enrich(row, next, { mode: opts.enrichMode ?? 'missing' });

    await this.syncAirtable(row, next);

    return {
      kind: changes.length > 0 ? 'changed' : 'unchanged',
      changes,
      snapshot: next,
      enriched: filled.lines.length,
    };
  }

  /** `/house close` — force closed, no fetch required. */
  async close(row: PropertyRow): Promise<{ message: string }> {
    const now = this.now();
    const snapshot = parseSnapshot(row);
    const title = buildThreadTitle(snapshot ?? {}, {
      closed: true,
      fallback: row.title ?? row.listing_key,
    });
    await this.repo.setForceClosed(row.id, true, title, now);
    if (title !== row.title) await this.rest.renameThread(row.thread_id, title);
    await this.rest.postMessage(row.thread_id, `❌ **Closed** — marked closed manually.`);
    return { message: 'marked closed. it will stop being refreshed by the cron.' };
  }

  /** `/house open` — only if the live listing agrees the house is available. */
  async open(row: PropertyRow): Promise<{ message: string }> {
    const now = this.now();
    const result = await fetchListing(row.source_url, this.fetchOpts(), now);
    if (!result.ok) {
      return {
        message: `left it closed — ${explainFailure(result.reason, result.detail)}`,
      };
    }
    if (result.kind === 'not-modified') {
      // Fall back to the stored status; a 304 means nothing changed.
      const stored = (row.status as ListingStatus) ?? 'unknown';
      return this.finishOpen(row, stored, parseSnapshot(row), now);
    }
    return this.finishOpen(row, result.snapshot.status, result.snapshot, now);
  }

  private async finishOpen(
    row: PropertyRow,
    status: ListingStatus,
    snapshot: Snapshot | null,
    now: number,
  ): Promise<{ message: string }> {
    const decision = evaluateOpen(status);
    if (!decision.ok) {
      return { message: OPEN_FAILURE_TEXT[decision.reason] };
    }
    const title = buildThreadTitle(snapshot ?? {}, {
      closed: false,
      fallback: row.title ?? row.listing_key,
    });
    await this.repo.setForceClosed(row.id, false, title, now);
    await this.repo.setStatus(row.id, status, now);
    if (title !== row.title) await this.rest.renameThread(row.thread_id, title);
    await this.rest.postMessage(row.thread_id, `**Re-opened** — listing status is ${statusLabel(status)}.`);
    return { message: `re-opened (listing status: ${statusLabel(status)}).` };
  }

  /** `/house status` — D1 only, including stored enrichment. No outgoing fetch. */
  async status(row: PropertyRow): Promise<{ embed: Embed }> {
    const snapshot = parseSnapshot(row);

    let commute: CommuteValue | null = null;
    let commuteProvenance: string | null = null;
    let commuteStatus: string | null = null;
    let hvac: HvacClassification | null = null;
    try {
      for (const e of await this.repo.listEnrichment(row.id)) {
        if (e.kind === 'commute') {
          commuteProvenance = e.provenance;
          commuteStatus = e.status;
          commute = e.value_json ? normalizeCommuteValue(JSON.parse(e.value_json)) : null;
        } else if (e.kind === 'hvac' && e.value_json) {
          hvac = JSON.parse(e.value_json) as HvacClassification;
        }
      }
    } catch (err) {
      console.error('status enrichment read failed', { id: row.id, error: errText(err) });
    }

    return {
      embed: buildStatusEmbed({
        title: row.title ?? row.listing_key,
        sourceUrl: row.source_url,
        listingKey: row.listing_key,
        status: (row.status as ListingStatus) ?? 'unknown',
        forceClosed: row.force_closed === 1,
        lastCheckedAt: row.last_checked_at,
        lastChangedAt: row.last_changed_at,
        failCount: row.fail_count,
        lastError: row.last_error,
        snapshot,
        commute,
        commuteProvenance,
        commuteStatus,
        hvac,
      }),
    };
  }

  /**
   * Location-derived facts.
   *
   * `mode: 'missing'` (the refresh path) computes only what is still blank. A
   * kind that already holds a value is skipped, so a routine `/house update`
   * costs zero Google calls. A kind previously recorded `unavailable` IS
   * retried -- that is what makes update self-healing: a house added before we
   * parsed coordinates picks up its commute on the next update, with no manual
   * step. HVAC additionally re-runs when the listing's heating text changed,
   * since reclassifying costs nothing.
   *
   * `mode: 'force'` (the first add/bind, or `/house update reenrich:true`)
   * recomputes everything.
   */
  async enrich(
    row: PropertyRow,
    snapshot: Snapshot,
    opts: { post?: boolean; mode?: 'missing' | 'force' } = {},
  ): Promise<{ lines: string[] }> {
    const now = this.now();
    const mode = opts.mode ?? 'force';
    const lines: string[] = [];
    let hvacValue: HvacClassification | null = null;
    let commuteValue: CommuteValue | null = null;
    let commuteProvenance = '';

    let existing: EnrichmentRow[] = [];
    if (mode === 'missing') {
      try {
        existing = await this.repo.listEnrichment(row.id);
      } catch (err) {
        console.error('enrichment lookup failed', { id: row.id, error: errText(err) });
      }
    }
    // "settled" means: holds a value the CURRENT adapter would produce. A row
    // written by an older adapter version is deliberately not settled, so an
    // update upgrades it instead of preserving a stale shape forever.
    const settled = (kind: string): boolean =>
      existing.some((e) => {
        if (e.kind !== kind || e.value_json === null) return false;
        if (kind !== 'commute') return true;
        try {
          return isCurrentCommuteShape(JSON.parse(e.value_json));
        } catch {
          return false;
        }
      });

    const hvacStale =
      mode === 'force' ||
      !settled('hvac') ||
      storedHvacRaw(existing) !== (snapshot.hvac ?? null);

    // HVAC: pure classification of text we already have. No network.
    if (hvacStale) try {
      const hvac = classifyHvac(snapshot.hvac);
      if (hvac) {
        await this.repo.putEnrichment({
          propertyId: row.id,
          kind: 'hvac',
          status: 'unverified',
          provenance: 'classified from public listing text; not confirmed against county records',
          value: hvac,
          now,
        });
        hvacValue = hvac;
        lines.push('heating');
      } else {
        await this.repo.putEnrichment({
          propertyId: row.id,
          kind: 'hvac',
          status: 'unavailable',
          provenance: 'listing page did not expose a heating field',
          value: null,
          now,
        });
      }
    } catch (err) {
      console.error('hvac enrichment failed', { id: row.id, error: errText(err) });
    }

    // Commute: two Google Routes calls on the Pro SKU. Skipping this when we
    // already have an answer is the entire point of `missing` mode.
    if (mode === 'missing' && settled('commute')) {
      return this.postEnrichment(row, lines, opts, {
        snapshot,
        hvac: hvacValue,
        commute: null,
        commuteProvenance: '',
      });
    }
    try {
      const result = await estimateCommutes(
        snapshot.lat ?? row.lat,
        snapshot.lon ?? row.lon,
        this.commute,
      );
      await this.repo.putEnrichment({
        propertyId: row.id,
        kind: 'commute',
        status: result.status,
        provenance: result.provenance,
        value: result.status === 'ok' ? result.value : null,
        now,
      });
      if (result.status === 'ok') {
        commuteValue = result.value;
        commuteProvenance = result.provenance;
        if (result.value.drive.length) lines.push('driving');
        if (result.value.transit.length) lines.push('transit');
      }
    } catch (err) {
      console.error('commute enrichment failed', { id: row.id, error: errText(err) });
    }

    return this.postEnrichment(row, lines, opts, {
      snapshot,
      hvac: hvacValue,
      commute: commuteValue,
      commuteProvenance,
    });
  }

  private async postEnrichment(
    row: PropertyRow,
    lines: string[],
    opts: { post?: boolean },
    payload: {
      snapshot: Snapshot;
      hvac: HvacClassification | null;
      commute: CommuteValue | null;
      commuteProvenance: string;
    },
  ): Promise<{ lines: string[] }> {
    if (opts.post === false || lines.length === 0) return { lines };
    const embed: Embed = buildEnrichmentEmbed({
      snapshot: payload.snapshot,
      commute: payload.commute,
      commuteProvenance: payload.commuteProvenance,
      hvac: payload.hvac,
      closed: isClosed({
        forceClosed: row.force_closed === 1,
        listingStatus: payload.snapshot.status,
      }),
    });
    if (!embed.fields?.length) return { lines };
    await this.rest.postMessage(row.thread_id, '', [embed]);
    return { lines };
  }

  /** Airtable is advisory: results are recorded, failures never propagate. */
  private async syncAirtable(row: PropertyRow, snapshot: Snapshot): Promise<AirtableSyncResult> {
    let result: AirtableSyncResult;
    try {
      result = await syncListing(snapshot, this.airtable, {
        guildId: row.guild_id,
        threadId: row.thread_id,
      });
    } catch (err) {
      result = {
        status: 'error',
        detail: err instanceof Error ? err.message : String(err),
      };
    }
    try {
      await this.repo.recordAirtableSync(
        snapshot.listingKey,
        result.status,
        result.detail,
        result.status === 'ok' ? result.recordId : null,
        this.now(),
      );
    } catch {
      // Sync bookkeeping is never allowed to fail a user command.
    }
    return result;
  }
}

/** The heating text a stored HVAC classification was derived from, if any. */
function storedHvacRaw(rows: EnrichmentRow[]): string | null {
  const row = rows.find((r) => r.kind === 'hvac');
  if (!row?.value_json) return null;
  try {
    return (JSON.parse(row.value_json) as { raw?: string }).raw ?? null;
  } catch {
    return null;
  }
}

function errText(err: unknown): string {
  return err instanceof Error ? (err.stack ?? err.message) : String(err);
}

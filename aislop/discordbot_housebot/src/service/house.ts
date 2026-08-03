import { syncListing, type AirtableConfig, type AirtableSyncResult } from '../airtable/client';
import { estimateCommutes, formatCommute, type CommuteConfig } from '../enrichment/commute';
import { classifyHvac, type HvacClassification } from '../enrichment/hvac';
import { buildHouseEmbed, buildStatusEmbed, type Embed } from '../discord/embeds';
import { lookupIsp, type IspConfig, type IspOffer } from '../enrichment/isp';
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
import { buildChangeMessage, buildThreadTitle, statusLabel } from '../listing/format';
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
  isp?: IspConfig;
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
  private readonly isp: IspConfig;
  private readonly fetchImpl?: typeof fetch;
  private readonly nowFn: () => number;

  constructor(deps: HouseServiceDeps) {
    this.repo = deps.repo;
    this.rest = deps.rest;
    this.config = deps.config;
    this.airtable = deps.airtable;
    this.commute = deps.commute ?? {};
    this.isp = deps.isp ?? {};
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

    // Forum/media channels reject a thread with no starter message; text
    // channels reject one that has it. Ask what this channel is, don't assume.
    let parentType: number | undefined;
    try {
      parentType = (await this.rest.getChannel(this.config.houseChannelId)).type;
    } catch (err) {
      console.error('channel type lookup failed', { error: errText(err) });
    }
    // Enrichment runs BEFORE the thread exists so the starter message can be
    // the complete house card rather than a stub followed by a second embed.
    const bundle = await this.computeEnrichment(snapshot, {
      hvac: true,
      commute: true,
      isp: true,
    });
    const starterEmbed = buildHouseEmbed({
      snapshot,
      closed,
      commute: (bundle.commute?.value as CommuteValue | null) ?? null,
      commuteProvenance: bundle.commute?.provenance ?? null,
      hvac: (bundle.hvac?.value as HvacClassification | null) ?? null,
      isp: (bundle.isp?.value as IspOffer[] | null) ?? null,
      ispProvenance: bundle.isp?.provenance ?? null,
    });
    const thread = await this.rest.createThread(this.config.houseChannelId, title, {
      parentType,
      starter: { embeds: [starterEmbed] },
    });

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

    // In a forum channel the embed IS the starter message; a text channel needs
    // it posted separately.
    if (!thread.usedStarter) {
      await this.rest.postMessage(thread.id, '', [starterEmbed]);
    }
    await this.persistEnrichment(row.id, bundle, now);
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
    const bundle = await this.computeEnrichment(snapshot, {
      hvac: true,
      commute: true,
      isp: true,
    });
    await this.rest.postMessage(input.threadId, '', [
      buildHouseEmbed({
        snapshot,
        closed,
        commute: (bundle.commute?.value as CommuteValue | null) ?? null,
        commuteProvenance: bundle.commute?.provenance ?? null,
        hvac: (bundle.hvac?.value as HvacClassification | null) ?? null,
        isp: (bundle.isp?.value as IspOffer[] | null) ?? null,
        ispProvenance: bundle.isp?.provenance ?? null,
      }),
    ]);
    await this.persistEnrichment(row.id, bundle, now);
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
    // An explicit override URL means "ignore my cached validators". So does a
    // missing photo: a 304 returns no body, so a house stored before we parsed
    // og:image could never acquire one. Bootstrapping it is worth one full GET.
    const storedSnapshot = parseSnapshot(row);
    const needsPhoto = !storedSnapshot?.photoUrl;
    const fetchOptions = overrideUrl || needsPhoto ? this.fetchOpts() : this.fetchOpts(row);
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

    const prev = storedSnapshot;
    const next = result.snapshot;
    const photoAdded = !prev?.photoUrl && Boolean(next.photoUrl);
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
    const filled = await this.enrich(row, next, {
      mode: opts.enrichMode ?? 'missing',
      photoAdded,
    });

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

  /**
   * Compute location-derived facts. Pure of the database and of Discord, so it
   * can run BEFORE the thread exists — which is what lets the thread's starter
   * message be the complete house embed instead of a bare snapshot followed by
   * a second, disaggregated card.
   */
  async computeEnrichment(
    snapshot: Snapshot,
    want: { hvac: boolean; commute: boolean; isp: boolean },
  ): Promise<EnrichmentBundle> {
    const out: EnrichmentBundle = {};

    if (want.hvac) {
      try {
        const hvac = classifyHvac(snapshot.hvac);
        out.hvac = hvac
          ? {
              status: 'unverified',
              provenance:
                'classified from public listing text; not confirmed against county records',
              value: hvac,
            }
          : {
              status: 'unavailable',
              provenance: 'listing page did not expose a heating field',
              value: null,
            };
      } catch (err) {
        console.error('hvac enrichment failed', { error: errText(err) });
      }
    }

    if (want.commute) {
      try {
        const r = await estimateCommutes(snapshot.lat, snapshot.lon, this.commute);
        out.commute =
          r.status === 'ok'
            ? { status: 'ok', provenance: r.provenance, value: r.value }
            : { status: 'unavailable', provenance: r.provenance, value: null };
      } catch (err) {
        console.error('commute enrichment failed', { error: errText(err) });
      }
    }

    if (want.isp) {
      try {
        const r = await lookupIsp(snapshot.lat, snapshot.lon, this.isp);
        out.isp =
          r.status === 'unverified'
            ? { status: 'unverified', provenance: r.provenance, value: r.value }
            : { status: 'unavailable', provenance: r.provenance, value: null };
      } catch (err) {
        console.error('isp enrichment failed', { error: errText(err) });
      }
    }

    return out;
  }

  /** Persist whatever was computed. Failures never propagate to the command. */
  private async persistEnrichment(
    propertyId: number,
    bundle: EnrichmentBundle,
    now: number,
  ): Promise<void> {
    for (const kind of ['hvac', 'commute', 'isp'] as const) {
      const entry = bundle[kind];
      if (!entry) continue;
      try {
        await this.repo.putEnrichment({
          propertyId,
          kind,
          status: entry.status,
          provenance: entry.provenance,
          value: entry.value,
          now,
        });
      } catch (err) {
        console.error('enrichment persist failed', { propertyId, kind, error: errText(err) });
      }
    }
  }

  /**
   * Which enrichments a refresh still needs.
   *
   * `force` recomputes everything. Otherwise a kind holding a current-shaped
   * value is skipped — so a routine update costs no API calls — while a kind
   * recorded `unavailable` IS retried, which is what makes update self-healing.
   * HVAC additionally re-runs when the listing's heating text changed, since
   * reclassifying is free.
   */
  private async decideWanted(
    row: PropertyRow,
    snapshot: Snapshot,
    mode: 'missing' | 'force',
  ): Promise<{ hvac: boolean; commute: boolean; isp: boolean }> {
    if (mode === 'force') return { hvac: true, commute: true, isp: true };

    let existing: EnrichmentRow[] = [];
    try {
      existing = await this.repo.listEnrichment(row.id);
    } catch (err) {
      console.error('enrichment lookup failed', { id: row.id, error: errText(err) });
    }
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

    return {
      hvac: !settled('hvac') || storedHvacRaw(existing) !== (snapshot.hvac ?? null),
      commute: !settled('commute'),
      isp: !settled('isp'),
    };
  }

  /** Merge freshly computed values over what is already stored. */
  private async mergeForDisplay(
    row: PropertyRow,
    bundle: EnrichmentBundle,
  ): Promise<{
    commute: CommuteValue | null;
    commuteProvenance: string | null;
    hvac: HvacClassification | null;
    isp: IspOffer[] | null;
    ispProvenance: string | null;
  }> {
    const stored = await this.loadEnrichment(row);
    return {
      commute: (bundle.commute?.value as CommuteValue | null) ?? stored.commute,
      commuteProvenance: bundle.commute?.provenance ?? stored.commuteProvenance,
      hvac: (bundle.hvac?.value as HvacClassification | null) ?? stored.hvac,
      isp: (bundle.isp?.value as IspOffer[] | null) ?? stored.isp,
      ispProvenance: bundle.isp?.provenance ?? stored.ispProvenance,
    };
  }

  /** Stored enrichment, decoded. Shared by `/house status` and the embed path. */
  private async loadEnrichment(row: PropertyRow): Promise<{
    commute: CommuteValue | null;
    commuteProvenance: string | null;
    commuteStatus: string | null;
    hvac: HvacClassification | null;
    isp: IspOffer[] | null;
    ispProvenance: string | null;
  }> {
    let commute: CommuteValue | null = null;
    let commuteProvenance: string | null = null;
    let commuteStatus: string | null = null;
    let hvac: HvacClassification | null = null;
    let isp: IspOffer[] | null = null;
    let ispProvenance: string | null = null;
    try {
      for (const e of await this.repo.listEnrichment(row.id)) {
        if (e.kind === 'commute') {
          commuteProvenance = e.provenance;
          commuteStatus = e.status;
          commute = e.value_json ? normalizeCommuteValue(JSON.parse(e.value_json)) : null;
        } else if (e.kind === 'hvac' && e.value_json) {
          hvac = JSON.parse(e.value_json) as HvacClassification;
        } else if (e.kind === 'isp') {
          ispProvenance = e.provenance;
          isp = e.value_json ? (JSON.parse(e.value_json) as IspOffer[]) : null;
        }
      }
    } catch (err) {
      console.error('enrichment read failed', { id: row.id, error: errText(err) });
    }
    return { commute, commuteProvenance, commuteStatus, hvac, isp, ispProvenance };
  }

  /** `/house info` — D1 only, including stored enrichment. No outgoing fetch. */
  async info(row: PropertyRow): Promise<{ embed: Embed }> {
    const snapshot = parseSnapshot(row);
    const { commute, commuteProvenance, commuteStatus, hvac, isp, ispProvenance } =
      await this.loadEnrichment(row);

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
        isp,
        ispProvenance,
      }),
    };
  }

  /**
   * Refresh enrichment for an existing house and post the merged embed.
   *
   * `mode: 'missing'` computes only what is blank or stale; `'force'` redoes
   * everything. The embed is posted only when something new was learned (or a
   * photo just appeared), and it always shows the FULL picture — freshly
   * computed values merged over what D1 already holds — so the thread never
   * ends up with two half-cards describing one house.
   */
  async enrich(
    row: PropertyRow,
    snapshot: Snapshot,
    opts: { post?: boolean; mode?: 'missing' | 'force'; photoAdded?: boolean } = {},
  ): Promise<{ lines: string[] }> {
    const now = this.now();
    const mode = opts.mode ?? 'force';
    const want = await this.decideWanted(row, snapshot, mode);

    const bundle = await this.computeEnrichment(snapshot, want);
    await this.persistEnrichment(row.id, bundle, now);

    const lines: string[] = [];
    if (bundle.hvac?.value) lines.push('heating');
    if (bundle.commute?.value) lines.push('commute');
    if (bundle.isp?.value) lines.push('internet');
    if (opts.photoAdded) lines.push('photo');

    if (opts.post === false || lines.length === 0) return { lines };

    const merged = await this.mergeForDisplay(row, bundle);
    const embed = buildHouseEmbed({
      snapshot,
      closed: isClosed({
        forceClosed: row.force_closed === 1,
        listingStatus: snapshot.status,
      }),
      ...merged,
    });
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

export interface EnrichmentEntry {
  status: string;
  provenance: string;
  value: unknown;
}

export interface EnrichmentBundle {
  hvac?: EnrichmentEntry;
  commute?: EnrichmentEntry;
  isp?: EnrichmentEntry;
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

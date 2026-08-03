import { syncListing, type AirtableConfig, type AirtableSyncResult } from '../airtable/client';
import { backoffSeconds, parseSnapshot, Repo, type PropertyRow } from '../db/repo';
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
  fetchImpl?: typeof fetch;
  now?: () => number;
}

export interface RefreshOutcome {
  kind: 'changed' | 'unchanged' | 'not-modified' | 'error';
  changes: FieldChange[];
  snapshot?: Snapshot;
  detail?: string;
}

export class HouseService {
  private readonly repo: Repo;
  private readonly rest: DiscordRest;
  private readonly config: HouseServiceConfig;
  private readonly airtable: AirtableConfig;
  private readonly fetchImpl?: typeof fetch;
  private readonly nowFn: () => number;

  constructor(deps: HouseServiceDeps) {
    this.repo = deps.repo;
    this.rest = deps.rest;
    this.config = deps.config;
    this.airtable = deps.airtable;
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
    await this.syncAirtable(row, snapshot);

    return { message: `tracking → <#${thread.id}>` };
  }

  /**
   * Shared refresh path for `/house update` and the cron.
   * Posts a change notice only when a material field moved.
   */
  async refresh(
    row: PropertyRow,
    source: 'manual' | 'scheduled',
    overrideUrl?: string,
  ): Promise<RefreshOutcome> {
    const now = this.now();
    const url = overrideUrl ?? row.source_url;
    // An explicit override URL means "ignore my cached validators".
    const opts = overrideUrl ? this.fetchOpts() : this.fetchOpts(row);
    const result = await fetchListing(url, opts, now);

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
      return { kind: 'not-modified', changes: [] };
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

    await this.syncAirtable(row, next);

    return {
      kind: changes.length > 0 ? 'changed' : 'unchanged',
      changes,
      snapshot: next,
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

  /** `/house status` — pure D1 read, no outgoing fetch. */
  status(row: PropertyRow): { message: string } {
    const s = parseSnapshot(row);
    const closed = isClosed({
      forceClosed: row.force_closed === 1,
      listingStatus: (row.status as ListingStatus) ?? 'unknown',
    });
    const lines = [
      `${closed ? '❌ ' : ''}**${row.title ?? row.listing_key}**`,
      `**Status:** ${statusLabel((row.status as ListingStatus) ?? 'unknown')}${
        row.force_closed === 1 ? ' (force-closed)' : ''
      }`,
      `**Source:** ${row.source_url}`,
      `**Listing key:** \`${row.listing_key}\``,
      row.last_checked_at
        ? `**Last checked:** <t:${row.last_checked_at}:R>`
        : '**Last checked:** never',
      row.last_changed_at
        ? `**Last change:** <t:${row.last_changed_at}:R>`
        : '**Last change:** none recorded',
      `**Next scheduled check:** <t:${row.next_check_at}:R>`,
    ];
    if (row.fail_count > 0) {
      lines.push(`**Consecutive failures:** ${row.fail_count} — ${row.last_error ?? 'unknown'}`);
    }
    if (s?.hvac) lines.push(`**HVAC (unverified):** ${s.hvac}`);
    return { message: lines.join('\n') };
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

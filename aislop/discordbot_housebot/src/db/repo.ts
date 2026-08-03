import type { FieldChange, ListingStatus, Snapshot } from '../listing/types';

export interface PropertyRow {
  id: number;
  listing_key: string;
  provider: string;
  listing_id: string;
  source_url: string;
  guild_id: string | null;
  parent_channel_id: string;
  thread_id: string;
  snapshot_json: string | null;
  status: string;
  title: string | null;
  force_closed: number;
  etag: string | null;
  last_modified: string | null;
  last_checked_at: number | null;
  last_changed_at: number | null;
  next_check_at: number;
  fail_count: number;
  last_error: string | null;
  created_at: number;
  updated_at: number;
}

export interface SnapshotRow {
  id: number;
  property_id: number;
  snapshot_json: string;
  changes_json: string;
  source: string;
  created_at: number;
}

export function parseSnapshot(row: PropertyRow | null): Snapshot | null {
  if (!row?.snapshot_json) return null;
  try {
    return JSON.parse(row.snapshot_json) as Snapshot;
  } catch {
    return null;
  }
}

export class Repo {
  constructor(private readonly db: D1Database) {}

  getByListingKey(listingKey: string): Promise<PropertyRow | null> {
    return this.db
      .prepare('SELECT * FROM properties WHERE listing_key = ?1')
      .bind(listingKey)
      .first<PropertyRow>();
  }

  getByThreadId(threadId: string): Promise<PropertyRow | null> {
    return this.db
      .prepare('SELECT * FROM properties WHERE thread_id = ?1')
      .bind(threadId)
      .first<PropertyRow>();
  }

  async insertProperty(input: {
    snapshot: Snapshot;
    guildId: string | null;
    parentChannelId: string;
    threadId: string;
    title: string;
    etag?: string | null;
    lastModified?: string | null;
    now: number;
    nextCheckAt: number;
  }): Promise<PropertyRow> {
    const s = input.snapshot;
    await this.db
      .prepare(
        `INSERT INTO properties
           (listing_key, provider, listing_id, source_url, guild_id, parent_channel_id,
            thread_id, snapshot_json, status, title, force_closed, etag, last_modified,
            last_checked_at, last_changed_at, next_check_at, fail_count, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,?11,?12,?13,?13,?14,0,?13,?13)`,
      )
      .bind(
        s.listingKey,
        s.provider,
        s.listingId,
        s.sourceUrl,
        input.guildId,
        input.parentChannelId,
        input.threadId,
        JSON.stringify(s),
        s.status,
        input.title,
        input.etag ?? null,
        input.lastModified ?? null,
        input.now,
        input.nextCheckAt,
      )
      .run();

    const row = await this.getByListingKey(s.listingKey);
    if (!row) throw new Error('insert failed: property not found after write');
    await this.recordSnapshot(row.id, s, [], 'add', input.now);
    return row;
  }

  async applyFetchSuccess(input: {
    propertyId: number;
    snapshot: Snapshot;
    title: string;
    changes: FieldChange[];
    etag?: string | null;
    lastModified?: string | null;
    now: number;
    nextCheckAt: number;
    source: 'manual' | 'scheduled';
  }): Promise<void> {
    const changed = input.changes.length > 0;
    await this.db
      .prepare(
        `UPDATE properties SET
           snapshot_json = ?2, status = ?3, title = ?4, etag = ?5, last_modified = ?6,
           last_checked_at = ?7, next_check_at = ?8, fail_count = 0, last_error = NULL,
           last_changed_at = CASE WHEN ?9 = 1 THEN ?7 ELSE last_changed_at END,
           updated_at = ?7
         WHERE id = ?1`,
      )
      .bind(
        input.propertyId,
        JSON.stringify(input.snapshot),
        input.snapshot.status,
        input.title,
        input.etag ?? null,
        input.lastModified ?? null,
        input.now,
        input.nextCheckAt,
        changed ? 1 : 0,
      )
      .run();

    if (changed) {
      await this.recordSnapshot(
        input.propertyId,
        input.snapshot,
        input.changes,
        input.source,
        input.now,
      );
    }
  }

  async touchChecked(propertyId: number, now: number, nextCheckAt: number): Promise<void> {
    await this.db
      .prepare(
        `UPDATE properties SET last_checked_at = ?2, next_check_at = ?3,
           fail_count = 0, last_error = NULL, updated_at = ?2 WHERE id = ?1`,
      )
      .bind(propertyId, now, nextCheckAt)
      .run();
  }

  async applyFetchFailure(
    propertyId: number,
    error: string,
    now: number,
    nextCheckAt: number,
  ): Promise<void> {
    await this.db
      .prepare(
        `UPDATE properties SET fail_count = fail_count + 1, last_error = ?2,
           last_checked_at = ?3, next_check_at = ?4, updated_at = ?3 WHERE id = ?1`,
      )
      .bind(propertyId, error.slice(0, 500), now, nextCheckAt)
      .run();
  }

  async setForceClosed(
    propertyId: number,
    forceClosed: boolean,
    title: string,
    now: number,
  ): Promise<void> {
    await this.db
      .prepare(
        'UPDATE properties SET force_closed = ?2, title = ?3, updated_at = ?4 WHERE id = ?1',
      )
      .bind(propertyId, forceClosed ? 1 : 0, title, now)
      .run();
  }

  async setStatus(propertyId: number, status: ListingStatus, now: number): Promise<void> {
    await this.db
      .prepare('UPDATE properties SET status = ?2, updated_at = ?3 WHERE id = ?1')
      .bind(propertyId, status, now)
      .run();
  }

  async recordSnapshot(
    propertyId: number,
    snapshot: Snapshot,
    changes: FieldChange[],
    source: string,
    now: number,
  ): Promise<void> {
    await this.db
      .prepare(
        `INSERT INTO snapshots (property_id, snapshot_json, changes_json, source, created_at)
         VALUES (?1,?2,?3,?4,?5)`,
      )
      .bind(propertyId, JSON.stringify(snapshot), JSON.stringify(changes), source, now)
      .run();
  }

  /** Newest-first change history for a property (diagnostics / tests). */
  async listSnapshots(propertyId: number, limit = 20): Promise<SnapshotRow[]> {
    const res = await this.db
      .prepare(
        `SELECT * FROM snapshots WHERE property_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2`,
      )
      .bind(propertyId, limit)
      .all<SnapshotRow>();
    return res.results ?? [];
  }

  /** Bounded due-batch for the cron. Force-closed houses are never refreshed. */
  async listDue(now: number, limit: number): Promise<PropertyRow[]> {
    const res = await this.db
      .prepare(
        `SELECT * FROM properties
          WHERE force_closed = 0 AND next_check_at <= ?1
          ORDER BY next_check_at ASC
          LIMIT ?2`,
      )
      .bind(now, limit)
      .all<PropertyRow>();
    return res.results ?? [];
  }

  /**
   * Idempotency: returns true the first time an interaction id is seen.
   * Discord retries deliveries; this makes handling exactly-once.
   */
  async claimInteraction(interactionId: string, command: string, now: number): Promise<boolean> {
    const res = await this.db
      .prepare(
        `INSERT OR IGNORE INTO interaction_log (interaction_id, command, created_at)
         VALUES (?1,?2,?3)`,
      )
      .bind(interactionId, command, now)
      .run();
    const changes = (res.meta as { changes?: number } | undefined)?.changes;
    return (changes ?? 0) > 0;
  }

  /** Housekeeping so the log doesn't grow forever. Called from the cron. */
  async pruneInteractions(olderThan: number): Promise<void> {
    await this.db
      .prepare('DELETE FROM interaction_log WHERE created_at < ?1')
      .bind(olderThan)
      .run();
  }

  async recordAirtableSync(
    listingKey: string,
    status: string,
    detail: string | null,
    recordId: string | null,
    now: number,
  ): Promise<void> {
    await this.db
      .prepare(
        `INSERT INTO airtable_sync (listing_key, record_id, last_status, last_detail, last_synced_at)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(listing_key) DO UPDATE SET
           record_id = COALESCE(excluded.record_id, airtable_sync.record_id),
           last_status = excluded.last_status,
           last_detail = excluded.last_detail,
           last_synced_at = excluded.last_synced_at`,
      )
      .bind(listingKey, recordId, status, detail?.slice(0, 500) ?? null, now)
      .run();
  }

  getAirtableRecordId(listingKey: string): Promise<{ record_id: string | null } | null> {
    return this.db
      .prepare('SELECT record_id FROM airtable_sync WHERE listing_key = ?1')
      .bind(listingKey)
      .first<{ record_id: string | null }>();
  }
}

/** Exponential backoff on consecutive failures, capped at 24h. */
export function backoffSeconds(failCount: number, baseSeconds: number): number {
  const factor = Math.min(2 ** Math.max(0, failCount), 32);
  return Math.min(baseSeconds * factor, 86400);
}

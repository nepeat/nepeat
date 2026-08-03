import type { Repo } from '../db/repo';
import type { HouseService } from '../service/house';

export interface ScheduledDeps {
  repo: Repo;
  service: HouseService;
  batchSize: number;
  now?: () => number;
  /** Interactions older than this are pruned from the idempotency log. */
  interactionRetentionSeconds?: number;
}

export interface ScheduledReport {
  considered: number;
  changed: number;
  unchanged: number;
  notModified: number;
  failed: number;
  errors: Array<{ listingKey: string; detail: string }>;
}

/**
 * One cron tick.
 *
 * Bounded on purpose: we only take `batchSize` properties whose `next_check_at`
 * is due, so a tick costs at most `batchSize` outbound fetches regardless of how
 * many houses are tracked. Each property is handled independently -- one bad
 * listing never fails the batch -- and failures back off exponentially.
 */
export async function runScheduledRefresh(deps: ScheduledDeps): Promise<ScheduledReport> {
  const now = deps.now?.() ?? Math.floor(Date.now() / 1000);
  const report: ScheduledReport = {
    considered: 0,
    changed: 0,
    unchanged: 0,
    notModified: 0,
    failed: 0,
    errors: [],
  };

  const due = await deps.repo.listDue(now, deps.batchSize);
  report.considered = due.length;

  for (const row of due) {
    try {
      const outcome = await deps.service.refresh(row, 'scheduled');
      switch (outcome.kind) {
        case 'changed':
          report.changed++;
          break;
        case 'unchanged':
          report.unchanged++;
          break;
        case 'not-modified':
          report.notModified++;
          break;
        case 'error':
          report.failed++;
          report.errors.push({ listingKey: row.listing_key, detail: outcome.detail ?? 'unknown' });
          break;
      }
    } catch (err) {
      report.failed++;
      report.errors.push({
        listingKey: row.listing_key,
        detail: err instanceof Error ? err.message : String(err),
      });
    }
  }

  const retention = deps.interactionRetentionSeconds ?? 7 * 86400;
  try {
    await deps.repo.pruneInteractions(now - retention);
  } catch {
    // Housekeeping only; never fail the tick over it.
  }

  return report;
}

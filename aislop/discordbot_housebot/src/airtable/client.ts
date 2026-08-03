import type { Snapshot } from '../listing/types';
import { buildFields, parseFieldMap, type ExportContext } from './fieldmap';

export interface AirtableConfig {
  token?: string;
  baseId?: string;
  table?: string;
  fieldMapJson?: string;
  fetchImpl?: typeof fetch;
  apiBase?: string;
  timeoutMs?: number;
}

export type AirtableSyncResult =
  | { status: 'ok'; recordId: string | null; detail: string }
  | { status: 'skipped'; detail: string }
  | { status: 'error'; detail: string };

const API_BASE = 'https://api.airtable.com/v0';

/**
 * Best-effort, idempotent Airtable upsert keyed on the canonical listing key.
 *
 * Every failure mode -- unconfigured, bad map, network, HTTP error -- returns a
 * traceable result instead of throwing, so the Discord path never depends on
 * Airtable being up. The token is never echoed into any returned detail.
 */
export async function syncListing(
  snapshot: Snapshot,
  cfg: AirtableConfig,
  ctx: ExportContext = {},
): Promise<AirtableSyncResult> {
  if (!cfg.token) return { status: 'skipped', detail: 'AIRTABLE_TOKEN not set' };
  if (!cfg.baseId) return { status: 'skipped', detail: 'AIRTABLE_BASE_ID not set' };
  if (!cfg.table) return { status: 'skipped', detail: 'AIRTABLE_TABLE not set' };

  const fm = parseFieldMap(cfg.fieldMapJson);
  if (!fm.ok) return { status: 'skipped', detail: fm.problem ?? 'field map unusable' };

  const keyField = fm.map.key as string;
  const fields = buildFields(snapshot, fm.map, ctx);
  if (Object.keys(fields).length === 0) {
    return { status: 'skipped', detail: 'field map produced no writable fields' };
  }

  const url = `${cfg.apiBase ?? API_BASE}/${encodeURIComponent(cfg.baseId)}/${encodeURIComponent(
    cfg.table,
  )}`;
  const body = {
    performUpsert: { fieldsToMergeOn: [keyField] },
    records: [{ fields }],
    typecast: true,
  };

  const doFetch = cfg.fetchImpl ?? fetch;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), cfg.timeoutMs ?? 6000);
  try {
    const res = await doFetch(url, {
      method: 'PATCH',
      headers: {
        Authorization: `Bearer ${cfg.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    });

    if (!res.ok) {
      const text = await res.text().catch(() => '');
      return {
        status: 'error',
        detail: `airtable HTTP ${res.status}: ${redact(text).slice(0, 200)}`,
      };
    }
    const json = (await res.json().catch(() => null)) as {
      records?: Array<{ id?: string }>;
      createdRecords?: string[];
      updatedRecords?: string[];
    } | null;
    const recordId = json?.records?.[0]?.id ?? null;
    const created = (json?.createdRecords ?? []).length > 0;
    return {
      status: 'ok',
      recordId,
      detail: created ? 'created' : 'updated',
    };
  } catch (err) {
    const aborted = controller.signal.aborted;
    return {
      status: 'error',
      detail: aborted ? 'airtable timeout' : redact(err instanceof Error ? err.message : String(err)),
    };
  } finally {
    clearTimeout(timer);
  }
}

/** Defensive: never let a bearer token reach a log line or a Discord message. */
function redact(text: string): string {
  return text.replace(/pat[A-Za-z0-9._-]{6,}/g, 'pat***').replace(/Bearer\s+\S+/gi, 'Bearer ***');
}

import { renderField } from './format';
import { MATERIAL_FIELDS, type FieldChange, type Snapshot } from './types';

/**
 * Material change set between two snapshots. Only fields in MATERIAL_FIELDS
 * count, and a field going from known -> unknown is ignored: providers drop
 * data from public pages all the time and we refuse to announce that as news.
 */
export function computeChanges(
  prev: Partial<Snapshot> | null,
  next: Partial<Snapshot>,
): FieldChange[] {
  if (!prev) return [];
  const changes: FieldChange[] = [];
  for (const field of MATERIAL_FIELDS) {
    const before = renderField(field, prev);
    const after = renderField(field, next);
    if (after === null) continue; // lost data is not a change
    if (before === after) continue;
    changes.push({ field, from: before, to: after });
  }
  return changes;
}

export function hasMaterialChange(
  prev: Partial<Snapshot> | null,
  next: Partial<Snapshot>,
): boolean {
  return computeChanges(prev, next).length > 0;
}

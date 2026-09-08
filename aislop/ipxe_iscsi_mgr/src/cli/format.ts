/**
 * Table formatting for list views. Split out of index.ts so it can be unit
 * tested -- importing index.ts would run the CLI.
 */

/** Unix seconds -> readable. Bare epochs in a --long dump are unreadable. */
export function fmtCell(col: string, v: unknown): string {
  if (v === null || v === undefined) return '';
  if ((col.endsWith('_seen') || col === 'ts' || col.endsWith('_at')) && typeof v === 'number' && v > 1e9) {
    return new Date(v * 1000).toISOString().replace('T', ' ').slice(0, 19);
  }
  return String(v);
}

/**
 * Every column the API actually returned, in the order the rows present them
 * (SQLite gives schema order). Used by --long so new columns appear without
 * anyone having to remember to add them here.
 */
export function allColumns(rows: Record<string, unknown>[]): string[] {
  const seen = new Set<string>();
  for (const r of rows) for (const k of Object.keys(r)) seen.add(k);
  return [...seen];
}

/** Columns for a list view: the curated short set, or everything with --long. */
export function listCols(rows: Record<string, unknown>[], short: string[], long: boolean): string[] {
  return long ? allColumns(rows) : short;
}

/** Render rows as an aligned table. Wide output is expected under --long. */
export function renderTable(rows: Record<string, unknown>[], cols: string[]): string {
  if (!rows.length) return '(none)';
  const cells = rows.map((r) => cols.map((c) => fmtCell(c, r[c])));
  const w = cols.map((c, i) => Math.max(c.length, ...cells.map((row) => row[i]!.length)));
  const line = (vals: string[]) => vals.map((c, i) => c.padEnd(w[i]!)).join('  ').trimEnd();
  return [line(cols), line(w.map((n) => '-'.repeat(n))), ...cells.map(line)].join('\n');
}

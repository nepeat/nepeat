import { describe, expect, it } from 'vitest';
import { allColumns, fmtCell, listCols, renderTable } from '../src/cli/format.js';

const rows = [
  { name: 'a', mem_mb: 4096, last_seen: 1787711607, notes: null },
  { name: 'bb', mem_mb: null, last_seen: 1787711999, notes: 'hi' },
];

describe('fmtCell', () => {
  it('humanises unix-second timestamp columns', () => {
    expect(fmtCell('last_seen', 1787711607)).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
    expect(fmtCell('first_seen', 1787711607)).toContain('-');
    expect(fmtCell('created_at', 1787711607)).toContain('-');
  });

  it('leaves non-timestamp numbers alone, even large ones', () => {
    expect(fmtCell('size_bytes', 68719476736)).toBe('68719476736');
    expect(fmtCell('mem_mb', 4096)).toBe('4096');
  });

  it('does not mangle a small number in a timestamp-named column', () => {
    expect(fmtCell('last_seen', 0)).toBe('0');
  });

  it('renders null and undefined as empty, not "null"', () => {
    expect(fmtCell('notes', null)).toBe('');
    expect(fmtCell('notes', undefined)).toBe('');
  });
});

describe('--long column selection', () => {
  it('returns the curated set when long is false', () => {
    expect(listCols(rows, ['name', 'mem_mb'], false)).toEqual(['name', 'mem_mb']);
  });

  it('returns every field when long is true', () => {
    expect(listCols(rows, ['name'], true)).toEqual(['name', 'mem_mb', 'last_seen', 'notes']);
  });

  it('unions columns across rows, so a field only some rows have still shows', () => {
    expect(allColumns([{ a: 1 }, { b: 2 }])).toEqual(['a', 'b']);
  });

  it('preserves the order the API returned (schema order), not alphabetical', () => {
    expect(allColumns([{ zebra: 1, apple: 2 }])).toEqual(['zebra', 'apple']);
  });
});

describe('renderTable', () => {
  it('aligns columns and underlines the header', () => {
    const out = renderTable(rows, ['name', 'mem_mb']).split('\n');
    expect(out[0]).toBe('name  mem_mb');
    expect(out[1]).toBe('----  ------');
    expect(out[2]).toBe('a     4096');
  });

  it('does not leave trailing whitespace on short rows', () => {
    for (const line of renderTable(rows, ['name', 'notes']).split('\n')) {
      expect(line).toBe(line.trimEnd());
    }
  });

  it('says (none) rather than printing a bare header for an empty set', () => {
    expect(renderTable([], ['name'])).toBe('(none)');
  });
});

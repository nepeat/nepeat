import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Required at runtime so the bundler does not try to resolve node:sqlite.
const require = createRequire(import.meta.url);
const { DatabaseSync } = require('node:sqlite') as typeof import('node:sqlite');
type DatabaseSync = InstanceType<typeof DatabaseSync>;

/**
 * Minimal D1-compatible adapter over node:sqlite so tests exercise the real
 * SQL and the real migration file instead of a hand-rolled mock.
 */
class FakeStatement {
  private args: unknown[] = [];
  constructor(
    private readonly db: DatabaseSync,
    private readonly sql: string,
  ) {}

  bind(...args: unknown[]): FakeStatement {
    this.args = args.map((a) => (a === undefined ? null : a));
    return this;
  }

  async first<T>(): Promise<T | null> {
    const stmt = this.db.prepare(this.sql);
    const row = stmt.get(...(this.args as never[]));
    return (row as T) ?? null;
  }

  async all<T>(): Promise<{ results: T[]; success: true }> {
    const stmt = this.db.prepare(this.sql);
    const rows = stmt.all(...(this.args as never[]));
    return { results: rows as T[], success: true };
  }

  async run(): Promise<{ success: true; meta: { changes: number; last_row_id: number } }> {
    const stmt = this.db.prepare(this.sql);
    const info = stmt.run(...(this.args as never[]));
    return {
      success: true,
      meta: {
        changes: Number(info.changes),
        last_row_id: Number(info.lastInsertRowid),
      },
    };
  }
}

export class FakeD1 {
  readonly db: DatabaseSync;

  constructor() {
    this.db = new DatabaseSync(':memory:');
    for (const name of ['0001_init.sql', '0002_geo_and_enrichment.sql']) {
      const path = fileURLToPath(new URL(`../../migrations/${name}`, import.meta.url));
      this.db.exec(readFileSync(path, 'utf8'));
    }
  }

  prepare(sql: string): FakeStatement {
    return new FakeStatement(this.db, sql);
  }
}

/** Cast helper: FakeD1 implements the slice of D1Database the repo uses. */
export function asD1(fake: FakeD1): never {
  return fake as never;
}

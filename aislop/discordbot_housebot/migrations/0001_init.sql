-- housebot initial schema
-- Every lookup path used at runtime is covered by an index; the hot paths are
-- (1) dedupe by canonical key, (2) resolve a thread id -> property, (3) pick the
-- next due batch for the cron.

CREATE TABLE properties (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  -- provider + listing id, e.g. "zillow:12345678". Canonical dedupe key.
  listing_key       TEXT    NOT NULL,
  provider          TEXT    NOT NULL,
  listing_id        TEXT    NOT NULL,
  -- honest, normalized source URL (no tracking params, no fabrication)
  source_url        TEXT    NOT NULL,

  guild_id          TEXT,
  parent_channel_id TEXT    NOT NULL,
  thread_id         TEXT    NOT NULL,

  -- latest normalized snapshot, JSON-encoded (see src/listing/types.ts)
  snapshot_json     TEXT,
  -- derived, denormalized for cheap queries / title rebuilds
  status            TEXT    NOT NULL DEFAULT 'unknown',
  title             TEXT,
  -- operator forced this house closed; survives listing flapping
  force_closed      INTEGER NOT NULL DEFAULT 0,

  -- fetch bookkeeping
  etag              TEXT,
  last_modified     TEXT,
  last_checked_at   INTEGER,          -- unix seconds, any fetch attempt
  last_changed_at   INTEGER,          -- unix seconds, material field change
  next_check_at     INTEGER NOT NULL, -- unix seconds, cron due-at
  fail_count        INTEGER NOT NULL DEFAULT 0,
  last_error        TEXT,

  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_properties_listing_key ON properties (listing_key);
CREATE UNIQUE INDEX idx_properties_thread_id   ON properties (thread_id);
-- cron picks live rows by due time; force-closed rows are excluded in SQL.
CREATE INDEX idx_properties_due ON properties (force_closed, next_check_at);

-- Append-only change history. One row per observed material change set.
CREATE TABLE snapshots (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  property_id   INTEGER NOT NULL REFERENCES properties (id) ON DELETE CASCADE,
  snapshot_json TEXT    NOT NULL,
  -- JSON array of {field, from, to}; empty array for the initial snapshot
  changes_json  TEXT    NOT NULL DEFAULT '[]',
  source        TEXT    NOT NULL DEFAULT 'manual', -- manual | scheduled | add
  created_at    INTEGER NOT NULL
);

CREATE INDEX idx_snapshots_property ON snapshots (property_id, created_at DESC);

-- Discord retries interactions; this makes handling exactly-once.
CREATE TABLE interaction_log (
  interaction_id TEXT PRIMARY KEY,
  command        TEXT,
  created_at     INTEGER NOT NULL
);

CREATE INDEX idx_interaction_log_created ON interaction_log (created_at);

-- Best-effort Airtable sync state; never blocks the Discord path.
CREATE TABLE airtable_sync (
  listing_key   TEXT PRIMARY KEY,
  record_id     TEXT,
  last_status   TEXT NOT NULL,   -- ok | skipped | error
  last_detail   TEXT,
  last_synced_at INTEGER NOT NULL
);

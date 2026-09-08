-- Core schema. Uniqueness constraints here are load-bearing: the Worker does
-- not validate, so double-assignment must be impossible at the storage layer.

CREATE TABLE hosts (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL UNIQUE,
  state          TEXT NOT NULL DEFAULT 'pending',   -- pending | approved | disabled
  smbios_uuid    TEXT,
  serial         TEXT,
  asset_tag      TEXT,
  manufacturer   TEXT,
  product        TEXT,
  cpu_model      TEXT,
  mem_mb         INTEGER,
  arch           TEXT NOT NULL DEFAULT 'x86_64',
  platform       TEXT NOT NULL DEFAULT 'efi',
  initiator_iqn  TEXT NOT NULL UNIQUE,
  boot_profile_id TEXT,
  first_seen     INTEGER NOT NULL,
  last_seen      INTEGER NOT NULL,
  boot_count     INTEGER NOT NULL DEFAULT 0,
  notes          TEXT
);
-- Partial uniqueness: many hosts may have no UUID, but a present one is unique.
CREATE UNIQUE INDEX idx_hosts_uuid ON hosts(smbios_uuid) WHERE smbios_uuid IS NOT NULL;
CREATE INDEX idx_hosts_serial ON hosts(serial) WHERE serial IS NOT NULL;
CREATE INDEX idx_hosts_state ON hosts(state);

CREATE TABLE host_macs (
  mac        TEXT PRIMARY KEY,          -- normalised aa:bb:cc:dd:ee:ff
  host_id    TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
  first_seen INTEGER NOT NULL
);
CREATE INDEX idx_host_macs_host ON host_macs(host_id);

CREATE TABLE volumes (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL UNIQUE,
  provider     TEXT NOT NULL DEFAULT 'zfs',
  zvol_path    TEXT NOT NULL UNIQUE,     -- one zvol backs exactly one volume
  portal_host  TEXT NOT NULL,
  portal_port  INTEGER NOT NULL DEFAULT 3260,
  lun          INTEGER NOT NULL DEFAULT 0,
  target_iqn   TEXT NOT NULL,
  size_bytes   INTEGER,
  state        TEXT NOT NULL DEFAULT 'active',
  notes        TEXT,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
-- The same target IQN may expose several LUNs, but never the same LUN twice.
CREATE UNIQUE INDEX idx_volumes_target_lun ON volumes(target_iqn, lun);

CREATE TABLE assignments (
  id               TEXT PRIMARY KEY,
  volume_id        TEXT NOT NULL UNIQUE REFERENCES volumes(id) ON DELETE CASCADE,
  mode             TEXT NOT NULL,        -- pinned | general-exclusive | shared-ro
  host_id          TEXT REFERENCES hosts(id) ON DELETE CASCADE,
  lease_host_id    TEXT REFERENCES hosts(id) ON DELETE SET NULL,
  lease_expires_at INTEGER,
  priority         INTEGER NOT NULL DEFAULT 100,
  -- A pinned assignment must name a host; a pooled one must not.
  CHECK ((mode = 'pinned' AND host_id IS NOT NULL) OR (mode <> 'pinned' AND host_id IS NULL))
);
CREATE INDEX idx_assignments_host ON assignments(host_id);
CREATE INDEX idx_assignments_mode ON assignments(mode, priority);

CREATE TABLE boot_profiles (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL UNIQUE,
  arch           TEXT NOT NULL,
  efi_candidates TEXT NOT NULL,          -- JSON array, ordered
  extra_ipxe     TEXT
);

CREATE TABLE secrets (
  id           TEXT PRIMARY KEY,
  kind         TEXT NOT NULL DEFAULT 'boot',   -- boot | enrollment
  hash         TEXT NOT NULL UNIQUE,           -- sha256 hex of the secret
  label        TEXT,
  created_at   INTEGER NOT NULL,
  last_used_at INTEGER,
  disabled_at  INTEGER
);

CREATE TABLE boot_events (
  id        TEXT PRIMARY KEY,
  ts        INTEGER NOT NULL,
  host_id   TEXT REFERENCES hosts(id) ON DELETE SET NULL,
  volume_id TEXT REFERENCES volumes(id) ON DELETE SET NULL,
  outcome   TEXT NOT NULL,
  detail    TEXT
);
CREATE INDEX idx_boot_events_ts ON boot_events(ts DESC);
CREATE INDEX idx_boot_events_host ON boot_events(host_id, ts DESC);

CREATE TABLE audit_log (
  id      TEXT PRIMARY KEY,
  ts      INTEGER NOT NULL,
  actor   TEXT,
  action  TEXT NOT NULL,
  subject TEXT,
  detail  TEXT,
  forced  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_audit_ts ON audit_log(ts DESC);

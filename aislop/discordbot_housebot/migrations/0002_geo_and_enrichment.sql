-- Coordinates + derived enrichment.
--
-- Zillow ships offers.itemOffered.geo on the public page, so coordinates are
-- part of the listing observation and live on `properties`. Everything DERIVED
-- from them lives in `enrichment` instead: mixing computed values into
-- snapshot_json would corrupt the change-detection diff, which compares
-- observations of the listing, not facts about the location.

ALTER TABLE properties ADD COLUMN lat REAL;
ALTER TABLE properties ADD COLUMN lon REAL;

CREATE TABLE enrichment (
  property_id INTEGER NOT NULL REFERENCES properties (id) ON DELETE CASCADE,
  -- commute | hvac | transit | isp
  kind        TEXT    NOT NULL,
  -- ok | unverified | unavailable
  status      TEXT    NOT NULL,
  -- where the value came from, or why it is missing. Always populated.
  provenance  TEXT    NOT NULL,
  -- kind-specific payload, JSON-encoded. Null when status = unavailable.
  value_json  TEXT,
  computed_at INTEGER NOT NULL,
  PRIMARY KEY (property_id, kind)
);

CREATE INDEX idx_enrichment_kind ON enrichment (kind, computed_at);

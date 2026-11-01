import datetime as dt
import hashlib
import json
import re
import sqlite3
from collections import namedtuple
from pathlib import Path

_SnapshotMetadata = namedtuple('_SnapshotMetadata', 'fetched_at sha256 cache_path')

TARGETS = {
    "Accton": ["accton", "accton technology", "accton technology corp", "accton technology corporation"],
    "Edge-Core": ["edgecore", "edge core", "edge core networks", "edgecore networks", "edgecore networks corporation", "edgecore networks india pvt ltd"],
    "Celestica": ["celestica", "celestica inc", "celestica international", "celestica thailand limited", "celestica technology consultancy shanghai co ltd", "celestica technology consultancy shanghai co"],
    "Oracle": ["oracle", "oracle america", "oracle corporation", "oracle america inc", "oracle india private limited"],
    "Amazon Data Services": ["amazon data services", "amazon data services inc"],
    # Reviewed exact names only; subsidiaries and similarly named companies
    # are not aliases. See research/odm-target-registry.md.
    "Inventec": ["inventec", "inventec corporation"],
    "Hyve": ["hyve", "hyve solutions", "hyve solutions corporation"],
    "Foxconn": ["foxconn"],
    "Wiwynn": ["wiwynn", "wiwynn corporation"],
    "TD SYNNEX": ["td synnex", "td synnex corporation", "synnex", "synnex corporation"],
    "Quanta": ["quanta", "quanta computer inc"],
    "ByteDance": ["bytedance"],
    "Inspur": ["inspur", "inspur electronic information industry co ltd"],
}


def normalize(value):
    return re.sub(r"[^a-z0-9]", "", str(value).lower())


def target_for(name):
    key = normalize(name)
    return next((target for target, aliases in TARGETS.items()
                 if key in {normalize(a) for a in aliases + [target]}), None)


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


def digest(data):
    return hashlib.sha256(data).hexdigest()


SCHEMA = """
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS sources (
 id TEXT PRIMARY KEY, url TEXT NOT NULL, tier TEXT NOT NULL, license TEXT NOT NULL,
 coverage TEXT NOT NULL, fetched_at TEXT, sha256 TEXT, cache_path TEXT,
 upstream_revision TEXT, records_seen INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS devices (
 id TEXT PRIMARY KEY, brand TEXT NOT NULL, model TEXT NOT NULL,
 revision TEXT NOT NULL DEFAULT '', title TEXT NOT NULL, kind TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS records (
 id TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES devices(id),
 source_id TEXT NOT NULL REFERENCES sources(id), source_key TEXT NOT NULL,
 source_url TEXT NOT NULL, observed_at TEXT NOT NULL, raw_json TEXT NOT NULL,
 UNIQUE(source_id, source_key)
);
CREATE TABLE IF NOT EXISTS affiliations (
 record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
 company TEXT NOT NULL, target TEXT, role TEXT NOT NULL,
 PRIMARY KEY(record_id, company, role)
);
CREATE TABLE IF NOT EXISTS identifiers (
 record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
 kind TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(record_id, kind, value)
);
CREATE TABLE IF NOT EXISTS facts (
 record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
 name TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(record_id, name, value)
);
CREATE TABLE IF NOT EXISTS hyperscaler_evidence (
 record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
 operator TEXT NOT NULL, relationship TEXT NOT NULL,
 detail TEXT NOT NULL, PRIMARY KEY(record_id, operator, relationship)
);
CREATE INDEX IF NOT EXISTS records_device ON records(device_id);
CREATE INDEX IF NOT EXISTS affiliations_target ON affiliations(target);
CREATE INDEX IF NOT EXISTS identifiers_value ON identifiers(value);
CREATE TABLE IF NOT EXISTS fcc_applications (
 record_id TEXT PRIMARY KEY REFERENCES records(id) ON DELETE CASCADE,
 fcc_id TEXT NOT NULL, application_id TEXT NOT NULL, grantee TEXT NOT NULL,
 frn TEXT NOT NULL, purpose TEXT NOT NULL, grant_date TEXT NOT NULL,
 frequency_mhz TEXT NOT NULL, reviewed_at TEXT NOT NULL,
 exhibit_scope TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS fcc_exhibits (
 record_id TEXT NOT NULL REFERENCES fcc_applications(record_id) ON DELETE CASCADE,
 exhibit_key TEXT NOT NULL, document_id TEXT, title TEXT NOT NULL,
 document_type TEXT NOT NULL, availability TEXT NOT NULL,
 available_date TEXT, document_url TEXT,
 PRIMARY KEY(record_id, exhibit_key)
);
CREATE TABLE IF NOT EXISTS fcc_grantees (
 grantee_code TEXT PRIMARY KEY, grantee_name TEXT NOT NULL,
 mailing_address TEXT NOT NULL, po_box TEXT NOT NULL, city TEXT NOT NULL,
 state TEXT NOT NULL, country TEXT NOT NULL, zip_code TEXT NOT NULL,
 contact_name TEXT NOT NULL, date_received TEXT NOT NULL,
 source_id TEXT NOT NULL REFERENCES sources(id)
);
CREATE INDEX IF NOT EXISTS fcc_grantees_name ON fcc_grantees(grantee_name COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS equipment_register (
 source_id TEXT NOT NULL REFERENCES sources(id), page INTEGER NOT NULL,
 row_number INTEGER NOT NULL, name TEXT, model TEXT, category TEXT,
 approval_number TEXT, parse_status TEXT NOT NULL, cells_json TEXT NOT NULL,
 source_url TEXT NOT NULL, PRIMARY KEY(source_id,page,row_number)
);
CREATE INDEX IF NOT EXISTS equipment_register_model ON equipment_register(model COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS ntra_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), page INTEGER NOT NULL,
 table_number INTEGER NOT NULL, row_number INTEGER NOT NULL,
 registry_number TEXT, product_description TEXT, brand TEXT, model TEXT, linked_models TEXT,
 parse_status TEXT NOT NULL, cells_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,page,table_number,row_number)
);
CREATE INDEX IF NOT EXISTS ntra_equipment_model ON ntra_equipment(model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS ntra_equipment_brand ON ntra_equipment(brand COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS bsmi_certificates (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 vendor_code TEXT NOT NULL, certificate_number TEXT NOT NULL, applicant TEXT NOT NULL,
 expiry_raw TEXT NOT NULL, product_name TEXT NOT NULL, primary_model TEXT NOT NULL,
 status_raw TEXT NOT NULL, PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS bsmi_certificates_model ON bsmi_certificates(primary_model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS bsmi_certificates_applicant ON bsmi_certificates(applicant COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS bsmi_product_certificates (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 certificate_number TEXT NOT NULL, applicant TEXT NOT NULL, expiry_raw TEXT NOT NULL,
 contact_name TEXT NOT NULL, contact_phone TEXT NOT NULL, product_name TEXT NOT NULL,
 primary_model TEXT NOT NULL, status_raw TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS bsmi_product_certificates_primary_model ON bsmi_product_certificates(primary_model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS bsmi_product_certificates_applicant ON bsmi_product_certificates(applicant COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS bsmi_voluntary_certificates (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 certificate_number TEXT NOT NULL, applicant TEXT NOT NULL, expiry_raw TEXT NOT NULL, product_name TEXT NOT NULL, primary_model TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS bsmi_voluntary_certificates_primary_model ON bsmi_voluntary_certificates(primary_model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS bsmi_voluntary_certificates_applicant ON bsmi_voluntary_certificates(applicant COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS bsmi_declarations (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 registration_number TEXT NOT NULL, company_name TEXT NOT NULL, business_number TEXT NOT NULL,
 product_name TEXT NOT NULL, company_address TEXT NOT NULL, contact_phone TEXT NOT NULL,
 primary_model TEXT NOT NULL, application_date_raw TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS bsmi_declarations_primary_model ON bsmi_declarations(primary_model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS bsmi_declarations_company_name ON bsmi_declarations(company_name COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS ncc_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 equipment_type TEXT NOT NULL, year_suffix_raw TEXT NOT NULL,
 certificate_number TEXT NOT NULL, trade_certificate_number TEXT NOT NULL,
 equipment_name TEXT NOT NULL, brand TEXT NOT NULL, model TEXT NOT NULL,
 applicant TEXT NOT NULL, manufacturer TEXT NOT NULL, certification_body TEXT NOT NULL,
 sar_raw TEXT NOT NULL, camera_raw TEXT NOT NULL, camera_sound_raw TEXT NOT NULL,
 charger TEXT NOT NULL, battery TEXT NOT NULL, approval_date_raw TEXT NOT NULL,
 renewal_date_raw TEXT NOT NULL, test_lab TEXT NOT NULL, test_lab_country TEXT NOT NULL,
 certificate_validity_raw TEXT NOT NULL, revocation_date_raw TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS ncc_equipment_model ON ncc_equipment(model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS ncc_equipment_manufacturer ON ncc_equipment(manufacturer COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS tec_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 applicant TEXT NOT NULL, product_name TEXT NOT NULL, product_variant TEXT NOT NULL,
 declared_oem TEXT NOT NULL, primary_model TEXT NOT NULL, additional_models TEXT NOT NULL,
 certificate_category TEXT NOT NULL, certificate_type TEXT NOT NULL,
 issue_date_raw TEXT NOT NULL, valid_upto_raw TEXT NOT NULL, certificate_id TEXT NOT NULL,
 issue_date TEXT, valid_upto TEXT, parse_status TEXT NOT NULL, issue TEXT NOT NULL,
 cells_json TEXT NOT NULL, source_url TEXT NOT NULL, PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS tec_equipment_model ON tec_equipment(primary_model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS tec_equipment_oem ON tec_equipment(declared_oem COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS ieee_assignments (
 source_id TEXT NOT NULL REFERENCES sources(id), registry TEXT NOT NULL,
 row_number INTEGER NOT NULL, assignment TEXT NOT NULL, prefix_bits INTEGER NOT NULL,
 organization_name TEXT NOT NULL, organization_address TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,registry,row_number)
);
CREATE INDEX IF NOT EXISTS ieee_assignments_prefix ON ieee_assignments(assignment,prefix_bits);
CREATE TABLE IF NOT EXISTS pci_entries (
 source_id TEXT NOT NULL REFERENCES sources(id), line_number INTEGER NOT NULL,
 entry_kind TEXT NOT NULL, id_path TEXT NOT NULL, name TEXT NOT NULL, comments TEXT NOT NULL,
 vendor_id TEXT NOT NULL, device_id TEXT NOT NULL, subvendor_id TEXT NOT NULL, subdevice_id TEXT NOT NULL,
 class_id TEXT NOT NULL, subclass_id TEXT NOT NULL, interface_id TEXT NOT NULL,
 vendor_name TEXT NOT NULL, subvendor_name TEXT NOT NULL,
 PRIMARY KEY(source_id,line_number), UNIQUE(source_id,entry_kind,id_path)
);
CREATE TABLE IF NOT EXISTS fcc_id_index (
 source_id TEXT NOT NULL REFERENCES sources(id), fcc_id TEXT NOT NULL,
 grantee_code TEXT NOT NULL, grantee_name TEXT NOT NULL, listed_date TEXT NOT NULL,
 product_description TEXT NOT NULL, equipment_class TEXT NOT NULL,
 application_purpose TEXT NOT NULL, source_url TEXT NOT NULL, page_url TEXT NOT NULL,
 PRIMARY KEY(source_id,fcc_id)
);
CREATE TABLE IF NOT EXISTS fcc_detail_pages (
 source_id TEXT NOT NULL REFERENCES sources(id), fcc_id TEXT NOT NULL,
 grantee_code TEXT NOT NULL, grantee_name TEXT NOT NULL, product_description TEXT NOT NULL,
 fields_json TEXT NOT NULL, source_url TEXT NOT NULL, status TEXT NOT NULL, issue TEXT NOT NULL,
 PRIMARY KEY(source_id,fcc_id)
);
CREATE TABLE IF NOT EXISTS fcc_detail_links (
 source_id TEXT NOT NULL, fcc_id TEXT NOT NULL, ordinal INTEGER NOT NULL,
 category TEXT NOT NULL, title TEXT NOT NULL, document_url TEXT NOT NULL,
 PRIMARY KEY(source_id,fcc_id,ordinal),
 FOREIGN KEY(source_id,fcc_id) REFERENCES fcc_detail_pages(source_id,fcc_id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS fcc_text_excerpts (
 source_id TEXT NOT NULL, fcc_id TEXT NOT NULL, ordinal INTEGER NOT NULL,
 category TEXT NOT NULL, text TEXT NOT NULL, truncated INTEGER NOT NULL CHECK(truncated IN (0,1)),
 PRIMARY KEY(source_id,fcc_id,ordinal),
 FOREIGN KEY(source_id,fcc_id) REFERENCES fcc_detail_pages(source_id,fcc_id) ON DELETE CASCADE
);
CREATE VIEW IF NOT EXISTS fcc_excerpt_text AS
 SELECT e.*,p.grantee_name,p.product_description,p.source_url
 FROM fcc_text_excerpts e JOIN fcc_detail_pages p USING(source_id,fcc_id);
CREATE TABLE IF NOT EXISTS documents (
 document_key TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES sources(id),
 fcc_id TEXT NOT NULL, document_id TEXT NOT NULL, title TEXT NOT NULL, category TEXT NOT NULL,
 source_url TEXT NOT NULL, pdf_file TEXT NOT NULL, pdf_sha256 TEXT NOT NULL,
 page_count INTEGER NOT NULL, pages_with_text INTEGER NOT NULL, extraction_method TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS document_pages (
 document_key TEXT NOT NULL REFERENCES documents(document_key) ON DELETE CASCADE,
 page_number INTEGER NOT NULL, text TEXT NOT NULL, text_status TEXT NOT NULL,
 PRIMARY KEY(document_key,page_number)
);
CREATE VIRTUAL TABLE IF NOT EXISTS document_search USING fts5(document_key UNINDEXED, page_number UNINDEXED, text);
CREATE VIEW IF NOT EXISTS document_text AS
 SELECT d.*,p.page_number,p.text,p.text_status FROM documents d JOIN document_pages p USING(document_key);
CREATE TABLE IF NOT EXISTS bahrain_equipment (
 authority_ref TEXT PRIMARY KEY, manufacturer TEXT NOT NULL, model_number TEXT NOT NULL,
 device_description TEXT NOT NULL, marketing_name TEXT, device_remark TEXT,
 authority_remark TEXT, expiry TEXT NOT NULL, expiry_date TEXT NOT NULL,
 status_code TEXT NOT NULL, source_id TEXT NOT NULL REFERENCES sources(id)
);
CREATE INDEX IF NOT EXISTS bahrain_equipment_manufacturer ON bahrain_equipment(manufacturer COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS anatel_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 approval_date TEXT NOT NULL, approval_number TEXT NOT NULL, applicant TEXT NOT NULL,
 applicant_cnpj TEXT NOT NULL, certificate_number TEXT NOT NULL, certificate_date TEXT NOT NULL,
 certificate_expiry TEXT NOT NULL, certificate_status_code TEXT NOT NULL, certificate_status TEXT NOT NULL,
 application_status_code TEXT NOT NULL, application_status TEXT NOT NULL, manufacturer TEXT NOT NULL,
 model TEXT NOT NULL, commercial_name TEXT NOT NULL, product_category TEXT NOT NULL, product_type TEXT NOT NULL,
 antenna_flag TEXT NOT NULL, active_flag TEXT NOT NULL, manufacturer_country TEXT NOT NULL,
 country_itu TEXT NOT NULL, country_iso TEXT NOT NULL, PRIMARY KEY(source_id,row_number)
);
CREATE INDEX IF NOT EXISTS anatel_equipment_model ON anatel_equipment(model COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS anatel_equipment_manufacturer ON anatel_equipment(manufacturer COLLATE NOCASE);
CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(device_id UNINDEXED, text);
CREATE VIEW IF NOT EXISTS target_devices AS
 SELECT DISTINCT d.*, a.target, a.role FROM devices d
 JOIN records r ON r.device_id=d.id JOIN affiliations a ON a.record_id=r.id
 WHERE a.target IS NOT NULL;
CREATE VIEW IF NOT EXISTS device_facts AS
 SELECT d.id AS device_id, d.brand, d.model, f.name, f.value,
 r.source_url, r.source_id, s.tier, r.observed_at
 FROM devices d JOIN records r ON r.device_id=d.id
 JOIN facts f ON f.record_id=r.id JOIN sources s ON s.id=r.source_id;
CREATE VIEW IF NOT EXISTS device_identifiers AS
 SELECT r.device_id, i.kind, i.value, r.source_url
 FROM records r JOIN identifiers i ON i.record_id=r.id;
CREATE TABLE IF NOT EXISTS rra_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 applicant TEXT NOT NULL, equipment_name TEXT NOT NULL, main_model TEXT NOT NULL,
 derived_models TEXT NOT NULL, certificate_number TEXT NOT NULL, manufacturer TEXT NOT NULL,
 country_raw TEXT NOT NULL, approval_date_raw TEXT NOT NULL, other_raw TEXT NOT NULL,
 cells_json TEXT NOT NULL, source_url TEXT NOT NULL, detail_sha256 TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE TABLE IF NOT EXISTS energystar_servers (
 source_id TEXT NOT NULL REFERENCES sources(id), product_id TEXT NOT NULL,
 partner TEXT NOT NULL, brand TEXT NOT NULL, model_name TEXT NOT NULL,
 model_number TEXT NOT NULL, model_identifier TEXT NOT NULL,
 certified_date_raw TEXT NOT NULL, available_date_raw TEXT NOT NULL,
 raw_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,product_id)
);
CREATE TABLE IF NOT EXISTS energystar_server_archives (
 source_id TEXT NOT NULL REFERENCES sources(id), sheet TEXT NOT NULL, row_number INTEGER NOT NULL,
 partner TEXT NOT NULL, brand TEXT NOT NULL, model_name TEXT NOT NULL, model_number TEXT NOT NULL,
 model_identifier TEXT NOT NULL, certified_date_raw TEXT NOT NULL, available_date_raw TEXT NOT NULL,
 fields_json TEXT NOT NULL, cells_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,sheet,row_number)
);
CREATE TABLE IF NOT EXISTS energystar_datacenter (
 source_id TEXT NOT NULL REFERENCES sources(id), dataset_kind TEXT NOT NULL, product_id TEXT NOT NULL,
 partner TEXT NOT NULL, brand TEXT NOT NULL, model_name TEXT NOT NULL, model_number TEXT NOT NULL,
 model_identifier TEXT NOT NULL, certified_date_raw TEXT NOT NULL, available_date_raw TEXT NOT NULL,
 raw_json TEXT NOT NULL, source_url TEXT NOT NULL, sheet TEXT NOT NULL, row_number INTEGER NOT NULL,
 PRIMARY KEY(source_id,product_id)
);
CREATE TABLE IF NOT EXISTS ised_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 company_number TEXT NOT NULL, company_name TEXT NOT NULL, hvin TEXT NOT NULL,
 product_marketing_name TEXT NOT NULL, host_marketing_name TEXT NOT NULL, firmware_version TEXT NOT NULL,
 certification_number TEXT NOT NULL, approval_timestamp_raw TEXT NOT NULL,
 matches_html_identity INTEGER NOT NULL CHECK(matches_html_identity IN (0,1)),
 cells_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE TABLE IF NOT EXISTS nigeria_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 serial_number TEXT NOT NULL, applicant TEXT NOT NULL, certificate_holder TEXT NOT NULL,
 equipment_name TEXT NOT NULL, models TEXT NOT NULL, manufacturer TEXT NOT NULL,
 cells_json TEXT NOT NULL, invalid_cell_bytes_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE TABLE IF NOT EXISTS ghana_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 detail_id TEXT NOT NULL, approval_number TEXT NOT NULL, manufacturer TEXT NOT NULL,
 brand TEXT NOT NULL, product_type TEXT NOT NULL, model TEXT NOT NULL,
 cells_json TEXT NOT NULL, detail_url TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number), UNIQUE(source_id,detail_id)
);
CREATE TABLE IF NOT EXISTS public_equipment_rows (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 approval_number TEXT NOT NULL, brand TEXT NOT NULL, make TEXT NOT NULL,
 device_description TEXT NOT NULL, model TEXT NOT NULL, foreign_certification TEXT NOT NULL,
 cells_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE TABLE IF NOT EXISTS japan_equipment (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 approval_type TEXT NOT NULL, approval_number TEXT NOT NULL, approval_date_raw TEXT NOT NULL,
 registered_name TEXT NOT NULL, equipment_class TEXT NOT NULL, model_field TEXT NOT NULL,
 detail_url TEXT NOT NULL, cells_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE TABLE IF NOT EXISTS fcc_company_rows (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 page_number INTEGER NOT NULL, page_row_number INTEGER NOT NULL,
 company_name TEXT NOT NULL, company_frn TEXT NOT NULL, fcc_id TEXT NOT NULL,
 listed_date TEXT NOT NULL, model_cell TEXT NOT NULL, application_purpose TEXT NOT NULL,
 frequency_raw TEXT NOT NULL, source_url TEXT NOT NULL, web_text_line INTEGER NOT NULL,
 PRIMARY KEY(source_id,row_number)
);
CREATE TABLE IF NOT EXISTS fcc_sitemap_pages (
 source_id TEXT NOT NULL REFERENCES sources(id), sitemap_url TEXT NOT NULL,
 lastmod_raw TEXT NOT NULL, capture_state TEXT NOT NULL, issue TEXT NOT NULL,
 row_count INTEGER, sha256 TEXT NOT NULL,
 PRIMARY KEY(source_id,sitemap_url)
);
CREATE TABLE IF NOT EXISTS fcc_sitemap_urls (
 source_id TEXT NOT NULL, sitemap_url TEXT NOT NULL, ordinal INTEGER NOT NULL,
 record_url TEXT NOT NULL, fcc_id TEXT NOT NULL, page_kind TEXT NOT NULL,
 lastmod_raw TEXT NOT NULL, changefreq_raw TEXT NOT NULL, priority_raw TEXT NOT NULL,
 PRIMARY KEY(source_id,sitemap_url,ordinal),
 FOREIGN KEY(source_id,sitemap_url) REFERENCES fcc_sitemap_pages(source_id,sitemap_url) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS fcc_sitemap_urls_id ON fcc_sitemap_urls(fcc_id);
CREATE TABLE IF NOT EXISTS inspur_support_entries (
 source_id TEXT NOT NULL REFERENCES sources(id), row_number INTEGER NOT NULL,
 product_id TEXT NOT NULL, parent_id TEXT NOT NULL, name TEXT NOT NULL,
 level_raw TEXT, leaf_raw TEXT NOT NULL, raw_json TEXT NOT NULL, source_url TEXT NOT NULL,
 PRIMARY KEY(source_id,row_number), UNIQUE(source_id,product_id)
);
CREATE INDEX IF NOT EXISTS inspur_support_entries_name ON inspur_support_entries(name);
CREATE TABLE IF NOT EXISTS archive_releases (
 release_id TEXT PRIMARY KEY, sha256 TEXT NOT NULL UNIQUE, payload_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS archive_artifacts (
 sha256 TEXT PRIMARY KEY, byte_count INTEGER NOT NULL CHECK(byte_count>=0)
);
CREATE TABLE IF NOT EXISTS archive_release_artifacts (
 release_id TEXT NOT NULL REFERENCES archive_releases(release_id),
 sha256 TEXT NOT NULL REFERENCES archive_artifacts(sha256), path TEXT NOT NULL,
 PRIMARY KEY(release_id,sha256,path)
);
CREATE TABLE IF NOT EXISTS archive_receipts (
 receipt_id TEXT PRIMARY KEY, kind TEXT NOT NULL, created_at TEXT,
 payload_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS archive_release_receipts (
 release_id TEXT NOT NULL REFERENCES archive_releases(release_id),
 receipt_id TEXT NOT NULL REFERENCES archive_receipts(receipt_id),
 PRIMARY KEY(release_id,receipt_id)
);
CREATE TABLE IF NOT EXISTS evidence_statements (
 statement_id TEXT PRIMARY KEY, claim_id TEXT NOT NULL,
 source_id TEXT NOT NULL, source_key TEXT NOT NULL, predicate TEXT NOT NULL,
 raw_value TEXT NOT NULL, normalized_value TEXT, role TEXT NOT NULL,
 projection_kind TEXT NOT NULL CHECK(projection_kind IN ('fact','affiliation')),
 projection_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS evidence_claims ON evidence_statements(claim_id);
CREATE TABLE IF NOT EXISTS evidence_reviews (
 review_id TEXT PRIMARY KEY, statement_id TEXT NOT NULL REFERENCES evidence_statements(statement_id),
 decision TEXT NOT NULL CHECK(decision IN ('accepted','rejected','superseded')),
 reviewer TEXT NOT NULL, reviewed_at TEXT, reason TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS evidence_links (
 link_id TEXT PRIMARY KEY, statement_id TEXT NOT NULL REFERENCES evidence_statements(statement_id),
 artifact_sha256 TEXT NOT NULL REFERENCES archive_artifacts(sha256),
 receipt_id TEXT NOT NULL REFERENCES archive_receipts(receipt_id),
 locator_json TEXT NOT NULL, excerpt TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS archive_release_statements (
 release_id TEXT NOT NULL REFERENCES archive_releases(release_id),
 statement_id TEXT NOT NULL REFERENCES evidence_statements(statement_id),
 review_id TEXT NOT NULL REFERENCES evidence_reviews(review_id),
 PRIMARY KEY(release_id,statement_id)
);
PRAGMA user_version=29;
"""


class Catalog:
    def __init__(self, path, *, read_only=False):
        self.path = Path(path)
        if read_only:
            self.db = sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True)
        else:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.db = sqlite3.connect(str(self.path))
        self.db.row_factory = sqlite3.Row
        if read_only:
            # Keep compound reports in one snapshot without schema migration.
            self.db.execute('BEGIN')
        else:
            self.db.executescript(SCHEMA)

    @staticmethod
    def prepare_snapshot(path):
        """Read trusted local snapshot metadata without changing the catalog."""
        if not path:
            return _SnapshotMetadata(None, None, None)
        data = Path(path).read_bytes()
        return _SnapshotMetadata(
            dt.datetime.fromtimestamp(Path(path).stat().st_mtime, dt.timezone.utc).isoformat(),
            digest(data), str(path))

    def source(self, sid, url, tier, license, coverage, path=None, revision=None, *, prepared_snapshot=None):
        metadata = self.prepare_snapshot(path) if prepared_snapshot is None else prepared_snapshot
        if (not isinstance(metadata, _SnapshotMetadata)
                or metadata.cache_path != (str(path) if path else None)):
            raise ValueError('Prepared snapshot metadata must come from the matching local snapshot')
        self.db.execute("""INSERT INTO sources
            (id,url,tier,license,coverage,fetched_at,sha256,cache_path,upstream_revision)
            VALUES (?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET
            url=excluded.url,tier=excluded.tier,license=excluded.license,
            coverage=excluded.coverage,fetched_at=excluded.fetched_at,
            sha256=excluded.sha256,cache_path=excluded.cache_path,
            upstream_revision=excluded.upstream_revision""",
            (sid,url,tier,license,coverage,metadata.fetched_at,metadata.sha256,metadata.cache_path,revision))
        # A source import replaces that source's snapshot, not other evidence.
        # Caller rolls back on parser failure, preserving the previous snapshot.
        self.db.execute("DELETE FROM records WHERE source_id=?", (sid,))

    def add(self, sid, key, url, brand, model, title, kind, raw,
            affiliations=(), identifiers=(), facts=(), hyperscaler=(), revision=""):
        # Exact identity only. Never equate models across brands or strip suffixes.
        did = digest(json.dumps([normalize(brand), model.casefold().strip(),
                                 revision.casefold().strip()]).encode())[:20]
        rid = digest(json.dumps([sid, key]).encode())[:24]
        self.db.execute("INSERT OR IGNORE INTO devices VALUES (?,?,?,?,?,?)",
                        (did, brand, model, revision, title, kind))
        self.db.execute("DELETE FROM records WHERE id=?", (rid,))
        self.db.execute("INSERT INTO records VALUES (?,?,?,?,?,?,?)",
                        (rid,did,sid,key,url,now(),json.dumps(raw,ensure_ascii=False)))
        for company, role in affiliations:
            if company:
                self.db.execute("INSERT OR IGNORE INTO affiliations VALUES (?,?,?,?)",
                                (rid,company,target_for(company),role))
        for ikind, value in identifiers:
            if value:
                self.db.execute("INSERT OR IGNORE INTO identifiers VALUES (?,?,?)", (rid,ikind,str(value)))
        for name, value in facts:
            if value is not None and str(value):
                self.db.execute("INSERT OR IGNORE INTO facts VALUES (?,?,?)", (rid,name,str(value)))
        for operator, relationship, detail in hyperscaler:
            self.db.execute("INSERT OR IGNORE INTO hyperscaler_evidence VALUES (?,?,?,?)",
                            (rid,operator,relationship,detail))
        return did

    def finish(self, sid, count, *, reindex=True, commit=True):
        self.db.execute("UPDATE sources SET records_seen=? WHERE id=?", (count,sid))
        # Changed upstream identities should not leave orphan search entries.
        self.db.execute("DELETE FROM devices WHERE id NOT IN (SELECT device_id FROM records)")
        if reindex:
            self.reindex()
        if commit:
            self.db.commit()

    def reindex(self):
        self.db.execute("DELETE FROM search_index")
        for d in self.db.execute("SELECT * FROM devices ORDER BY id").fetchall():
            parts = [d["title"], d["brand"], d["model"], d["kind"], d["revision"]]
            for table, expression in [("affiliations","company || ' ' || role"),
                                      ("identifiers","value"),("facts","value"),
                                      ("hyperscaler_evidence","operator || ' ' || relationship || ' ' || detail"),
                                      ("fcc_exhibits","title || ' ' || document_type || ' ' || availability")]:
                parts.extend(row[0] for row in self.db.execute(
                    f"SELECT {expression} FROM {table} x JOIN records r ON r.id=x.record_id "
                    "WHERE r.device_id=? ORDER BY r.id,1", (d["id"],)))
            text = " ".join(parts)
            # Ports commonly appear as '32x100G'; index speed as its own token.
            speeds = re.findall(r"\d+[x×](\d+(?:\.\d+)?[kmg](?:be?)?)", text, re.I)
            text += " " + " ".join(speeds)
            self.db.execute("INSERT INTO search_index VALUES (?,?)", (d["id"], text))

    def search(self, query="", target=None, hyperscaler=False, limit=100, role=None):
        clauses, args = [], []
        if query.strip():
            # Quote each token: punctuation/model numbers aren't FTS operators.
            tokens = re.findall(r"[^\W_]+", query, flags=re.UNICODE)
            if not tokens:
                return []
            from .validation import IDENTITY_KINDS
            # Discovery fragments do not change identity matching or indexed
            # evidence. Short terms keep the existing word-prefix behavior.
            self.db.create_function('dh_search_contains', 2,
                                    lambda value, term: term.casefold() in (value or '').casefold(),
                                    deterministic=True)
            kinds = sorted(IDENTITY_KINDS)
            for token in tokens:
                clause = 'd.id IN (SELECT device_id FROM search_index WHERE search_index MATCH ?)'
                args.append('"' + token.replace('"', '""') + '"*')
                if len(token) >= 3:
                    clause += (' OR dh_search_contains(d.model, ?) OR d.id IN '
                               '(SELECT r.device_id FROM identifiers i JOIN records r ON r.id=i.record_id '
                               'WHERE i.kind IN (' + ','.join('?' for _ in kinds) + ') '
                               'AND dh_search_contains(i.value, ?))')
                    args.extend([token, *kinds, token])
                clauses.append('(' + clause + ')')
        if target or role:
            sub = "SELECT 1 FROM records r JOIN affiliations a ON a.record_id=r.id WHERE r.device_id=d.id"
            if target:
                canonical = target_for(target)
                if canonical is None:
                    raise ValueError("Unknown target: " + target)
                sub += " AND a.target=?"
                args.append(canonical)
            if role:
                sub += " AND a.role=?"
                args.append(role)
            clauses.append("EXISTS (" + sub + ")")
        if hyperscaler:
            clauses.append("EXISTS (SELECT 1 FROM hyperscaler_evidence h JOIN records r ON r.id=h.record_id WHERE r.device_id=d.id)")
        sql = "SELECT d.* FROM devices d" + (" WHERE " + " AND ".join(clauses) if clauses else "")
        sql += " ORDER BY d.brand,d.model,d.revision LIMIT ?"
        return [dict(r) for r in self.db.execute(sql, args + [limit])]

    def detail(self, did):
        row = self.db.execute("SELECT * FROM devices WHERE id=?", (did,)).fetchone()
        if row is None:
            raise ValueError("Unknown device ID: " + did)
        result = dict(row)
        result["records"] = [dict(r) for r in self.db.execute(
            "SELECT r.*,s.tier,s.license FROM records r JOIN sources s ON s.id=r.source_id WHERE device_id=?",(did,))]
        for table in ["affiliations","identifiers","facts","hyperscaler_evidence","fcc_applications","fcc_exhibits"]:
            result[table] = [dict(r) for r in self.db.execute(
                f"SELECT x.*,r.source_url FROM {table} x JOIN records r ON r.id=x.record_id WHERE r.device_id=?",(did,))]
        names = {result['model']}
        names.update(i['value'] for i in result['identifiers']
                     if i['kind'] in {'model_search', 'manufacturer_model', 'alias'})
        result['registry_matches'] = [dict(r) for r in self.db.execute(
            "SELECT * FROM equipment_register WHERE parse_status='identified' AND model COLLATE NOCASE IN (" +
            ','.join('?' for _ in names) + ") ORDER BY source_id,page,row_number", sorted(names))]
        return result

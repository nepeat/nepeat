-- EFI firmware update images. Each row is one bootable FAT image in R2 that
-- contains a UEFI Shell, a startup.nsh and the vendor's EFI flasher.
--
-- vendor/model are matched against the SMBIOS strings we already collect at
-- check-in (`hosts.manufacturer` / `hosts.product`), which is what makes the
-- "auto-detected system" menu entry possible.
CREATE TABLE firmware (
  id          TEXT PRIMARY KEY,
  vendor      TEXT NOT NULL,
  model       TEXT NOT NULL,
  version     TEXT NOT NULL,
  label       TEXT,               -- menu text; defaults to "<version>" if unset
  r2_key      TEXT NOT NULL UNIQUE,
  size_bytes  INTEGER,
  sha256      TEXT,
  notes       TEXT,
  created_at  INTEGER NOT NULL
);

-- One image per vendor/model/version.
CREATE UNIQUE INDEX idx_firmware_vmv ON firmware(vendor, model, version);
CREATE INDEX idx_firmware_vendor_model ON firmware(vendor, model);

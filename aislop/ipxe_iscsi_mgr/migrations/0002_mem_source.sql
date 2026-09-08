-- `${memsize}` is empty under UEFI, so RAM often comes from an approximate
-- SMBIOS probe. Record which, so an approximation is never mistaken for a
-- measured total. Also split the CPU vendor out of the model string.
ALTER TABLE hosts ADD COLUMN mem_source TEXT;
ALTER TABLE hosts ADD COLUMN cpu_vendor TEXT;

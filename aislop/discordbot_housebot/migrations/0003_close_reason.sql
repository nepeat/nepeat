-- Close reason for /house close (not_interested | bad | purchased)
-- NULL = closed before this field existed or never closed.
ALTER TABLE properties ADD COLUMN close_reason TEXT;

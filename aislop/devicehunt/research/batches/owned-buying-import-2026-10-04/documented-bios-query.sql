SELECT d.model, r.source_key AS guide_record_id, j.key AS source_row_index,
       json_extract(j.value, '$.bios') AS bios_literal,
       json_extract(j.value, '$.certId') AS certification_id,
       j.value AS complete_literal_row, r.source_url
FROM facts f
JOIN records r ON r.id = f.record_id
JOIN devices d ON d.id = r.device_id
JOIN json_each(f.value) j
WHERE r.source_id = 'broadcom-servers'
  AND f.name = 'compatibility_bios_certification_rows'
ORDER BY r.source_key, j.key;
